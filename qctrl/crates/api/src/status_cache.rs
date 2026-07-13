//! Cached server status, fed by a background poller.
//!
//! # Why a cache
//!
//! `/api/status` used to do a live RCON round-trip per request. Six frontend
//! components poll the `['status']` query key and react-query dedupes them to
//! the shortest interval (2s), so the server saw an RCON `status` every 2
//! seconds against a `sv_rcon_limit` of 1/sec. Past that limit a Q2 server
//! answers *every* command with `Bad rcon_password` (see `context/pitfalls.md`),
//! which looks exactly like a misconfigured password. Serving requests from a
//! cache decouples "how often the UI asks" from "how often we touch the server".
//!
//! # Why the poller is a hybrid
//!
//! The OOB status query is free (own rate limit, no password) and carries the
//! map, the cvars, and each player's frags/ping/name. But `SV_StatusString`
//! emits *no client numbers and no addresses*, and `clientkick` needs a client
//! number. So RCON `status` is still polled — just slowly, as an identity table,
//! rather than on every request.
//!
//! OOB is authoritative for who is connected and what their score is; RCON only
//! supplies the client-number and address columns.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::clock::{ClockState, FrameObservation, MapClock, Observation};
use crate::oob::OobStatus;
use crate::status::{Player, StatusResponse};

/// A client number we could not resolve. The frontend must disable kick/ban on
/// these rather than send `clientkick -1` and boot whoever happens to be there.
pub const UNKNOWN_CLIENT_NUM: i32 = -1;

#[derive(Default)]
struct Inner {
    map: Option<String>,
    dmflags: Option<i32>,
    timelimit: Option<i32>,
    fraglimit: Option<i32>,
    maxclients: Option<i32>,

    /// Truth for who is connected, from the 1 Hz OOB poll.
    oob_players: Vec<crate::oob::OobPlayer>,
    /// Identity columns only (client_num, address), from the slow RCON poll.
    rcon_players: Vec<Player>,
    rcon_players_at: Option<Instant>,

    clock: ClockState,
    last_ok: Option<Instant>,
    consecutive_failures: u32,
}

/// Shared, cheap-to-read server status.
#[derive(Clone)]
pub struct StatusCache {
    inner: Arc<RwLock<Inner>>,
    refresh: Arc<tokio::sync::Notify>,
}

impl Default for StatusCache {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner::default())),
            refresh: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Wake the poller now instead of waiting for its next tick. Called after a
    /// successful RCON mutation so the UI's follow-up read doesn't see a
    /// pre-change cache.
    pub fn request_refresh(&self) {
        self.refresh.notify_one();
    }

    pub async fn refresh_requested(&self) {
        self.refresh.notified().await;
    }

    /// Fold a successful OOB poll into the cache.
    pub fn apply_oob(&self, status: OobStatus, now: Instant) {
        let mut inner = self.inner.write().unwrap();

        let map = status.get("mapname").map(str::to_string);

        inner.clock.observe(
            Observation {
                map: map.as_deref(),
            },
            now,
        );

        inner.map = map;
        inner.dmflags = status.get_int("dmflags");
        inner.timelimit = status.get_int("timelimit");
        inner.fraglimit = status.get_int("fraglimit");
        inner.maxclients = status.get_int("maxclients");
        inner.oob_players = status.players;
        inner.last_ok = Some(now);
        inner.consecutive_failures = 0;
    }

    /// Fold a successful RCON `status` poll into the cache. Only the identity
    /// columns are taken; scores and the connected set come from OOB.
    pub fn apply_rcon_identity(&self, players: Vec<Player>, now: Instant) {
        let mut inner = self.inner.write().unwrap();
        inner.rcon_players = players;
        inner.rcon_players_at = Some(now);
    }

    /// Record a failed poll. Returns the new consecutive-failure count so the
    /// caller can log the first failure only and not flood on an outage.
    pub fn note_failure(&self) -> u32 {
        let mut inner = self.inner.write().unwrap();
        inner.consecutive_failures += 1;
        inner.consecutive_failures
    }

    /// qctrl issued its own `map`/`gamemap`. See `ClockState::note_own_map_command`.
    pub fn note_own_map_command(&self, map: &str, now: Instant) {
        self.inner
            .write()
            .unwrap()
            .clock
            .note_own_map_command(map, now);
    }

    /// Fold a qbots serverframe beacon into the clock (Plan 13). See `crate::frames`.
    ///
    /// Deliberately does **not** touch `last_ok` or `consecutive_failures`. A beacon proves the
    /// *game server* is alive; it says nothing about whether *qctrl* can reach it. `server_online`
    /// and `ClockQuality::Degraded` must stay honest about qctrl's own polling, because the
    /// rotator holds on `Degraded` — and a qctrl that cannot talk to the server has no business
    /// rotating it, however healthy the beacon looks.
    pub fn apply_beacon(&self, beacon: &crate::frames::Beacon, now: Instant) {
        self.inner.write().unwrap().clock.observe_frame(
            FrameObservation {
                map: &beacon.map,
                servercount: beacon.servercount,
                serverframe: beacon.serverframe,
                age: Duration::from_millis(beacon.age_ms),
                bots: beacon.bots,
            },
            now,
        );
    }

    /// The server *probably* restarted — the `sv_maplist` watchdog found the cvar wiped.
    ///
    /// This is an inference, and a slow one (up to 60 s). It is ignored while a live beacon owns
    /// the anchor, because the beacon *measures* a restart within a second. See
    /// `ClockState::invalidate_inferred`.
    pub fn invalidate_clock(&self) {
        self.inner
            .write()
            .unwrap()
            .clock
            .invalidate_inferred(Instant::now());
    }

    pub fn needs_rcon_identity(&self, now: Instant, max_age: std::time::Duration) -> bool {
        let inner = self.inner.read().unwrap();

        // Never polled, or the table has aged out.
        let stale = match inner.rcon_players_at {
            None => true,
            Some(at) => now.saturating_duration_since(at) >= max_age,
        };
        if stale {
            return true;
        }

        // The set of connected names changed, so at least one client number is
        // unresolved. Refresh now rather than leave a player unkickable.
        let known: std::collections::HashSet<&str> =
            inner.rcon_players.iter().map(|p| p.name.as_str()).collect();
        inner
            .oob_players
            .iter()
            .any(|p| !known.contains(p.name.as_str()))
    }

    /// The current status, as of *now*. A pure read — no network.
    pub fn snapshot(&self) -> StatusResponse {
        let inner = self.inner.read().unwrap();
        let now = Instant::now();

        let clock: MapClock = inner.clock.snapshot(now, inner.last_ok, inner.timelimit);

        StatusResponse {
            map: inner.map.clone(),
            dmflags: inner.dmflags,
            timelimit: inner.timelimit,
            fraglimit: inner.fraglimit,
            maxclients: inner.maxclients,
            players: merge_players(&inner.oob_players, &inner.rcon_players),
            server_online: inner.last_ok.is_some() && inner.consecutive_failures == 0,
            clock,
        }
    }
}

