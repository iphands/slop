//! The map clock: how long the current map has been running.
//!
//! # Why this is inferred and not read
//!
//! A Quake 2 server does not publish elapsed or remaining map time. The match
//! clock is `level.time` inside the game DLL (`g_main.c`); it has no cvar, no
//! configstring, and no serverinfo key, and neither RCON `status` nor the OOB
//! status query carries it. There is nothing to read.
//!
//! So we infer it: poll the map name once a second, and when it *changes*, that
//! edge is the map start. Elapsed time is then measured from a monotonic
//! `Instant` we hold ourselves.
//!
//! # The honesty constraint
//!
//! Inference has a hole: if qctrl starts up while a map is already running, we
//! never saw its start edge, and no amount of querying can recover it. That case
//! is genuinely unknowable, and this module refuses to guess. `ClockAnchor` and
//! the `Option<u32>` on `elapsed_seconds` encode that in the type system —
//! `elapsed_seconds` is `None` if and only if the anchor is `Unknown`, so a
//! consumer *cannot* render a countdown that isn't backed by an observed event.
//!
//! # Why sv_uptime matters (when the engine has it)
//!
//! One failure mode would otherwise be silent: a server restart onto the *same*
//! map produces no map-name change, so a naive edge detector keeps counting and
//! is confidently wrong. `sv_uptime` (second-resolution, monotonic within a
//! server process) catches it — uptime going backwards means the process
//! restarted, which invalidates the anchor.
//!
//! But `sv_uptime` is a q2pro/q2repro cvar; **yquake2 does not have it**, and the
//! observed status replies from the current server carry no `uptime` key. So the
//! clock treats uptime as strictly optional: everything works without it, and the
//! uptime-based checks below simply never fire. The backstop for that case is the
//! `sv_maplist` watchdog, which spots a restart (a restart wipes the cvar) and
//! calls `invalidate` — within a minute rather than within a second.

use serde::Serialize;
use std::time::Instant;

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
    /// qctrl itself issued a `map`/`gamemap` command. This is the only way to
    /// catch a restart onto the *same* map, which produces no name edge.
    OwnMapCommand,
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
    pub server_uptime_seconds: Option<u64>,
    pub last_poll_age_seconds: u32,
}

/// A poll's worth of server truth, as far as the clock cares.
pub struct Observation<'a> {
    pub map: Option<&'a str>,
    pub uptime_seconds: Option<u64>,
}

/// Elapsed is past `timelimit` by more than this before we call it overdue. The
/// server ends the match on its own frame clock, and a map change takes a moment
/// to load and show up in a poll, so a few seconds over is normal, not a bug.
const OVERDUE_GRACE_SECONDS: u32 = 15;

/// A poll that lands within this many seconds counts as healthy. Two poll
/// intervals of slack at the default 1 Hz.
const LIVE_POLL_MAX_AGE_SECONDS: u32 = 3;

/// The clock's internal state. Lives inside the status cache.
#[derive(Debug)]
pub struct ClockState {
    map_start: Option<Instant>,
    anchor: ClockAnchor,
    source: ClockSource,
    current_map: Option<String>,
    last_uptime: Option<u64>,
}

impl Default for ClockState {
    fn default() -> Self {
        Self {
            map_start: None,
            // Until we observe a map change, we have not seen a map start.
            anchor: ClockAnchor::Unknown,
            source: ClockSource::None,
            current_map: None,
            last_uptime: None,
        }
    }
}

