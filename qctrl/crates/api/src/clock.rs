//! The map clock: how long the current map has been running.
//!
//! # Why this is inferred — unless something *measures* it for us
//!
//! A Quake 2 server does not publish elapsed or remaining map time **on any channel
//! qctrl speaks**. The match clock is `level.time` inside the game DLL (`g_main.c`);
//! it has no cvar, no configstring, and no serverinfo key, and neither RCON `status`
//! nor the OOB status query carries it. From here, there is nothing to read.
//!
//! So by default we infer it: poll the map name once a second, and when it *changes*,
//! that edge is the map start. Elapsed time is then measured from a monotonic `Instant`
//! we hold ourselves.
//!
//! # The honesty constraint
//!
//! Inference has a hole: if qctrl starts up while a map is already running, we never saw
//! its start edge, and no amount of *querying* can recover it. This module refuses to
//! guess. `ClockAnchor` and the `Option<u32>` on `elapsed_seconds` encode that in the
//! type system — `elapsed_seconds` is `None` if and only if the anchor is `Unknown`, so
//! a consumer *cannot* render a countdown that isn't backed by an observed event.
//!
//! # The serverframe beacon closes the hole (Plan 13)
//!
//! What qctrl cannot query, a connected *client* is simply told. yquake2 zeroes
//! `sv.framenum` on every map spawn (`memset(&sv, 0, sizeof(sv))`, `sv_init.c:267`),
//! ticks it at exactly 10 Hz (`sv_main.c:343`), and sends it to **every connected client
//! every frame** (`svc_frame`, `sv_entities.c:425`). So `serverframe / 10` **is** the age
//! of the running map — and qbots has up to 32 clients already decoding it.
//!
//! [`ClockState::observe_frame`] folds that in. It is not a new time model: it is simply a
//! better way to compute the anchor we already keep,
//! `map_start = received_at − serverframe×100ms − age`. Everything downstream is unchanged.
//!
//! The beacon is a **source, not a dependency**: absent it (the default), every branch
//! below takes exactly the path it took before, and the inference above is the whole story.
//! See [`crate::frames`].
//!
//! # The restart-onto-the-same-map problem, and the cvar that never solved it
//!
//! One failure mode is otherwise silent: a server restart onto the *same* map produces no
//! map-name change, so a naive edge detector keeps counting and is confidently wrong.
//!
//! qctrl used to try to catch this with `sv_uptime` — uptime going backwards ⇒ the process
//! restarted. **It never worked.** yquake2 has no `sv_uptime` cvar at all; the only `uptime`
//! in the tree is a local in the *client's* input code. `Cvar_Set` merely *created* the cvar
//! when we set it, and nothing ever read it — which is why the server cheerfully reported
//! `sv_uptime` as `1` while no status reply ever carried an `uptime` key. We were writing a
//! junk cvar to someone's server and then detecting that it had done nothing. All of that is
//! retired; do not bring it back.
//!
//! Today the beacon *measures* the restart within a second: a changed `servercount` means a
//! new level instance, and `serverframe` says exactly how old it is. Without the beacon, the
//! backstop is the `sv_maplist` watchdog, which *guesses* at a restart within a minute (a
//! restart wipes the cvar). Both are strictly better than a cvar that did nothing at all.

use serde::Serialize;
use std::time::{Duration, Instant};

/// Whether we actually know when the current map started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockAnchor {
    /// We observed the map start. Elapsed time is real.
    Exact,
    /// We did not observe the map start and cannot recover it. Elapsed is unknowable.
    Unknown,
}

/// How the anchor was established. Diagnostic; the UI keys off `anchor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockSource {
    /// The polled map name changed.
    ObservedEdge,
    /// qctrl itself issued a `map`/`gamemap` command. Without a beacon, this is the
    /// only way to catch a restart onto the *same* map, which produces no name edge.
    OwnMapCommand,
    /// The server's own frame counter, relayed by a qbots client (Plan 13).
    ///
    /// The only *measured* source. The other two infer a start time from when we noticed
    /// something; this one reads the age off the server. It outranks them both.
    ServerFrame,
    /// No anchor.
    None,
}

/// How much the elapsed figure can be trusted right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockQuality {
    /// Polling is healthy.
    Live,
    /// Polling is failing. The last known elapsed keeps ticking, but the server
    /// may have changed map without us seeing it.
    Degraded,
    /// Elapsed has run past the timelimit and the map still hasn't changed, so
    /// our model disagrees with reality. Shown rather than hidden: a countdown
    /// stuck at zero is a lie, "overtime" is not.
    Overdue,
}

