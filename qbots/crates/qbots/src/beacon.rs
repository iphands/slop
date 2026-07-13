//! Fleet-wide serverframe beacon — the one number qctrl cannot see (Plan 66).
//!
//! # What this publishes, and why it matters
//!
//! yquake2 zeroes `sv.framenum` on every map spawn (`memset(&sv, 0, sizeof(sv))`,
//! `sv_init.c:267`), advances it at exactly 10 Hz (`sv.framenum++; sv.time =
//! sv.framenum * 100`, `sv_main.c:343`), and writes it to **every connected client
//! every frame** (`svc_frame`, `sv_entities.c:425`). So `serverframe / 10` **is** the
//! age of the running map, to the frame.
//!
//! qctrl cannot see it: it speaks RCON and the connectionless OOB `status` query, and
//! neither carries a frame counter. It therefore *infers* map elapsed time by polling
//! the map name and watching for a change — which cannot recover the age of a map that
//! was already running when qctrl started, and cannot see a restart onto the same map
//! at all. A *client* has the number for free. We have up to 32 of them.
//!
//! # Telemetry, not world state
//!
//! Bots only ever **write** here and never read, so no bot can perceive another through
//! it. That is the same carve-out `stats.rs` documents for `FleetStats`, and the beacon
//! is strictly weaker than a kill tally (AGENTS.md §Concurrency is about *world* state).
//!
//! # The 32-bots-one-message guarantee
//!
//! All 32 bots decode frame N. The socket must carry **one** message for frame N.
//!
//! [`fold`] is the coalescing point: it returns `true` iff the beacon actually advanced,
//! and is handed straight to `watch::Sender::send_if_modified`, whose return value gates
//! the single downstream wakeup. The first bot to report frame N returns `true`; the
//! other 31 return `false` and wake nobody. One fanout task owns the only receiver and
//! is the only writer to the socket.
//!
//! So `messages/sec <= distinct serverframes/sec`, **independent of bot count**. The
//! property belongs to the data flow, not to a timer — which is why `fold` is a free
//! function and is tested without tokio at all.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, watch};

use crate::config::BeaconCfg;
use crate::supervisor::Shutdown;

/// Wire schema version. Bump on any breaking change to [`encode`]'s output; qctrl
/// rejects versions it does not know rather than mis-parsing them.
pub const SCHEMA: u32 = 1;

/// How long a just-retired `servercount` stays untrusted.
///
/// At a level change the fleet does not switch atomically: for a few ms a straggler's
/// last packet from the *old* level can land after another bot's first packet from the
/// *new* one. Without this grace the beacon would flap backwards and forwards across
/// the boundary.
const LEVEL_FLAP_GRACE: Duration = Duration::from_secs(2);

/// One bot's report of a freshly decoded server frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BotFrame {
    pub servercount: i32,
    pub serverframe: i32,
}

/// The single fleet-wide beacon value. Only the newest frame survives.
///
/// The published position is a pure function of `(servercount, serverframe)`. `map` rides
/// along as a passenger and never triggers a message on its own — that is what keeps the
/// message count tied to server ticks rather than to bot activity.
#[derive(Clone, Debug, Default)]
pub struct BeaconState {
    pub servercount: i32,
    pub serverframe: i32,
    /// Bare map name (`q2dm1`). Empty until the level's configstring 33 arrives.
    pub map: String,
    pub observed_at: Option<Instant>,
    /// Incremented once per accepted frame. This is the message counter: qctrl sees it
    /// advance by exactly 1 per published line, so a gap means a dropped line.
    pub seq: u64,
    /// The servercount we just left, and when. Not published; see [`LEVEL_FLAP_GRACE`].
    retired: Option<(i32, Instant)>,
}

impl BeaconState {
    /// Is `sc` the level we *just* left, recently enough that a straggler is the likely
    /// explanation rather than a genuine change back?
    fn is_retired(&self, sc: i32, now: Instant) -> bool {
        self.retired.is_some_and(|(old, at)| {
            old == sc && now.saturating_duration_since(at) < LEVEL_FLAP_GRACE
        })
    }
}