impl ClockState {
    /// Fold one successful poll into the clock.
    ///
    /// The state machine, in the order the checks must happen:
    ///
    /// | observation                          | result                          |
    /// |--------------------------------------|---------------------------------|
    /// | server restarted (uptime went back)  | `Unknown` — invalidate          |
    /// | map name changed                     | `Exact` / `ObservedEdge`        |
    /// | anchor claims more elapsed than the  | `Unknown` — impossible, so our  |
    /// | server has been up                   | anchor is bogus                 |
    /// | otherwise                            | unchanged; keep ticking         |
    ///
    /// The restart check runs *before* the edge check on purpose: a restart that
    /// also lands on a different map is still a restart, and re-anchoring on the
    /// edge is correct there anyway (the map genuinely just started).
    pub fn observe(&mut self, obs: Observation<'_>, now: Instant) {
        let restarted = match (self.last_uptime, obs.uptime_seconds) {
            // Monotonic within a server process, so a decrease means a new process.
            (Some(prev), Some(now_up)) => now_up < prev,
            _ => false,
        };
        if let Some(up) = obs.uptime_seconds {
            self.last_uptime = Some(up);
        }

        let map_changed = match (self.current_map.as_deref(), obs.map) {
            (Some(prev), Some(now_map)) => prev != now_map,
            // First sighting of a map is NOT an edge: qctrl may have started
            // mid-map, and we have no way to tell that apart from a real start.
            (None, Some(_)) => false,
            _ => false,
        };

        if let Some(map) = obs.map {
            self.current_map = Some(map.to_string());
        } else {
            // Server has no map (down / between maps). Nothing to time.
            self.current_map = None;
            self.invalidate();
            return;
        }

        if map_changed {
            self.map_start = Some(now);
            self.anchor = ClockAnchor::Exact;
            self.source = ClockSource::ObservedEdge;
            return;
        }

        if restarted {
            // Same map name, brand new server process: our anchor is stale and
            // there is no edge to re-anchor on. This is exactly the case that
            // would otherwise tick along being silently wrong.
            self.invalidate();
            return;
        }

        // Sanity: a map cannot have been running longer than the server has been
        // up. If it claims to be, the anchor is bogus — don't serve it.
        if let (ClockAnchor::Exact, Some(start), Some(up)) =
            (self.anchor, self.map_start, obs.uptime_seconds)
        {
            if (start.elapsed().as_secs()) > up + 2 {
                self.invalidate();
            }
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
            server_uptime_seconds: self.last_uptime,
            last_poll_age_seconds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn obs<'a>(map: &'a str, uptime: Option<u64>) -> Observation<'a> {
        Observation {
            map: Some(map),
            uptime_seconds: uptime,
        }
    }

    /// qctrl started while a map was already running. We did not see it start,
    /// so we do not know how long it has run — and we must say so.
    #[test]
    fn first_poll_mid_map_is_unknown() {
        let mut clock = ClockState::default();
        let now = Instant::now();
        clock.observe(obs("q2dm7", Some(500)), now);

        let snap = clock.snapshot(now, Some(now), Some(10));
        assert_eq!(snap.anchor, ClockAnchor::Unknown);
        assert_eq!(snap.elapsed_seconds, None);
        assert_eq!(snap.source, ClockSource::None);
    }

    #[test]
    fn observed_map_change_anchors_the_clock() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7", Some(500)), t0);
        clock.observe(obs("q2dm1", Some(510)), t0);