/// The clock as served to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct MapClock {
    pub anchor: ClockAnchor,
    /// `None` if and only if `anchor == Unknown`.
    pub elapsed_seconds: Option<u32>,
    pub quality: ClockQuality,
    pub source: ClockSource,
    pub last_poll_age_seconds: u32,
    /// The server's own frame counter, when a beacon is feeding us. Diagnostic — the UI keys
    /// off `anchor`/`elapsed_seconds` exactly as before.
    pub server_frame: Option<i32>,
    /// Age of the most recent beacon. Grows once the fleet stops; `None` if there never was one.
    pub beacon_age_seconds: Option<u32>,
    /// How many bots are feeding the beacon.
    pub beacon_bots: Option<u32>,
}

/// A poll's worth of server truth, as far as the clock cares.
///
/// Just the map name. The OOB status reply carries no clock of any kind — that is the whole
/// reason [`FrameObservation`] exists.
pub struct Observation<'a> {
    pub map: Option<&'a str>,
}

/// A serverframe beacon, relayed by a qbots client. See [`crate::frames`].
pub struct FrameObservation<'a> {
    pub map: &'a str,
    /// A *new level instance* every time it changes — map change or process restart, and we
    /// deliberately never try to tell which. `SV_InitGame` seeds `svs.spawncount = randk()`
    /// (`sv_init.c:495`) once per server process, so a restarted server returns a **random**
    /// servercount that may be higher *or* lower. **Never compare these with `<` or `>`.**
    /// We don't need to: `serverframe` tells us exactly how old the new level is.
    pub servercount: i32,
    pub serverframe: i32,
    /// How stale the reading already was when it left qbots. Subtracted back out, which is why
    /// qbots' 1 Hz publish rate costs no accuracy at all.
    pub age: Duration,
    pub bots: u32,
}

/// Elapsed is past `timelimit` by more than this before we call it overdue. The
/// server ends the match on its own frame clock, and a map change takes a moment
/// to load and show up in a poll, so a few seconds over is normal, not a bug.
const OVERDUE_GRACE_SECONDS: u32 = 15;

/// A poll that lands within this many seconds counts as healthy. Two poll
/// intervals of slack at the default 1 Hz.
const LIVE_POLL_MAX_AGE_SECONDS: u32 = 3;

/// The Q2 server runs at exactly 10 Hz: `sv.framenum++; sv.time = sv.framenum * 100`
/// (`sv_main.c:343`). This is the constant that turns a frame counter into a clock.
const MS_PER_SERVER_FRAME: u64 = 100;

/// A beacon older than this no longer owns the clock — the fleet has presumably stopped.
///
/// It does **not** invalidate the anchor: that anchor is a fixed monotonic `Instant` and keeps
/// ticking correctly forever on its own. Stopping the bot fleet must not blank a countdown that
/// is still perfectly valid. All that lapses is the beacon's *authority*, so map-edge detection
/// goes back to being the one that spots the next change.
const FRAME_TRUST_MAX_AGE: Duration = Duration::from_secs(3);

/// How far a beacon-derived start may sit from the anchor we already hold before we move it.
///
/// Below this we do **nothing** — re-deriving the anchor on every beacon would make the
/// countdown wobble by each beacon's network latency. Above it, the server's own frame counter
/// is simply right and we are simply wrong, so we take its answer.
const REANCHOR_TOLERANCE: Duration = Duration::from_secs(2);

/// The clock's internal state. Lives inside the status cache.
#[derive(Debug)]
pub struct ClockState {
    map_start: Option<Instant>,
    anchor: ClockAnchor,
    source: ClockSource,
    current_map: Option<String>,

    // ── Serverframe beacon (Plan 13). All `None` when no beacon is configured, in which case
    // every branch keyed off them takes the pre-beacon path. ──
    /// The map the OOB poll says is running. Authoritative for *which* map — the beacon is
    /// authoritative only for *how long* it has been running.
    oob_map: Option<String>,
    last_servercount: Option<i32>,
    last_serverframe: Option<i32>,
    last_frame_map: Option<String>,
    last_frame_at: Option<Instant>,
    beacon_bots: Option<u32>,
}

impl Default for ClockState {
    fn default() -> Self {
        Self {
            map_start: None,
            // Until we observe a map change, we have not seen a map start.
            anchor: ClockAnchor::Unknown,
            source: ClockSource::None,
            current_map: None,
            oob_map: None,
            last_servercount: None,
            last_serverframe: None,
            last_frame_map: None,
            last_frame_at: None,
            beacon_bots: None,
        }
    }
}