/// Fold one bot's frame into the fleet beacon.
///
/// Returns `true` **iff the beacon changed** — i.e. iff this bot is the first to report
/// this frame. This return value is the whole coalescing mechanism; see the module doc.
///
/// # Level changes
///
/// `servercount != previous` means **a new level instance exists**. Full stop. We do not
/// classify it as map-change-vs-restart, and we must not try: `SV_InitGame` seeds
/// `svs.spawncount = randk()` (`sv_init.c:495`) once per server *process*, so a restarted
/// server returns a **random** servercount that may be higher or lower than the old one.
/// **Never compare servercounts with `<` or `>`.** We don't need to — `serverframe` tells
/// us exactly how old the new level is, which is the entire point of the beacon.
pub fn fold(cur: &mut BeaconState, obs: BotFrame, map: &str, now: Instant) -> bool {
    if obs.serverframe < 0 {
        return false;
    }

    let same_level = obs.servercount == cur.servercount;
    let accept = match cur.observed_at {
        // Nothing yet: anything is news.
        None => true,
        // Steady state — the only branch that runs 99.99% of the time. Strictly greater,
        // so a bot whose packet arrived late cannot rewind the fleet's position.
        Some(_) if same_level => obs.serverframe > cur.serverframe,
        // A different level. Accept it unless it is the one we just left (a straggler).
        Some(_) => !cur.is_retired(obs.servercount, now),
    };
    if !accept {
        return false;
    }

    if !same_level && cur.observed_at.is_some() {
        cur.retired = Some((cur.servercount, now));
    }
    cur.servercount = obs.servercount;
    cur.serverframe = obs.serverframe;
    cur.observed_at = Some(now);
    // Only touch the String when it actually differs, so the accept path allocates at
    // most once per level rather than once per frame.
    if cur.map != map {
        cur.map = map.to_string();
    }
    cur.seq += 1;
    true
}

/// What the fanout task last put on the wire. Drives [`should_write`].
#[derive(Clone, Copy, Debug)]
pub struct Written {
    pub at: Instant,
    pub servercount: i32,
}

/// Should the fanout task write a line right now?
///
/// - A **level change publishes immediately** — qctrl re-anchors its map clock off this,
///   and making it wait out a heartbeat interval would show a stale countdown for a second.
/// - Otherwise, at most once per `interval`.
/// - The interval also acts as a **heartbeat**: it fires even when the frame counter has
///   not moved, so a wedged server surfaces as a growing `age_ms` rather than as silence.
pub fn should_write(
    st: &BeaconState,
    last: Option<&Written>,
    now: Instant,
    interval: Duration,
) -> bool {
    if st.observed_at.is_none() {
        return false;
    }
    match last {
        None => true,
        Some(w) if w.servercount != st.servercount => true,
        Some(w) => now.saturating_duration_since(w.at) >= interval,
    }
}

/// One line on the wire. Field order here **is** the JSON field order, and it is pinned
/// by `encode_pins_the_wire_format` against qctrl's `the_wire_format_matches_what_qbots_emits`.
#[derive(Serialize)]
struct Line<'a> {
    v: u32,
    /// Resolved `ip:port` the bots are actually connected to. qctrl rejects a beacon whose
    /// server is not the one it manages — without this, a fleet pointed at a test server
    /// would silently drive the production map clock.
    server: &'a str,
    /// The server as *named* in qbots' config, for the (common) case where the two sides
    /// spell the same host differently.
    server_name: &'a str,
    map: &'a str,
    servercount: i32,
    serverframe: i32,
    /// How stale this reading already was when it left qbots. This is what makes a 1 Hz
    /// publish interval cost **zero** accuracy: qctrl reconstructs the map start as
    /// `received_at - serverframe*100ms - age_ms`.
    age_ms: u64,
    bots: u32,
    seq: u64,
}

/// Render the beacon as one NDJSON line.
///
/// `None` when there is nothing worth publishing: no frame yet, or the map name has not
/// arrived. **A frame must never be published against the wrong map name** — qctrl keys
/// its re-anchor on the map matching what its own poll sees, so a mislabelled frame is
/// worse than no frame.
pub fn encode(
    st: &BeaconState,
    server: &str,
    server_name: &str,
    bots: u32,
    now: Instant,
) -> Option<String> {
    let observed_at = st.observed_at?;
    if st.map.is_empty() {
        return None;
    }
    let line = Line {
        v: SCHEMA,
        server,
        server_name,
        map: &st.map,
        servercount: st.servercount,
        serverframe: st.serverframe,
        age_ms: now.saturating_duration_since(observed_at).as_millis() as u64,
        bots,
        seq: st.seq,
    };
    serde_json::to_string(&line).ok()
}

