//! Who advances the map when a match ends.
//!
//! # Why this has to exist at all
//!
//! Quake 2's intermission never ends by itself. When the timelimit is hit,
//! `CheckDMRules` calls `EndDMLevel`, which calls `BeginIntermission`, and from
//! then on `CheckDMRules` returns at the top — the match is over and the server
//! is parked. The *only* thing in the deathmatch game DLL that ever sets
//! `level.exitintermission` is `ClientThink` (`yquake2 game/player/client.c:2122`),
//! and it requires a **connected client to press `BUTTON_ANY`, at least five
//! seconds in**. There is no timeout, no maximum intermission length, and no
//! empty-server special case. An idle Q2 server sits in intermission forever.
//!
//! `sv_maplist` does not save us. It only decides *which* map the changelevel
//! points at (`g_main.c:236-279`); it does nothing to make the exit fire. So
//! "just keep sv_maplist in sync and let the server rotate itself" — the story
//! qctrl used to tell itself — is not a fallback. It is a deadlock.
//!
//! Rotation therefore needs an owner outside the game. It used to be the
//! browser: a React hook fired `map <next>` over rcon. That worked, but only
//! while a tab was open, which is why an unattended server would sit at the end
//! of a match until someone loaded the frontend. This module moves that
//! ownership into the backend, where it runs headless.
//!
//! `sv_maplist` sync stays exactly as it was — it is still the right destination
//! when a *real* player presses fire and the server exits intermission on its
//! own. This module is the owner for when nobody does.
//!
//! # The two triggers
//!
//! [`decide`] is pure, so the policy below is testable without a server.

use crate::clock::{ClockAnchor, ClockQuality, MapClock};
use crate::rotation::RotationMode;
use std::time::Duration;

/// Rotate this many seconds *before* the server's own timelimit.
///
/// Beating the server to the punch means `EndDMLevel` never runs and
/// intermission never happens at all, which is the good path: no five-second
/// scoreboard freeze that nothing is guaranteed to release.
///
/// Five seconds is enough because the clock is anchored on an observed map
/// change and is accurate to roughly one poll interval (1s by default).
pub const EARLY_FIRE_SECONDS: u32 = 5;

/// How far past the timelimit the rescue trigger waits before forcing a map.
///
/// Only relevant when the clock has no anchor, where we are guessing from
/// "how long have we been staring at this same map name" rather than from a
/// real start time. The grace keeps us from cutting a map short on a server
/// that is about to rotate itself.
pub const RESCUE_GRACE_SECONDS: u32 = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RotationDecision {
    /// Nothing to do.
    Hold,
    /// Send `map <name>`.
    Rotate(String),
}

/// Everything the rotator knows on one tick. Borrowed, so `decide` stays pure.
pub struct Tick<'a> {
    pub clock: &'a MapClock,
    pub timelimit_minutes: Option<i32>,
    pub current_map: Option<&'a str>,
    pub enabled: bool,
    pub maps: &'a [String],
    pub mode: RotationMode,
    /// True while a rotation is in flight or its cooldown has not expired.
    pub cooling_down: bool,
    /// How long the rotator has continuously observed `current_map`. This is the
    /// rotator's own measurement, not the clock's — it is available even when the
    /// clock has no anchor, which is the whole point of the rescue trigger.
    pub map_seen_for: Duration,
    /// Random-mode tiebreaker, supplied by the caller so this stays deterministic.
    pub pick: u64,
}