        let snap = clock.snapshot(t0, Some(t0), Some(10));
        assert_eq!(snap.anchor, ClockAnchor::Exact);
        assert_eq!(snap.elapsed_seconds, Some(0));
        assert_eq!(snap.source, ClockSource::ObservedEdge);
    }

    #[test]
    fn elapsed_counts_up_from_the_anchor() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7", Some(500)), t0);
        clock.observe(obs("q2dm1", Some(510)), t0);

        let later = t0 + Duration::from_secs(90);
        let snap = clock.snapshot(later, Some(later), Some(10));
        assert_eq!(snap.elapsed_seconds, Some(90));
    }

    /// The case sv_uptime exists to catch: the server restarts onto the SAME map,
    /// so there is no name edge. Without the uptime check the clock would keep
    /// counting from the old anchor and be silently, confidently wrong.
    #[test]
    fn server_restart_on_same_map_invalidates_the_anchor() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7", Some(500)), t0);
        clock.observe(obs("q2dm1", Some(510)), t0);
        assert_eq!(clock.anchor, ClockAnchor::Exact);

        // Uptime goes backwards: new server process, same map.
        clock.observe(obs("q2dm1", Some(3)), t0 + Duration::from_secs(60));

        let snap = clock.snapshot(t0 + Duration::from_secs(60), Some(t0), Some(10));
        assert_eq!(snap.anchor, ClockAnchor::Unknown);
        assert_eq!(snap.elapsed_seconds, None);
    }

    /// A restart that also lands on a new map still gets a valid anchor — the map
    /// really did just start, so the edge is genuine.
    #[test]
    fn server_restart_onto_a_new_map_still_anchors() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7", Some(500)), t0);
        clock.observe(obs("q2dm1", Some(2)), t0);

        assert_eq!(clock.anchor, ClockAnchor::Exact);
        assert_eq!(clock.source, ClockSource::ObservedEdge);
    }

    /// A map cannot have run longer than the server has been up. If our anchor
    /// says otherwise it is bogus, whatever produced it.
    #[test]
    fn anchor_older_than_the_server_is_rejected() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7", Some(500)), t0);
        clock.observe(obs("q2dm1", Some(510)), t0);

        // 10 minutes later, but the server claims it has only been up 30s.
        let later = t0 + Duration::from_secs(600);
        clock.observe(obs("q2dm1", Some(30)), later);

        assert_eq!(clock.anchor, ClockAnchor::Unknown);
    }

    #[test]
    fn own_map_command_anchors_a_same_map_restart() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7", Some(500)), t0);
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
        clock.observe(obs("q2dm7", Some(500)), t0);
        clock.observe(obs("q2dm1", Some(510)), t0);

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
        clock.observe(obs("q2dm7", Some(500)), t0);
        clock.observe(obs("q2dm1", Some(510)), t0);

        // timelimit 1 = 60s; 90s in with no map change means our model is wrong.
        let later = t0 + Duration::from_secs(90);
        let snap = clock.snapshot(later, Some(later), Some(1));
        assert_eq!(snap.quality, ClockQuality::Overdue);
    }

    #[test]
    fn a_few_seconds_over_the_limit_is_not_overdue() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7", Some(500)), t0);
        clock.observe(obs("q2dm1", Some(510)), t0);

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
        clock.observe(obs("q2dm7", Some(500)), t0);
        clock.observe(obs("q2dm1", Some(510)), t0);

        let later = t0 + Duration::from_secs(100_000);
        let snap = clock.snapshot(later, Some(later), Some(0));
        assert_eq!(snap.quality, ClockQuality::Live);
    }

    #[test]
    fn a_server_with_no_map_has_no_clock() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7", Some(500)), t0);
        clock.observe(obs("q2dm1", Some(510)), t0);
        assert_eq!(clock.anchor, ClockAnchor::Exact);

        clock.observe(
            Observation {
                map: None,
                uptime_seconds: None,
            },
            t0,
        );
        assert_eq!(clock.anchor, ClockAnchor::Unknown);
    }

    /// Without sv_uptime everything still works — we just lose restart detection.
    /// Degrading to a wrong-but-honest-looking clock is the documented tradeoff.
    #[test]
    fn works_without_uptime() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();
        clock.observe(obs("q2dm7", None), t0);
        clock.observe(obs("q2dm1", None), t0);

        let snap = clock.snapshot(t0, Some(t0), Some(10));
        assert_eq!(snap.anchor, ClockAnchor::Exact);
        assert_eq!(snap.server_uptime_seconds, None);
    }

    /// The core invariant, asserted directly: a number is served if and only if
    /// we observed the map start.
    #[test]
    fn elapsed_is_some_iff_anchor_is_exact() {
        let mut clock = ClockState::default();
        let t0 = Instant::now();

        for (map, uptime) in [("q2dm7", 500u64), ("q2dm1", 510), ("q2dm1", 5)] {
            clock.observe(obs(map, Some(uptime)), t0);
            let snap = clock.snapshot(t0, Some(t0), Some(10));
            assert_eq!(
                snap.elapsed_seconds.is_some(),
                snap.anchor == ClockAnchor::Exact,
                "elapsed must be Some iff anchor is Exact"
            );
        }
    }
}