/// RAII "this bot is Active" counter. `+1` on creation, `-1` on drop — so every `bot_task`
/// exit path (clean return, error, panic unwind) decrements exactly once.
pub struct ActiveBot(Arc<AtomicU32>);

impl Drop for ActiveBot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Clone-cheap fleet beacon handle, passed to every bot task exactly like `FleetStats`.
#[derive(Clone)]
pub struct Beacon {
    tx: Arc<watch::Sender<BeaconState>>,
    /// Bots currently `Active`. Deliberately **not** part of the watch value: a bot
    /// joining or leaving must not be able to emit a message.
    bots: Arc<AtomicU32>,
}

impl Default for Beacon {
    fn default() -> Self {
        // The initial receiver is dropped: `send_if_modified` works with none attached, and
        // the fanout task makes its own via `subscribe()`.
        let (tx, _rx) = watch::channel(BeaconState::default());
        Self {
            tx: Arc::new(tx),
            bots: Arc::new(AtomicU32::new(0)),
        }
    }
}

impl Beacon {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold a freshly decoded frame in. Called from `bot_task` on the ack-on-frame edge —
    /// the hottest path in the program, so this is non-async, lock-light, and allocates
    /// only when the beacon actually advances onto a new map.
    pub fn on_frame(&self, servercount: i32, serverframe: i32, map: &str, now: Instant) {
        self.tx.send_if_modified(|cur| {
            fold(
                cur,
                BotFrame {
                    servercount,
                    serverframe,
                },
                map,
                now,
            )
        });
    }

    /// Count this bot as Active until the returned guard drops.
    pub fn bot_active(&self) -> ActiveBot {
        self.bots.fetch_add(1, Ordering::Relaxed);
        ActiveBot(Arc::clone(&self.bots))
    }

    pub fn bots(&self) -> u32 {
        self.bots.load(Ordering::Relaxed)
    }

    pub fn subscribe(&self) -> watch::Receiver<BeaconState> {
        self.tx.subscribe()
    }
}

/// Bind the beacon socket, reclaiming a corpse but never stealing a live one.
///
/// A socket file left behind by a SIGKILLed run looks identical to one a running qbots is
/// listening on. The difference is observable: **connect to it**. If the connect *succeeds*,
/// another qbots owns this path and we must back off — unlinking it would silently steal
/// qctrl's feed out from under the other fleet. If it *fails*, nobody is home and the file
/// is a corpse, so reclaiming it is correct.
async fn bind(path: &Path, mode: u32) -> std::io::Result<UnixListener> {
    if path.exists() {
        if UnixStream::connect(path).await.is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "another qbots is already publishing on this socket",
            ));
        }
        std::fs::remove_file(path)?;
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(listener)
}

/// The single writer. Owns the only [`watch::Receiver`], so the "one message per tick"
/// guarantee cannot be broken by adding readers: readers subscribe to the *broadcast*,
/// downstream of the coalescing point.
async fn fanout(
    beacon: Beacon,
    tx: broadcast::Sender<Arc<str>>,
    interval: Duration,
    server_addr: String,
    server_name: String,
    shutdown: Shutdown,
) {
    let mut rx = beacon.subscribe();
    let mut last: Option<Written> = None;

    while !shutdown.requested() {
        // Either a bot advanced the beacon, or the heartbeat came due. `watch` collapses any
        // number of sends between two wakeups into one value, so a burst cannot queue up.
        tokio::select! {
            changed = rx.changed() => {
                if changed.is_err() {
                    break; // every bot task is gone
                }
            }
            _ = tokio::time::sleep(interval) => {}
        }

        let state = rx.borrow_and_update().clone();
        let now = Instant::now();
        if !should_write(&state, last.as_ref(), now, interval) {
            continue;
        }
        let Some(line) = encode(&state, &server_addr, &server_name, beacon.bots(), now) else {
            continue; // no frame yet, or the map name hasn't landed
        };

        last = Some(Written {
            at: now,
            servercount: state.servercount,
        });
        // Errs only when nobody is listening, which is the normal idle case.
        let _ = tx.send(Arc::from(format!("{line}\n")));
    }
}