impl ClockState {
    /// Fold one successful poll into the clock.
    ///
    /// The state machine:
    ///
    /// | observation      | result                                    |
    /// |------------------|-------------------------------------------|
    /// | no map           | `Unknown` — invalidate; nothing to time    |
    /// | map name changed | `Exact` / `ObservedEdge`                   |
    /// | otherwise        | unchanged; keep ticking                    |
    ///
    /// That is the *whole* of what a poll can tell us. The OOB status reply carries a map name
    /// and nothing resembling a clock — see the module doc. A restart onto the **same** map is
    /// invisible here, by construction; catching it is [`Self::observe_frame`]'s job.
    ///
    /// Every anchor-*mutating* branch below is skipped while a live beacon owns the clock for
    /// the map this poll is reporting ([`Self::frame_owns_the_clock`]) — the beacon has already
    /// anchored it, and more precisely. With no beacon configured that guard is always `false`
    /// and this function is exactly the edge detector it has always been.
    pub fn observe(&mut self, obs: Observation<'_>, now: Instant) {
        let frame_owns = self.frame_owns_the_clock(now, obs.map);

        let map_changed = match (self.current_map.as_deref(), obs.map) {
            (Some(prev), Some(now_map)) => prev != now_map,
            // First sighting of a map is NOT an edge: qctrl may have started
            // mid-map, and we have no way to tell that apart from a real start.
            (None, Some(_)) => false,
            _ => false,
        };

        if let Some(map) = obs.map {
            self.current_map = Some(map.to_string());
            self.oob_map = Some(map.to_string());
        } else {
            // Server has no map (down / between maps). Nothing to time.
            self.current_map = None;
            self.oob_map = None;
            // Unconditional, beacon or not: this is a real signal, and if the server comes
            // back the next beacon re-anchors within a second anyway.
            self.invalidate();
            return;
        }

        if map_changed && !frame_owns {
            // Unless the beacon already anchored this map — it read the age off the server,
            // where this edge only knows when we *noticed*, up to one poll interval late.
            self.map_start = Some(now);
            self.anchor = ClockAnchor::Exact;
            self.source = ClockSource::ObservedEdge;
        }
    }

    /// Fold a serverframe beacon into the clock. **This is the only measured input.**
    ///
    /// The anchor is derived, not guessed:
    ///
    /// ```text
    /// map_start = now − (serverframe × 100ms) − age
    /// ```
    ///
    /// # Why this does not re-anchor on every beacon
    ///
    /// Because it would wobble. Each beacon arrives with its own network latency, so re-deriving
    /// `map_start` every time would jitter the countdown by tens of milliseconds in both
    /// directions. We re-anchor only when something has actually *changed*, and otherwise leave
    /// the held anchor alone — in steady state, that is every single beacon.
    pub fn observe_frame(&mut self, f: FrameObservation<'_>, now: Instant) {
        if f.serverframe < 0 {
            return;
        }

        // The OOB serverinfo is authoritative for WHICH map is running; the beacon only says how
        // long it has run. If they disagree we are straddling a map change — this line is from
        // the level we just left, and anchoring the new map with the old map's age would be
        // exactly the confidently-wrong countdown this whole feature exists to abolish. Skip it;
        // the next one agrees.
        if let Some(oob) = self.oob_map.as_deref() {
            if !oob.eq_ignore_ascii_case(f.map) {
                return;
            }
        }

        let map_age = Duration::from_millis(f.serverframe as u64 * MS_PER_SERVER_FRAME) + f.age;
        let Some(derived) = now.checked_sub(map_age) else {
            // The map claims to be older than this process has existed. Impossible; refuse to
            // anchor rather than anchor somewhere absurd.
            return;
        };

        // ANY change of servercount means a new level instance. We do not — and cannot —
        // classify it as map-change vs restart: `svs.spawncount = randk()` per server process.
        let level_changed = self.last_servercount != Some(f.servercount);
        let beacon_map_changed = self
            .last_frame_map
            .as_deref()
            .is_none_or(|m| !m.eq_ignore_ascii_case(f.map));
        let drifted = match self.map_start {
            Some(start) => derived.max(start) - derived.min(start) > REANCHOR_TOLERANCE,
            None => true,
        };

        let reanchor = self.anchor != ClockAnchor::Exact
            // Precedence: the first beacon after an inferred anchor always takes over. An
            // ObservedEdge anchors when we *noticed*; an OwnMapCommand anchors when we *sent*
            // the rcon, before the server had even loaded the map. Only this one is measured.
            || self.source != ClockSource::ServerFrame
            || level_changed
            || beacon_map_changed
            || drifted;

        if reanchor {
            self.map_start = Some(derived);
            self.anchor = ClockAnchor::Exact;
            self.source = ClockSource::ServerFrame;
        }

        self.last_servercount = Some(f.servercount);
        self.last_serverframe = Some(f.serverframe);
        self.last_frame_map = Some(f.map.to_string());
        self.last_frame_at = Some(now);
        self.beacon_bots = Some(f.bots);
    }