/// Merge the OOB player list (truth) with the RCON identity table.
///
/// Matched by name, because that is the only column the two share. A name that
/// appears more than once in *either* list is ambiguous, and guessing which
/// client number belongs to which player would mean `clientkick` boots the wrong
/// person. So ambiguity resolves to `UNKNOWN_CLIENT_NUM` and the UI disables the
/// action — a disabled button is a much better failure than a wrong kick.
fn merge_players(oob: &[crate::oob::OobPlayer], rcon: &[Player]) -> Vec<Player> {
    let mut by_name: HashMap<&str, Option<&Player>> = HashMap::new();
    for p in rcon {
        by_name
            .entry(p.name.as_str())
            // Second sighting of a name: ambiguous, so no identity for either.
            .and_modify(|slot| *slot = None)
            .or_insert(Some(p));
    }

    let mut oob_name_counts: HashMap<&str, usize> = HashMap::new();
    for p in oob {
        *oob_name_counts.entry(p.name.as_str()).or_default() += 1;
    }

    let mut players: Vec<Player> = oob
        .iter()
        .map(|p| {
            let unambiguous = oob_name_counts.get(p.name.as_str()) == Some(&1);
            let identity = if unambiguous {
                by_name.get(p.name.as_str()).copied().flatten()
            } else {
                None
            };

            Player {
                client_num: identity.map_or(UNKNOWN_CLIENT_NUM, |i| i.client_num),
                address: identity.map_or_else(String::new, |i| i.address.clone()),
                score: p.frags,
                ping: p.ping,
                name: p.name.clone(),
            }
        })
        .collect();

    players.sort_by_key(|p| std::cmp::Reverse(p.score));
    players
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oob::OobPlayer;

    fn oob(name: &str, frags: i32) -> OobPlayer {
        OobPlayer {
            frags,
            ping: 30,
            name: name.into(),
        }
    }

    fn rcon(name: &str, client_num: i32) -> Player {
        Player {
            client_num,
            score: 0,
            address: format!("10.0.0.{}:27901", client_num + 1),
            name: name.into(),
            ping: 0,
        }
    }

    #[test]
    fn merges_identity_onto_oob_truth() {
        let players = merge_players(&[oob("Alice", 10)], &[rcon("Alice", 3)]);
        assert_eq!(players.len(), 1);
        assert_eq!(players[0].client_num, 3);
        assert_eq!(players[0].address, "10.0.0.4:27901");
        // Score comes from OOB, not from the stale RCON table.
        assert_eq!(players[0].score, 10);
    }

    #[test]
    fn oob_decides_who_is_connected() {
        // RCON table is stale and still lists a player who has left.
        let players = merge_players(&[oob("Alice", 10)], &[rcon("Alice", 3), rcon("Ghost", 4)]);
        assert_eq!(players.len(), 1);
        assert_eq!(players[0].name, "Alice");
    }

    #[test]
    fn an_unseen_player_has_no_client_num() {
        // Connected per OOB, but the slow RCON table hasn't caught up yet.
        let players = merge_players(&[oob("Newbie", 0)], &[]);
        assert_eq!(players[0].client_num, UNKNOWN_CLIENT_NUM);
        assert_eq!(players[0].address, "");
    }

    /// The important one: two players sharing a name must NOT get a guessed
    /// client number, because kicking the wrong one is worse than not kicking.
    #[test]
    fn duplicate_names_refuse_to_guess_an_identity() {
        let players = merge_players(
            &[oob("Player", 10), oob("Player", 2)],
            &[rcon("Player", 1), rcon("Player", 2)],
        );
        assert_eq!(players.len(), 2);
        for p in &players {
            assert_eq!(
                p.client_num, UNKNOWN_CLIENT_NUM,
                "an ambiguous name must never resolve to a client number"
            );
        }
    }

    #[test]
    fn a_duplicate_name_does_not_poison_other_players() {
        let players = merge_players(
            &[oob("Player", 10), oob("Player", 2), oob("Alice", 5)],
            &[rcon("Player", 1), rcon("Player", 2), rcon("Alice", 3)],
        );
        let alice = players.iter().find(|p| p.name == "Alice").unwrap();
        assert_eq!(alice.client_num, 3);
    }

    #[test]
    fn players_are_sorted_by_score_descending() {
        let players = merge_players(&[oob("Low", 1), oob("High", 20), oob("Mid", 7)], &[]);
        let names: Vec<&str> = players.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["High", "Mid", "Low"]);
    }

    #[test]
    fn negative_frags_sort_last() {
        let players = merge_players(&[oob("Bot", -9), oob("Human", 3)], &[]);
        assert_eq!(players[0].name, "Human");
        assert_eq!(players[1].score, -9);
    }

    #[test]
    fn a_fresh_cache_is_offline_and_unknown() {
        let cache = StatusCache::new();
        let snap = cache.snapshot();
        assert!(!snap.server_online);
        assert_eq!(snap.map, None);
        // Nothing observed yet, so there is no honest elapsed to report.
        assert_eq!(snap.clock.elapsed_seconds, None);
    }

    #[test]
    fn identity_refresh_is_needed_when_a_new_name_appears() {
        let cache = StatusCache::new();
        let now = Instant::now();
        let status = crate::oob::parse_oob_status("\\mapname\\q2dm1\n5 30 \"Alice\"").unwrap();
        cache.apply_oob(status, now);
        cache.apply_rcon_identity(vec![rcon("Alice", 1)], now);

        assert!(!cache.needs_rcon_identity(now, std::time::Duration::from_secs(30)));

        // Bob joins: OOB sees him immediately, but he has no client number yet.
        let status =
            crate::oob::parse_oob_status("\\mapname\\q2dm1\n5 30 \"Alice\"\n0 40 \"Bob\"").unwrap();
        cache.apply_oob(status, now);
        assert!(cache.needs_rcon_identity(now, std::time::Duration::from_secs(30)));
    }

    fn beacon(map: &str, servercount: i32, serverframe: i32) -> crate::frames::Beacon {
        crate::frames::Beacon {
            v: 1,
            server: "192.168.1.10:27910".into(),
            server_name: "noir.lan:27910".into(),
            map: map.into(),
            servercount,
            serverframe,
            age_ms: 0,
            bots: 12,
            seq: 1,
        }
    }

    /// A beacon anchors the clock exactly — 4210 frames at 10 Hz is 421 seconds.
    #[test]
    fn apply_beacon_anchors_the_clock() {
        let cache = StatusCache::new();
        // Instant is monotonic from boot; a beacon anchors in the past, so start far enough
        // forward that `now - 421s` cannot underflow on a freshly-booted machine.
        let now = Instant::now() + Duration::from_secs(86_400);

        cache.apply_beacon(&beacon("q2dm7", 1234, 4210), now);

        let snap = cache.snapshot();
        let clock = snap.clock;
        assert_eq!(clock.anchor, crate::clock::ClockAnchor::Exact);
        assert_eq!(clock.source, crate::clock::ClockSource::ServerFrame);
        assert_eq!(clock.server_frame, Some(4210));
        assert_eq!(clock.beacon_bots, Some(12));
    }

    /// A beacon proves the GAME SERVER is alive. It says nothing about whether QCTRL can reach it
    /// — those are different facts, and conflating them would be dangerous: the rotator holds when
    /// the clock is `Degraded`, and a qctrl that cannot talk to the server has no business
    /// rotating it, however healthy the bots' view looks.
    #[test]
    fn apply_beacon_does_not_fake_server_online() {
        let cache = StatusCache::new();
        let now = Instant::now() + Duration::from_secs(86_400);

        cache.apply_beacon(&beacon("q2dm7", 1234, 4210), now);

        let snap = cache.snapshot();
        assert!(
            !snap.server_online,
            "a beacon must not make an unreachable server look online"
        );
    }
}