/// Should we rotate right now?
///
/// | clock anchor | rule                                                     |
/// |--------------|----------------------------------------------------------|
/// | `Exact`      | preempt: fire at `timelimit - EARLY_FIRE_SECONDS`        |
/// | `Unknown`    | rescue: fire once the same map has been up for            |
/// |              | `timelimit + RESCUE_GRACE_SECONDS`                        |
///
/// The rescue rule exists because `Unknown` — qctrl started while a map was
/// already running — makes the preempt rule structurally unable to fire. Before
/// this module, that state deferred to "the server's own rotation", which does
/// not exist, so an idle server deadlocked permanently.
///
/// Note which way the error runs. `map_seen_for` is how long *we* have watched
/// the map, which is always less than or equal to how long it has actually been
/// running — we can only have missed time, never invented it. So the rescue can
/// only fire **late**, never early: it will never cut a live match short, it will
/// at worst let one run long by up to a timelimit. That is the right direction to
/// be wrong in, and it is bounded to once — the anchor becomes `Exact` at the
/// next map change and never comes back here.
///
/// Deliberately *not* done here: weakening `clock.rs`. `MapClock` refuses to
/// report an elapsed time it did not observe, and that invariant is worth
/// keeping. The pessimism lives here, where it is a policy choice rather than a
/// claim of fact.
pub fn decide(t: &Tick<'_>) -> RotationDecision {
    if !t.enabled || t.cooling_down {
        return RotationDecision::Hold;
    }

    // Polling is failing, so we cannot see what map is up or whether it already
    // changed. Firing blind would just queue rcon at a server we cannot hear.
    if t.clock.quality == ClockQuality::Degraded {
        return RotationDecision::Hold;
    }

    let Some(current) = t.current_map else {
        return RotationDecision::Hold;
    };

    // No timelimit means the match never ends on the clock, so there is nothing
    // for us to preempt. (Fraglimit is intentionally not handled: the server runs
    // its end-of-match logic the same frame the limit is hit, so qctrl cannot win
    // that race — see the module docs on why that is survivable.)
    let Some(limit_minutes) = t.timelimit_minutes.filter(|l| *l > 0) else {
        return RotationDecision::Hold;
    };
    let limit_seconds = limit_minutes as u32 * 60;

    let due = match (t.clock.anchor, t.clock.elapsed_seconds) {
        // `elapsed + EARLY >= limit` rather than `elapsed >= limit - EARLY` so a
        // pathologically short timelimit cannot underflow.
        (ClockAnchor::Exact, Some(elapsed)) => elapsed + EARLY_FIRE_SECONDS >= limit_seconds,
        (ClockAnchor::Unknown, _) => {
            t.map_seen_for.as_secs() >= (limit_seconds + RESCUE_GRACE_SECONDS) as u64
        }
        // Exact with no elapsed is impossible by clock.rs's own invariant.
        (ClockAnchor::Exact, None) => false,
    };

    if !due {
        return RotationDecision::Hold;
    }

    match select_next(t.mode, t.maps, Some(current), t.pick) {
        Some(next) => RotationDecision::Rotate(next),
        None => RotationDecision::Hold,
    }
}