    /// Is a beacon recent enough to be trusted? `false` whenever no beacon is configured.
    fn frame_is_fresh(&self, now: Instant) -> bool {
        self.last_frame_at
            .is_some_and(|t| now.saturating_duration_since(t) <= FRAME_TRUST_MAX_AGE)
    }

    /// Is a live beacon anchoring the clock **for the map this poll is reporting**?
    ///
    /// Both halves matter. A fresh beacon about the *old* map — the bots have not re-handshaked
    /// yet, which takes a second or two after a map change — must **not** suppress the poll's
    /// edge, or the countdown would sit there showing the previous map's elapsed time. So the
    /// edge fires, and the next beacon for the new level corrects it precisely. Whoever sees the
    /// change first anchors; the beacon always gets the last word.
    fn frame_owns_the_clock(&self, now: Instant, oob_map: Option<&str>) -> bool {
        self.frame_is_fresh(now)
            && match (self.last_frame_map.as_deref(), oob_map) {
                (Some(beacon_map), Some(polled)) => beacon_map.eq_ignore_ascii_case(polled),
                _ => false,
            }
    }

    /// An *inferred* restart hint from outside — the `sv_maplist` watchdog spotting a wiped cvar.
    ///
    /// Ignored while a live beacon owns the anchor. The watchdog *guesses* at a restart within a
    /// minute; the beacon *measures* one within a second (a new `servercount`, and a `serverframe`
    /// that says exactly how old the new level is). Honoring the guess would throw away a
    /// correct, measured anchor in favour of a slower, weaker signal.
    pub fn invalidate_inferred(&mut self, now: Instant) {
        if !self.frame_is_fresh(now) {
            self.invalidate();
        }
    }

    /// qctrl issued a `map`/`gamemap` itself. This is the only anchor available
    /// for a restart onto the map already running, which produces no name edge.
    pub fn note_own_map_command(&mut self, map: &str, now: Instant) {
        self.map_start = Some(now);
        self.anchor = ClockAnchor::Exact;
        self.source = ClockSource::OwnMapCommand;
        self.current_map = Some(map.to_string());
    }

    /// Drop the anchor. Elapsed becomes unknowable until the next observed edge.
    pub fn invalidate(&mut self) {
        self.map_start = None;
        self.anchor = ClockAnchor::Unknown;
        self.source = ClockSource::None;
    }

    /// Render the clock as of `now`.
    ///
    /// Elapsed is computed here, at request time, from the monotonic anchor —
    /// not snapshotted at poll time. That means the value the frontend receives
    /// is current as of the response, so it can anchor on it directly without
    /// correcting for how stale the last poll was.
    pub fn snapshot(
        &self,
        now: Instant,
        last_poll: Option<Instant>,
        timelimit_minutes: Option<i32>,
    ) -> MapClock {
        let last_poll_age_seconds = last_poll
            .map(|t| now.saturating_duration_since(t).as_secs() as u32)
            .unwrap_or(u32::MAX);

        let elapsed_seconds = match (self.anchor, self.map_start) {
            (ClockAnchor::Exact, Some(start)) => {
                Some(now.saturating_duration_since(start).as_secs() as u32)
            }
            // The invariant: no anchor, no number. There is nothing honest to put here.
            _ => None,
        };

        let stale = last_poll_age_seconds > LIVE_POLL_MAX_AGE_SECONDS;
        let overdue = match (elapsed_seconds, timelimit_minutes) {
            (Some(elapsed), Some(limit)) if limit > 0 => {
                elapsed > limit as u32 * 60 + OVERDUE_GRACE_SECONDS
            }
            _ => false,
        };

        let quality = if stale {
            ClockQuality::Degraded
        } else if overdue {
            ClockQuality::Overdue
        } else {
            ClockQuality::Live
        };

        MapClock {
            anchor: self.anchor,
            elapsed_seconds,
            quality,
            source: self.source,
            last_poll_age_seconds,
            server_frame: self.last_serverframe,
            // Reported even once stale, and deliberately: a growing age is how the UI can tell
            // "the fleet stopped" from "there was never a beacon".
            beacon_age_seconds: self
                .last_frame_at
                .map(|t| now.saturating_duration_since(t).as_secs() as u32),
            beacon_bots: self.beacon_bots,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn obs(map: &str) -> Observation<'_> {
        Observation { map: Some(map) }
    }

    /// qctrl started while a map was already running. We did not see it start,
    /// so we do not know how long it has run — and we must say so.
    #[test]
    fn first_poll_mid_map_is_unknown() {
        let mut clock = ClockState::default();
        let now = Instant::now();
        clock.observe(obs("q2dm7"), now);

        let snap = clock.snapshot(now, Some(now), Some(10));
        assert_eq!(snap.anchor, ClockAnchor::Unknown);
        assert_eq!(snap.elapsed_seconds, None);
        assert_eq!(snap.source, ClockSource::None);
    }

    #[test]
    fn observed_map_change_anchors_the_clock() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7"), t0);
        clock.observe(obs("q2dm1"), t0);

        let snap = clock.snapshot(t0, Some(t0), Some(10));
        assert_eq!(snap.anchor, ClockAnchor::Exact);
        assert_eq!(snap.elapsed_seconds, Some(0));
        assert_eq!(snap.source, ClockSource::ObservedEdge);
    }