/// Pump broadcast lines to one connected reader (qctrl). A reader that errors or falls
/// hopelessly behind is dropped; qctrl reconnects on its own.
async fn write_client(mut stream: UnixStream, mut rx: broadcast::Receiver<Arc<str>>) {
    loop {
        match rx.recv().await {
            Ok(line) => {
                if stream.write_all(line.as_bytes()).await.is_err() {
                    return; // reader hung up
                }
            }
            // A slow reader missing heartbeats is not our problem: the next line it does get
            // carries `age_ms`, so it re-syncs exactly. No need to drop it.
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                tracing::debug!(skipped, "beacon reader lagged");
            }
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

/// Run the beacon until shutdown.
///
/// **A beacon failure must never take the fleet down.** Every error path here logs and
/// disables the beacon; bots keep playing. The beacon is telemetry, not a dependency.
pub async fn serve(
    beacon: Beacon,
    cfg: BeaconCfg,
    server_addr: String,
    server_name: String,
    shutdown: Shutdown,
) {
    let path = cfg.socket_path.clone();
    let listener = match bind(&path, cfg.socket_mode).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!(
                path = %path.display(),
                error = %e,
                "beacon disabled: could not bind socket (the fleet is unaffected)"
            );
            return;
        }
    };
    tracing::info!(path = %path.display(), "beacon listening");

    // Floor the interval: a 0 here would make the fanout task spin.
    let interval = Duration::from_millis(cfg.publish_interval_ms.max(50));
    let (tx, _idle) = broadcast::channel::<Arc<str>>(8);
    let fan = tokio::spawn(fanout(
        beacon,
        tx.clone(),
        interval,
        server_addr,
        server_name,
        shutdown.clone(),
    ));

    let readers = Arc::new(AtomicU32::new(0));
    while !shutdown.requested() {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    if readers.load(Ordering::Relaxed) as usize >= cfg.max_clients {
                        tracing::warn!(max = cfg.max_clients, "beacon: reader cap reached, refusing");
                        continue; // `stream` drops, closing the connection
                    }
                    readers.fetch_add(1, Ordering::Relaxed);
                    tracing::info!("beacon reader connected");
                    let rx = tx.subscribe();
                    let readers = Arc::clone(&readers);
                    tokio::spawn(async move {
                        write_client(stream, rx).await;
                        readers.fetch_sub(1, Ordering::Relaxed);
                        tracing::info!("beacon reader disconnected");
                    });
                }
                Err(e) => tracing::warn!(error = %e, "beacon accept failed"),
            },
            _ = shutdown.sleep_or_cancel(Duration::from_millis(250)) => {}
        }
    }

    fan.abort();
    // Leave no corpse for the next run to have to probe.
    let _ = std::fs::remove_file(&path);
    tracing::info!("beacon stopped");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(servercount: i32, serverframe: i32) -> BotFrame {
        BotFrame {
            servercount,
            serverframe,
        }
    }

    #[test]
    fn the_first_report_of_a_frame_is_accepted() {
        let mut st = BeaconState::default();
        let t0 = Instant::now();
        assert!(fold(&mut st, frame(7, 100), "q2dm1", t0));
        assert_eq!(st.serverframe, 100);
        assert_eq!(st.servercount, 7);
        assert_eq!(st.map, "q2dm1");
        assert_eq!(st.seq, 1);
    }

    /// THE hard requirement (Plan 66). Thirty-two bots all decode frame 100. Exactly one
    /// of them may move the beacon; the socket must carry one message, not thirty-two.
    #[test]
    fn thirty_two_bots_reporting_one_frame_produce_one_message() {
        let mut st = BeaconState::default();
        let t0 = Instant::now();

        let messages = (0..32)
            .filter(|_| fold(&mut st, frame(7, 100), "q2dm1", t0))
            .count();

        assert_eq!(
            messages, 1,
            "32 bots on one frame must produce exactly one message"
        );
        assert_eq!(st.seq, 1);
    }

    /// The same property over a stream: 32 bots x 10 frames is 320 reports and must be
    /// 10 messages. If this ever reads 320, the fanout has started scaling with bot count.
    #[test]
    fn a_fleet_tick_stream_yields_one_message_per_frame_not_one_per_bot() {
        let mut st = BeaconState::default();
        let t0 = Instant::now();

        let mut messages = 0;
        for f in 1..=10 {
            for _bot in 0..32 {
                if fold(&mut st, frame(7, f), "q2dm1", t0) {
                    messages += 1;
                }
            }
        }

        assert_eq!(messages, 10, "one message per server frame, not per bot");
        assert_eq!(st.seq, 10);
        assert_eq!(st.serverframe, 10);
    }

    #[test]
    fn a_lagging_bot_cannot_rewind_the_beacon() {
        let mut st = BeaconState::default();
        let t0 = Instant::now();
        fold(&mut st, frame(7, 100), "q2dm1", t0);

        // A packet from the same level that took the scenic route.
        assert!(!fold(&mut st, frame(7, 98), "q2dm1", t0));
        assert_eq!(st.serverframe, 100);
        assert_eq!(st.seq, 1);
    }

    #[test]
    fn a_level_change_resets_the_frame_counter() {
        let mut st = BeaconState::default();
        let t0 = Instant::now();
        fold(&mut st, frame(7, 900), "q2dm1", t0);

        // New level: SV_SpawnServer zeroed sv.framenum, so the counter goes BACKWARDS
        // and that is correct.
        assert!(fold(
            &mut st,
            frame(8, 3),
            "q2dm3",
            t0 + Duration::from_secs(1)
        ));
        assert_eq!(st.serverframe, 3);
        assert_eq!(st.servercount, 8);
        assert_eq!(st.map, "q2dm3");
        assert_eq!(st.seq, 2);
    }

    /// Pins the `randk()` finding (`sv_init.c:495`): `svs.spawncount` is seeded RANDOMLY
    /// per server process, so a restarted server can return a LOWER servercount. If someone
    /// "optimises" `fold` into a `>` comparison, this test is what stops them.
    #[test]
    fn a_lower_servercount_is_still_a_level_change() {
        let mut st = BeaconState::default();
        let t0 = Instant::now();
        fold(&mut st, frame(2_000_000, 900), "q2dm1", t0);

        // Server process restarted; randk() happened to come back small.
        assert!(fold(
            &mut st,
            frame(41, 5),
            "q2dm1",
            t0 + Duration::from_secs(1)
        ));
        assert_eq!(st.servercount, 41);
        assert_eq!(st.serverframe, 5);
    }

    #[test]
    fn a_straggler_on_the_old_level_cannot_flap_the_beacon_back() {
        let mut st = BeaconState::default();
        let t0 = Instant::now();
        fold(&mut st, frame(7, 900), "q2dm1", t0);
        fold(&mut st, frame(8, 3), "q2dm3", t0);

        // A bot that hadn't re-handshaked yet delivers its last old-level frame.
        assert!(!fold(
            &mut st,
            frame(7, 901),
            "q2dm1",
            t0 + Duration::from_millis(50)
        ));
        assert_eq!(st.servercount, 8, "must not flap back to the old level");
        assert_eq!(st.serverframe, 3);
    }

    /// The flap guard is a short grace, not a permanent ban: the server really can be put
    /// back onto a servercount we saw before (randk() collision, or a rotation returning).
    #[test]
    fn the_flap_guard_expires() {
        let mut st = BeaconState::default();
        let t0 = Instant::now();
        fold(&mut st, frame(7, 900), "q2dm1", t0);
        fold(&mut st, frame(8, 3), "q2dm3", t0);

        let later = t0 + LEVEL_FLAP_GRACE + Duration::from_millis(1);
        assert!(fold(&mut st, frame(7, 10), "q2dm1", later));
        assert_eq!(st.servercount, 7);
    }

    /// The map name is a passenger. A bot correcting the map on a frame we already have
    /// must not emit a message — otherwise bot count leaks back into the message rate.
    #[test]
    fn the_map_name_rides_along_without_emitting_a_message() {
        let mut st = BeaconState::default();
        let t0 = Instant::now();
        fold(&mut st, frame(7, 100), "", t0);
        assert_eq!(st.seq, 1);

        // Same frame, now with the map known. No new message.
        assert!(!fold(&mut st, frame(7, 100), "q2dm1", t0));
        assert_eq!(st.seq, 1);
    }

    #[test]
    fn a_negative_serverframe_is_rejected() {
        let mut st = BeaconState::default();
        assert!(!fold(&mut st, frame(7, -1), "q2dm1", Instant::now()));
        assert_eq!(st.seq, 0);
    }

    #[test]
    fn a_frame_with_no_map_yet_is_not_published() {
        let mut st = BeaconState::default();
        let t0 = Instant::now();
        fold(&mut st, frame(7, 100), "", t0);

        // Publishing this would attribute a frame to an unknown map. qctrl would have to
        // guess which map it belongs to; better to say nothing for the ~1s until CS 33 lands.
        assert_eq!(encode(&st, "1.2.3.4:27910", "noir.lan:27910", 32, t0), None);
    }

    #[test]
    fn an_empty_beacon_is_not_published() {
        let st = BeaconState::default();
        assert_eq!(
            encode(&st, "1.2.3.4:27910", "noir.lan:27910", 0, Instant::now()),
            None
        );
    }

    /// The cross-repo contract. qbots and qctrl share no crate by design (that would make
    /// the coupling mandatory), so this golden line and qctrl's
    /// `the_wire_format_matches_what_qbots_emits` are the ONLY thing keeping the two sides
    /// honest. Change one, change the other, and bump `SCHEMA`.
    #[test]
    fn encode_pins_the_wire_format() {
        let t0 = Instant::now();
        let mut st = BeaconState::default();
        fold(&mut st, frame(1234, 4210), "q2dm1", t0);
        st.seq = 99;

        let line = encode(
            &st,
            "192.168.1.10:27910",
            "noir.lan:27910",
            32,
            t0 + Duration::from_millis(250),
        )
        .expect("a beacon with a map and a frame must encode");

        assert_eq!(
            line,
            r#"{"v":1,"server":"192.168.1.10:27910","server_name":"noir.lan:27910","map":"q2dm1","servercount":1234,"serverframe":4210,"age_ms":250,"bots":32,"seq":99}"#
        );
    }

    #[test]
    fn should_write_says_nothing_when_there_is_nothing_to_say() {
        let st = BeaconState::default();
        assert!(!should_write(
            &st,
            None,
            Instant::now(),
            Duration::from_secs(1)
        ));
    }

    #[test]
    fn should_write_publishes_a_level_change_immediately() {
        let t0 = Instant::now();
        let mut st = BeaconState::default();
        fold(&mut st, frame(8, 3), "q2dm3", t0);

        // Written 1ms ago on the OLD level — far inside the 1s interval, but a level change
        // must not wait: qctrl re-anchors its countdown off this line.
        let last = Written {
            at: t0,
            servercount: 7,
        };
        assert!(should_write(
            &st,
            Some(&last),
            t0 + Duration::from_millis(1),
            Duration::from_secs(1)
        ));
    }

    #[test]
    fn should_write_rate_limits_steady_state() {
        let t0 = Instant::now();
        let mut st = BeaconState::default();
        fold(&mut st, frame(7, 100), "q2dm1", t0);
        let last = Written {
            at: t0,
            servercount: 7,
        };

        assert!(!should_write(
            &st,
            Some(&last),
            t0 + Duration::from_millis(999),
            Duration::from_secs(1)
        ));
        assert!(should_write(
            &st,
            Some(&last),
            t0 + Duration::from_secs(1),
            Duration::from_secs(1)
        ));
    }

    /// A server whose frame counter has frozen (wedged, or every bot dropped) must still
    /// heartbeat, so qctrl sees a growing `age_ms` and can age the beacon out — rather than
    /// silence, which is indistinguishable from a dead socket.
    #[test]
    fn should_write_heartbeats_a_frozen_server() {
        let t0 = Instant::now();
        let mut st = BeaconState::default();
        fold(&mut st, frame(7, 100), "q2dm1", t0);

        let last = Written {
            at: t0,
            servercount: 7,
        };
        // No new frame has arrived, seq is unchanged — but the interval has elapsed.
        assert!(should_write(
            &st,
            Some(&last),
            t0 + Duration::from_secs(1),
            Duration::from_secs(1)
        ));
    }

    /// The same guarantee as `thirty_two_bots_...`, but through the real `watch` channel:
    /// what we actually care about is how many times the *publisher* is woken.
    #[tokio::test]
    async fn the_watch_channel_never_wakes_the_publisher_more_than_once_per_frame() {
        let beacon = Beacon::new();
        let mut rx = beacon.subscribe();

        let consumer = tokio::spawn(async move {
            let mut wakeups = 0u32;
            while rx.changed().await.is_ok() {
                wakeups += 1;
                if rx.borrow_and_update().serverframe == 10 {
                    break;
                }
            }
            wakeups
        });

        // 10 frames, each reported by all 32 bots: 320 calls.
        for f in 1..=10 {
            let mut bots = Vec::new();
            for _ in 0..32 {
                let b = beacon.clone();
                bots.push(tokio::spawn(async move {
                    b.on_frame(7, f, "q2dm1", Instant::now());
                }));
            }
            for b in bots {
                b.await.unwrap();
            }
        }

        let wakeups = consumer.await.unwrap();
        assert!(
            wakeups <= 10,
            "320 bot reports across 10 frames woke the publisher {wakeups} times; \
             the cap is one per frame (watch may coalesce further, which is fine)"
        );
    }

    #[tokio::test]
    async fn the_active_bot_guard_counts_down_on_drop() {
        let beacon = Beacon::new();
        assert_eq!(beacon.bots(), 0);
        {
            let _a = beacon.bot_active();
            let _b = beacon.bot_active();
            assert_eq!(beacon.bots(), 2);
        }
        assert_eq!(beacon.bots(), 0, "guards must decrement on every exit path");
    }

    fn test_cfg(dir: &tempfile::TempDir) -> BeaconCfg {
        BeaconCfg {
            enabled: true,
            socket_path: dir.path().join("beacon.sock"),
            publish_interval_ms: 50,
            socket_mode: 0o600,
            max_clients: 4,
        }
    }

    /// The one socket-touching test: a real bind, a real connect, a real line.
    #[tokio::test]
    async fn serves_a_line_over_a_unix_socket_and_unlinks_on_shutdown() {
        use tokio::io::AsyncBufReadExt;

        let dir = tempfile::tempdir().unwrap();
        let cfg = test_cfg(&dir);
        let path = cfg.socket_path.clone();

        let beacon = Beacon::new();
        let shutdown = Shutdown::new();
        let _guard = beacon.bot_active();

        let server = tokio::spawn(serve(
            beacon.clone(),
            cfg,
            "192.168.1.10:27910".to_string(),
            "noir.lan:27910".to_string(),
            shutdown.clone(),
        ));

        // Wait for the listener to exist, then connect as qctrl would.
        let stream = loop {
            match UnixStream::connect(&path).await {
                Ok(s) => break s,
                Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        };
        let mut lines = tokio::io::BufReader::new(stream).lines();

        beacon.on_frame(1234, 4210, "q2dm1", Instant::now());

        let line = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
            .await
            .expect("a line must arrive within 5s")
            .unwrap()
            .expect("the stream must not close");

        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["v"], 1);
        assert_eq!(v["map"], "q2dm1");
        assert_eq!(v["serverframe"], 4210);
        assert_eq!(v["servercount"], 1234);
        assert_eq!(v["server"], "192.168.1.10:27910");
        assert_eq!(v["bots"], 1);

        shutdown.fire();
        server.await.unwrap();
        assert!(
            !path.exists(),
            "the socket file must be unlinked on shutdown, not left as a corpse"
        );
    }

    /// A socket file whose owner was SIGKILLed is a corpse: nobody is listening, so
    /// reclaiming it is correct and the beacon must start.
    #[tokio::test]
    async fn a_stale_socket_file_is_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("beacon.sock");
        std::fs::write(&path, b"corpse").unwrap();
        assert!(path.exists());

        let listener = bind(&path, 0o600)
            .await
            .expect("a corpse must be reclaimed");
        drop(listener);
    }

    /// But a socket somebody is *actually listening on* belongs to another qbots. Stealing it
    /// would silently redirect qctrl's feed to the wrong fleet, so we must refuse to bind.
    #[tokio::test]
    async fn a_live_socket_is_never_stolen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("beacon.sock");
        let _incumbent = UnixListener::bind(&path).unwrap();

        let err = bind(&path, 0o600)
            .await
            .expect_err("must refuse to steal a live socket");
        assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
        assert!(path.exists(), "the incumbent's socket must survive");
    }
}