/// Pick the map to play next.
///
/// This is the backend port of what used to be `determineNextMap` in the
/// frontend. It living in JS is why `mode: Random` was silently ignored whenever
/// a browser was not driving the rotation — the server's own `sv_maplist` walk is
/// always sequential.
///
/// `pick` is any random `u64`; it is only consulted in `Random` mode. Passing it
/// in rather than drawing it here keeps this function deterministic and testable.
pub fn select_next(
    mode: RotationMode,
    maps: &[String],
    current_map: Option<&str>,
    pick: u64,
) -> Option<String> {
    if maps.is_empty() {
        return None;
    }

    let index_of_current =
        current_map.and_then(|c| maps.iter().position(|m| m.eq_ignore_ascii_case(c.trim())));

    match mode {
        RotationMode::Sequential => {
            // Not in the queue, or the last entry: wrap to the front.
            let next = match index_of_current {
                Some(i) if i + 1 < maps.len() => i + 1,
                _ => 0,
            };
            Some(maps[next].clone())
        }
        RotationMode::Random => {
            // Never pick the map we are already on — replaying it looks like the
            // rotation is broken. Unless it is the only map we have, in which
            // case replaying it is the honest answer.
            let pool: Vec<&String> = match index_of_current {
                Some(i) if maps.len() > 1 => maps
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .map(|(_, m)| m)
                    .collect(),
                _ => maps.iter().collect(),
            };
            Some(pool[(pick % pool.len() as u64) as usize].clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ClockSource;

    fn maps(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn clock(anchor: ClockAnchor, elapsed: Option<u32>, quality: ClockQuality) -> MapClock {
        MapClock {
            anchor,
            elapsed_seconds: elapsed,
            quality,
            source: ClockSource::ObservedEdge,
            server_uptime_seconds: None,
            last_poll_age_seconds: 0,
            // The rotator reads only anchor/elapsed/quality; the beacon fields are diagnostic.
            server_frame: None,
            beacon_age_seconds: None,
            beacon_bots: None,
        }
    }

    fn tick<'a>(clock: &'a MapClock, maps: &'a [String], current: Option<&'a str>) -> Tick<'a> {
        Tick {
            clock,
            timelimit_minutes: Some(10),
            current_map: current,
            enabled: true,
            maps,
            mode: RotationMode::Sequential,
            cooling_down: false,
            map_seen_for: Duration::ZERO,
            pick: 0,
        }
    }

    // ---- select_next: sequential ----

    #[test]
    fn sequential_advances_to_the_next_map() {
        let m = maps(&["q2dm1", "q2dm2", "q2dm3"]);
        let next = select_next(RotationMode::Sequential, &m, Some("q2dm1"), 0);
        assert_eq!(next.as_deref(), Some("q2dm2"));
    }

    #[test]
    fn sequential_wraps_at_the_end_of_the_queue() {
        let m = maps(&["q2dm1", "q2dm2", "q2dm3"]);
        let next = select_next(RotationMode::Sequential, &m, Some("q2dm3"), 0);
        assert_eq!(next.as_deref(), Some("q2dm1"));
    }

    /// The map we are on isn't in the queue (someone forced it by hand, or it was
    /// removed mid-match). Start the queue from the top rather than giving up.
    #[test]
    fn sequential_starts_at_the_front_when_the_current_map_is_not_queued() {
        let m = maps(&["q2dm1", "q2dm2"]);
        let next = select_next(RotationMode::Sequential, &m, Some("kessel"), 0);
        assert_eq!(next.as_deref(), Some("q2dm1"));
    }

    #[test]
    fn sequential_starts_at_the_front_when_there_is_no_current_map() {
        let m = maps(&["q2dm5", "q2dm6"]);
        let next = select_next(RotationMode::Sequential, &m, None, 0);
        assert_eq!(next.as_deref(), Some("q2dm5"));
    }

    /// The OOB reply's casing is not guaranteed to match what the user typed into
    /// the queue, and Q2 map names are case-insensitive on disk anyway.
    #[test]
    fn the_current_map_is_matched_case_insensitively() {
        let m = maps(&["q2dm1", "q2dm2"]);
        let next = select_next(RotationMode::Sequential, &m, Some("Q2DM1"), 0);
        assert_eq!(next.as_deref(), Some("q2dm2"));
    }

    // ---- select_next: random ----

    /// Replaying the map you are already on reads as a broken rotation, and it is
    /// avoidable whenever there is anything else to pick.
    #[test]
    fn random_never_returns_the_current_map() {
        let m = maps(&["q2dm1", "q2dm2", "q2dm3", "q2dm4"]);
        for pick in 0..64u64 {
            let next = select_next(RotationMode::Random, &m, Some("q2dm2"), pick).unwrap();
            assert_ne!(next, "q2dm2", "pick={pick} returned the current map");
            assert!(m.contains(&next));
        }
    }

    #[test]
    fn random_reaches_every_other_map() {
        let m = maps(&["q2dm1", "q2dm2", "q2dm3"]);
        let seen: std::collections::HashSet<String> = (0..32u64)
            .map(|pick| select_next(RotationMode::Random, &m, Some("q2dm1"), pick).unwrap())
            .collect();
        assert_eq!(seen.len(), 2, "should reach both q2dm2 and q2dm3");
    }

    /// One map in the queue and it is the one running. Replaying it is the only
    /// honest answer — the alternative is refusing to rotate, which strands the
    /// server in intermission.
    #[test]
    fn random_replays_a_single_map_queue() {
        let m = maps(&["q2dm1"]);
        let next = select_next(RotationMode::Random, &m, Some("q2dm1"), 7);
        assert_eq!(next.as_deref(), Some("q2dm1"));
    }

    #[test]
    fn an_empty_queue_has_no_next_map() {
        assert_eq!(
            select_next(RotationMode::Sequential, &[], Some("q2dm1"), 0),
            None
        );
        assert_eq!(
            select_next(RotationMode::Random, &[], Some("q2dm1"), 0),
            None
        );
    }

    // ---- decide: the preempt trigger ----

    #[test]
    fn preempt_fires_early_fire_seconds_before_the_timelimit() {
        let m = maps(&["q2dm1", "q2dm2"]);
        let c = clock(
            ClockAnchor::Exact,
            Some(600 - EARLY_FIRE_SECONDS),
            ClockQuality::Live,
        );
        let t = tick(&c, &m, Some("q2dm1"));
        assert_eq!(decide(&t), RotationDecision::Rotate("q2dm2".into()));
    }

    #[test]
    fn preempt_holds_one_second_too_early() {
        let m = maps(&["q2dm1", "q2dm2"]);
        let c = clock(
            ClockAnchor::Exact,
            Some(600 - EARLY_FIRE_SECONDS - 1),
            ClockQuality::Live,
        );
        let t = tick(&c, &m, Some("q2dm1"));
        assert_eq!(decide(&t), RotationDecision::Hold);
    }

    /// We lost the race (no rotator running, server already parked in
    /// intermission). The preempt condition is still true, so we still fire —
    /// which is what unsticks the server. `Overdue` must not suppress it.
    #[test]
    fn an_overdue_clock_still_rotates() {
        let m = maps(&["q2dm1", "q2dm2"]);
        let c = clock(ClockAnchor::Exact, Some(900), ClockQuality::Overdue);
        let t = tick(&c, &m, Some("q2dm1"));
        assert_eq!(decide(&t), RotationDecision::Rotate("q2dm2".into()));
    }

    // ---- decide: the rescue trigger ----

    /// The deadlock this whole module exists to break: no anchor, so the preempt
    /// rule can never fire, and Q2 will never leave intermission on its own.
    #[test]
    fn rescue_fires_once_the_same_map_outlives_the_timelimit() {
        let m = maps(&["q2dm1", "q2dm2"]);
        let c = clock(ClockAnchor::Unknown, None, ClockQuality::Live);
        let mut t = tick(&c, &m, Some("q2dm1"));
        t.map_seen_for = Duration::from_secs((600 + RESCUE_GRACE_SECONDS) as u64);
        assert_eq!(decide(&t), RotationDecision::Rotate("q2dm2".into()));
    }

    #[test]
    fn rescue_holds_inside_the_grace_window() {
        let m = maps(&["q2dm1", "q2dm2"]);
        let c = clock(ClockAnchor::Unknown, None, ClockQuality::Live);
        let mut t = tick(&c, &m, Some("q2dm1"));
        t.map_seen_for = Duration::from_secs((600 + RESCUE_GRACE_SECONDS - 1) as u64);
        assert_eq!(decide(&t), RotationDecision::Hold);
    }

    /// An unanchored clock on a map we just started watching must not rotate —
    /// otherwise a qctrl restart would cut the running match short immediately.
    #[test]
    fn rescue_does_not_fire_right_after_a_qctrl_restart() {
        let m = maps(&["q2dm1", "q2dm2"]);
        let c = clock(ClockAnchor::Unknown, None, ClockQuality::Live);
        let t = tick(&c, &m, Some("q2dm1"));
        assert_eq!(decide(&t), RotationDecision::Hold);
    }

    // ---- decide: the guards ----

    #[test]
    fn disabled_rotation_never_fires() {
        let m = maps(&["q2dm1", "q2dm2"]);
        let c = clock(ClockAnchor::Exact, Some(600), ClockQuality::Live);
        let mut t = tick(&c, &m, Some("q2dm1"));
        t.enabled = false;
        assert_eq!(decide(&t), RotationDecision::Hold);
    }

    #[test]
    fn a_rotation_in_flight_never_double_fires() {
        let m = maps(&["q2dm1", "q2dm2"]);
        let c = clock(ClockAnchor::Exact, Some(600), ClockQuality::Live);
        let mut t = tick(&c, &m, Some("q2dm1"));
        t.cooling_down = true;
        assert_eq!(decide(&t), RotationDecision::Hold);
    }

    #[test]
    fn no_timelimit_means_no_timed_rotation() {
        let m = maps(&["q2dm1", "q2dm2"]);
        let c = clock(ClockAnchor::Exact, Some(100_000), ClockQuality::Live);
        let mut t = tick(&c, &m, Some("q2dm1"));
        t.timelimit_minutes = Some(0);
        assert_eq!(decide(&t), RotationDecision::Hold);

        t.timelimit_minutes = None;
        assert_eq!(decide(&t), RotationDecision::Hold);
    }

    /// Polling is broken, so we do not actually know what is running. Firing rcon
    /// blind at a server we cannot hear from just burns the rcon budget.
    #[test]
    fn a_degraded_clock_holds() {
        let m = maps(&["q2dm1", "q2dm2"]);
        let c = clock(ClockAnchor::Exact, Some(600), ClockQuality::Degraded);
        let t = tick(&c, &m, Some("q2dm1"));
        assert_eq!(decide(&t), RotationDecision::Hold);
    }

    #[test]
    fn an_empty_queue_holds_rather_than_sending_an_empty_map() {
        let c = clock(ClockAnchor::Exact, Some(600), ClockQuality::Live);
        let t = tick(&c, &[], Some("q2dm1"));
        assert_eq!(decide(&t), RotationDecision::Hold);
    }

    #[test]
    fn a_server_with_no_map_holds() {
        let m = maps(&["q2dm1", "q2dm2"]);
        let c = clock(ClockAnchor::Unknown, None, ClockQuality::Live);
        let mut t = tick(&c, &m, None);
        t.map_seen_for = Duration::from_secs(100_000);
        assert_eq!(decide(&t), RotationDecision::Hold);
    }
}