    #[test]
    fn elapsed_counts_up_from_the_anchor() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7"), t0);
        clock.observe(obs("q2dm1"), t0);

        let later = t0 + Duration::from_secs(90);
        let snap = clock.snapshot(later, Some(later), Some(10));
        assert_eq!(snap.elapsed_seconds, Some(90));
    }

    /// A poll CANNOT see a restart onto the same map — no map name changed, so there is no
    /// edge. This asserts the limitation rather than papering over it: the clock keeps counting
    /// the old level. `sv_uptime` was supposed to catch this and never did (see the module doc).
    /// The beacon actually does — see `a_server_restart_onto_the_same_map_re_anchors`.
    #[test]
    fn a_poll_alone_cannot_see_a_restart_onto_the_same_map() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7"), t0);
        clock.observe(obs("q2dm1"), t0);

        // The server restarts onto q2dm1. Nothing in a status reply reveals it.
        let later = t0 + Duration::from_secs(60);
        clock.observe(obs("q2dm1"), later);

        let snap = clock.snapshot(later, Some(later), Some(10));
        assert_eq!(
            snap.elapsed_seconds,
            Some(60),
            "still counting the dead level"
        );
        assert_eq!(snap.source, ClockSource::ObservedEdge);
    }

    /// A restart that also lands on a new map still gets a valid anchor — the map
    /// really did just start, so the edge is genuine.
    #[test]
    fn server_restart_onto_a_new_map_still_anchors() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7"), t0);
        clock.observe(obs("q2dm1"), t0);

        assert_eq!(clock.anchor, ClockAnchor::Exact);
        assert_eq!(clock.source, ClockSource::ObservedEdge);
    }

    #[test]
    fn own_map_command_anchors_a_same_map_restart() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7"), t0);
        assert_eq!(clock.anchor, ClockAnchor::Unknown);

        // qctrl restarts the map it's already on: no name edge will ever fire.
        clock.note_own_map_command("q2dm7", t0);

        let snap = clock.snapshot(t0, Some(t0), Some(10));
        assert_eq!(snap.anchor, ClockAnchor::Exact);
        assert_eq!(snap.source, ClockSource::OwnMapCommand);
        assert_eq!(snap.elapsed_seconds, Some(0));
    }

    #[test]
    fn a_stale_poll_degrades_quality_but_keeps_ticking() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7"), t0);
        clock.observe(obs("q2dm1"), t0);

        let later = t0 + Duration::from_secs(30);
        let snap = clock.snapshot(later, Some(t0), Some(10));
        assert_eq!(snap.quality, ClockQuality::Degraded);
        assert_eq!(snap.elapsed_seconds, Some(30));
        assert_eq!(snap.last_poll_age_seconds, 30);
    }

    #[test]
    fn running_past_the_timelimit_is_overdue() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7"), t0);
        clock.observe(obs("q2dm1"), t0);

        // timelimit 1 = 60s; 90s in with no map change means our model is wrong.
        let later = t0 + Duration::from_secs(90);
        let snap = clock.snapshot(later, Some(later), Some(1));
        assert_eq!(snap.quality, ClockQuality::Overdue);
    }

    #[test]
    fn a_few_seconds_over_the_limit_is_not_overdue() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7"), t0);
        clock.observe(obs("q2dm1"), t0);

        // The server ends the match on its own clock and the new map takes a
        // moment to load; a small overshoot is normal.
        let later = t0 + Duration::from_secs(65);
        let snap = clock.snapshot(later, Some(later), Some(1));
        assert_eq!(snap.quality, ClockQuality::Live);
    }

    #[test]
    fn no_timelimit_is_never_overdue() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7"), t0);
        clock.observe(obs("q2dm1"), t0);

        let later = t0 + Duration::from_secs(100_000);
        let snap = clock.snapshot(later, Some(later), Some(0));
        assert_eq!(snap.quality, ClockQuality::Live);
    }

    #[test]
    fn a_server_with_no_map_has_no_clock() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7"), t0);
        clock.observe(obs("q2dm1"), t0);
        assert_eq!(clock.anchor, ClockAnchor::Exact);

        clock.observe(Observation { map: None }, t0);
        assert_eq!(clock.anchor, ClockAnchor::Unknown);
    }

    // ── Serverframe beacon (Plan 13) ──────────────────────────────────────────────────
    //
    // `Instant` is monotonic from an arbitrary epoch (boot, on Linux). A beacon anchors in the
    // PAST — `now - serverframe*100ms` — so tests must start from a base far enough forward that
    // the subtraction cannot underflow on a freshly-booted machine. Hence this, rather than a
    // bare `Instant::now()`.
    fn base() -> Instant {
        Instant::now() + Duration::from_secs(86_400)
    }

    fn beacon(map: &str, servercount: i32, serverframe: i32) -> FrameObservation<'_> {
        FrameObservation {
            map,
            servercount,
            serverframe,
            age: Duration::ZERO,
            bots: 8,
        }
    }

    /// THE headline case. qctrl starts while a map is already running — which this module's own
    /// doc calls "genuinely unknowable" and which today yields `Unknown` forever. One beacon and
    /// we know the map has been up for exactly 421 seconds, because the server told us so.
    #[test]
    fn a_serverframe_beacon_anchors_a_cold_start() {
        let mut clock = ClockState::default();
        let t0 = base();

        // 4210 frames at 10 Hz = 421.0 s.
        clock.observe_frame(beacon("q2dm7", 1234, 4210), t0);

        let snap = clock.snapshot(t0, Some(t0), Some(10));
        assert_eq!(snap.anchor, ClockAnchor::Exact);
        assert_eq!(snap.source, ClockSource::ServerFrame);
        assert_eq!(snap.elapsed_seconds, Some(421));
        assert_eq!(snap.server_frame, Some(4210));
        assert_eq!(snap.beacon_bots, Some(8));
    }

    /// The anti-jitter rule. Beacons arrive with varying latency; re-deriving the anchor from each
    /// one would wobble the countdown. The anchor must be set once and then left alone.
    #[test]
    fn the_beacon_does_not_re_anchor_on_jitter() {
        let mut clock = ClockState::default();
        let t0 = base();
        clock.observe_frame(beacon("q2dm7", 7, 100), t0); // 10s in

        for i in 1..=60u64 {
            let now = t0 + Duration::from_secs(i);
            let mut obs = beacon("q2dm7", 7, 100 + (i as i32) * 10);
            // Each beacon reports a different staleness — this is the jitter.
            obs.age = Duration::from_millis((i % 5) * 40);
            clock.observe_frame(obs, now);

            let snap = clock.snapshot(now, Some(now), Some(20));
            assert_eq!(
                snap.elapsed_seconds,
                Some(10 + i as u32),
                "elapsed must advance exactly, with no wobble, at t+{i}s"
            );
        }
    }

    /// But a *large* disagreement is not jitter — it means our anchor is wrong. The server's own
    /// frame counter is the authority, so we take its answer.
    #[test]
    fn a_large_drift_re_anchors_to_the_server() {
        let mut clock = ClockState::default();
        let t0 = base();
        clock.observe_frame(beacon("q2dm7", 7, 100), t0); // anchored at 10s in

        // Same level, but the server says we are 300s in — our anchor is 290s off.
        clock.observe_frame(beacon("q2dm7", 7, 3000), t0);

        let snap = clock.snapshot(t0, Some(t0), Some(10));
        assert_eq!(snap.elapsed_seconds, Some(300));
    }

    #[test]
    fn a_servercount_change_re_anchors_at_the_new_level() {
        let mut clock = ClockState::default();
        let t0 = base();
        clock.observe_frame(beacon("q2dm7", 7, 4000), t0); // 400s into q2dm7

        // New level: SV_SpawnServer zeroed sv.framenum, so the counter restarts.
        let later = t0 + Duration::from_secs(1);
        clock.observe_frame(beacon("q2dm1", 8, 20), later);

        let snap = clock.snapshot(later, Some(later), Some(10));
        assert_eq!(snap.elapsed_seconds, Some(2));
        assert_eq!(snap.source, ClockSource::ServerFrame);
    }

    /// The case `sv_uptime` was invented for and never once caught on this engine: a restart onto
    /// the SAME map. There is no name edge, so today the clock sails on counting the old map's
    /// elapsed time, confidently wrong. The beacon simply measures the new level's age.
    ///
    /// Note the servercount goes DOWN. `svs.spawncount = randk()` (`sv_init.c:495`) is seeded
    /// randomly per server process, so a restart can hand back a smaller number. Any code that
    /// compared these with `>` would silently fail here.
    #[test]
    fn a_server_restart_onto_the_same_map_re_anchors() {
        let mut clock = ClockState::default();
        let t0 = base();
        clock.observe_frame(beacon("q2dm7", 2_000_000, 4210), t0); // 421s into q2dm7

        let later = t0 + Duration::from_secs(5);
        clock.observe_frame(beacon("q2dm7", 41, 20), later); // restarted; 2s into the new q2dm7

        let snap = clock.snapshot(later, Some(later), Some(10));
        assert_eq!(
            snap.elapsed_seconds,
            Some(2),
            "a same-map restart must re-anchor, not keep counting the old level"
        );
    }

    /// Precedence. An `ObservedEdge` anchors when the *poll* noticed — up to a poll interval
    /// late. The beacon knows the real age, so it takes over and corrects it.
    #[test]
    fn a_beacon_overrides_a_late_observed_edge_anchor() {
        let mut clock = ClockState::default();
        let t0 = base();
        clock.observe(obs("q2dm7"), t0);
        clock.observe(obs("q2dm1"), t0); // edge → Exact/ObservedEdge, elapsed 0
        assert_eq!(clock.source, ClockSource::ObservedEdge);

        // The map actually started 3s ago; our poll was simply late to notice.
        clock.observe_frame(beacon("q2dm1", 7, 30), t0);

        let snap = clock.snapshot(t0, Some(t0), Some(10));
        assert_eq!(snap.source, ClockSource::ServerFrame);
        assert_eq!(snap.elapsed_seconds, Some(3));
    }

    /// Same, for `OwnMapCommand` — which anchors at the moment we *sent* the rcon, before the
    /// server had even finished loading the map.
    #[test]
    fn a_beacon_corrects_an_own_map_command_anchor() {
        let mut clock = ClockState::default();
        let t0 = base();
        clock.note_own_map_command("q2dm1", t0);
        assert_eq!(clock.source, ClockSource::OwnMapCommand);

        // A moment later the map is actually up, and 1.2s into its life.
        let later = t0 + Duration::from_secs(2);
        clock.observe_frame(beacon("q2dm1", 7, 12), later);

        let snap = clock.snapshot(later, Some(later), Some(10));
        assert_eq!(snap.source, ClockSource::ServerFrame);
        assert_eq!(snap.elapsed_seconds, Some(1));
    }

    /// Stopping the bot fleet must NOT blank the countdown. The anchor is a fixed monotonic
    /// `Instant` — it stays correct on its own. All that lapses is the beacon's authority.
    #[test]
    fn a_stale_beacon_keeps_ticking_but_stops_owning_the_clock() {
        let mut clock = ClockState::default();
        let t0 = base();
        clock.observe_frame(beacon("q2dm7", 7, 100), t0); // 10s in

        // The fleet stops. A minute passes with no beacon.
        let later = t0 + Duration::from_secs(60);
        let snap = clock.snapshot(later, Some(later), Some(20));
        assert_eq!(
            snap.anchor,
            ClockAnchor::Exact,
            "must not blank the countdown"
        );
        assert_eq!(
            snap.elapsed_seconds,
            Some(70),
            "the anchor keeps ticking correctly"
        );
        assert!(
            !clock.frame_is_fresh(later),
            "but it no longer owns the clock"
        );
        assert_eq!(snap.beacon_age_seconds, Some(60));
    }

    /// ...and once stale, map-edge detection is the authority again, exactly as it was before
    /// Plan 13. The edge must fire rather than being suppressed by a beacon nobody is sending.
    #[test]
    fn edge_detection_resumes_once_the_beacon_goes_stale() {
        let mut clock = ClockState::default();
        let t0 = base();
        clock.observe_frame(beacon("q2dm7", 7, 100), t0);
        clock.observe(obs("q2dm7"), t0);

        let later = t0 + Duration::from_secs(60); // beacon long stale
        clock.observe(obs("q2dm1"), later);

        let snap = clock.snapshot(later, Some(later), Some(10));
        assert_eq!(snap.source, ClockSource::ObservedEdge);
        assert_eq!(snap.elapsed_seconds, Some(0));
    }

    /// The map-change race. For a second or two after a map change the bots have not re-handshaked,
    /// so the newest beacon still describes the OLD map. It must not suppress the poll's edge —
    /// otherwise the countdown would sit there showing the previous map's elapsed time.
    #[test]
    fn an_oob_edge_still_anchors_while_the_beacon_is_on_the_old_map() {
        let mut clock = ClockState::default();
        let t0 = base();
        clock.observe_frame(beacon("q2dm7", 7, 3000), t0); // fresh beacon, old map
        clock.observe(obs("q2dm7"), t0);

        // The poll sees the new map first. The beacon is still fresh — but it is about q2dm7.
        let later = t0 + Duration::from_secs(1);
        clock.observe(obs("q2dm1"), later);

        let snap = clock.snapshot(later, Some(later), Some(10));
        assert_eq!(
            snap.source,
            ClockSource::ObservedEdge,
            "a beacon about the old map must not suppress the new map's edge"
        );
        assert_eq!(snap.elapsed_seconds, Some(0));
    }

    /// The mirror image: a beacon line from the level we just left, arriving after the poll has
    /// already moved on. Anchoring the new map with the old map's age is precisely the
    /// confidently-wrong countdown this feature exists to abolish.
    #[test]
    fn a_beacon_disagreeing_with_the_polled_map_is_ignored() {
        let mut clock = ClockState::default();
        let t0 = base();
        clock.observe(obs("q2dm1"), t0); // the poll says q2dm1 is running

        clock.observe_frame(beacon("q2dm7", 7, 5000), t0); // a straggler from q2dm7

        assert_eq!(
            clock.anchor,
            ClockAnchor::Unknown,
            "a beacon for a different map than the one running must not anchor"
        );
    }

    /// The `sv_maplist` watchdog only *guesses* at a restart, and takes up to a minute to do it.
    /// A live beacon *measures* one within a second. Honoring the guess would throw away a
    /// correct anchor.
    #[test]
    fn an_inferred_invalidation_is_ignored_while_a_beacon_is_fresh() {
        let mut clock = ClockState::default();
        let t0 = base();
        clock.observe_frame(beacon("q2dm7", 7, 100), t0);

        clock.invalidate_inferred(t0);

        assert_eq!(clock.anchor, ClockAnchor::Exact);
        assert_eq!(clock.source, ClockSource::ServerFrame);
    }

    /// But with no beacon (the default deployment), it must still invalidate exactly as before.
    #[test]
    fn an_inferred_invalidation_still_works_without_a_beacon() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7"), t0);
        clock.observe(obs("q2dm1"), t0);
        assert_eq!(clock.anchor, ClockAnchor::Exact);

        clock.invalidate_inferred(t0);

        assert_eq!(clock.anchor, ClockAnchor::Unknown);
    }

    /// The core invariant still holds once beacons are in the mix.
    #[test]
    fn elapsed_is_some_iff_anchor_is_exact_with_beacons_interleaved() {
        let mut clock = ClockState::default();
        let t0 = base();

        clock.observe(obs("q2dm7"), t0);
        clock.observe_frame(beacon("q2dm7", 7, 100), t0);
        clock.observe(obs("q2dm7"), t0);
        clock.observe_frame(beacon("q2dm7", 7, 110), t0);
        clock.observe(Observation { map: None }, t0);

        for now in [t0, t0 + Duration::from_secs(5)] {
            let snap = clock.snapshot(now, Some(now), Some(10));
            assert_eq!(
                snap.elapsed_seconds.is_some(),
                snap.anchor == ClockAnchor::Exact,
                "elapsed must be Some iff anchor is Exact"
            );
        }
    }

    /// The core invariant, asserted directly: a number is served if and only if
    /// we observed the map start.
    #[test]
    fn elapsed_is_some_iff_anchor_is_exact() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();

        for map in ["q2dm7", "q2dm1", "q2dm1"] {
            clock.observe(obs(map), t0);
            let snap = clock.snapshot(t0, Some(t0), Some(10));
            assert_eq!(
                snap.elapsed_seconds.is_some(),
                snap.anchor == ClockAnchor::Exact,
                "elapsed must be Some iff anchor is Exact"
            );
        }
    }
}
