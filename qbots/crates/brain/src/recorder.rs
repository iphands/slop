//! Per-frame movement recorder — the measurement lens for Plans 11–14.
//!
//! A scenario (`spawn-to-spawn` / `spawn-to-weapon`, Plan 10) feeds one [`Sample`]
//! per server frame; the recorder derives movement-quality signals from it and
//! accumulates a structured log + a [`RunSummary`]. It only *observes* the same
//! `clc_move` path the live bot drives — it never sets velocity or teleports, so a
//! log showing sustained > ~320 u/s grounded speed is a physics bug to flag, not a
//! feature.
//!
//! Detectors reuse the existing [`world::CollisionModel::trace`] via the
//! [`WallProbe`] trait (production = [`CmWallProbe`]; tests stub it), so no new
//! physics is introduced.
//!
//! # Log schema (`dump()`)
//!
//! Written to `./logs/<scenario>/<unix_ts>.<bot>.log`, one token per column,
//! single-space separated and fully positional (so `awk '{print $4,$5,$6}'` etc.
//! works). `#`-prefixed lines are metadata; the rest are frame rows:
//!
//! ```text
//! # qbots movement log  scenario=<s>  bot=<name>  map=<map>  goal=(x,y,z)
//! # goal_classname=<cls>  started=<ISO8601>
//! # t frame x y z vx vy vz speed yaw pitch move_yaw face_delta wp wpd flags
//! <t> <frame> <x> <y> <z> <vx> <vy> <vz> <speed> <yaw> <pitch> <move_yaw> <face_delta> <wp> <wpd> <flags>
//! ...
//! # SUMMARY reached=<0|1> elapsed=<s> distance=<u> mean_speed=<u/s> max_speed=<u/s> bumps=<n> wrong_turns=<n> hindered_frames=<n> phantom_frames=<n> path_efficiency=<0..1>
//! ```
//!
//! Columns: `t` elapsed seconds; `frame` serverframe; `x y z` origin;
//! `vx vy vz` velocity (u/s); `speed` `|horizontal velocity|`; `yaw pitch` view
//! angles (deg); `move_yaw` velocity-heading yaw (`nan` when ~still);
//! `face_delta` `|yaw − move_yaw|` (0 when still); `wp` current waypoint index
//! (`-` if none); `wpd` 3D distance to it (`-`); `flags` a char run — `B` wall
//! bump, `W` wrong turn, `H` hindered, `A` airborne, `P` phantom-target (combat with
//! no LOS), `R` recovery-active (Plan 13), `S` swimming (waterlevel ≥ 2, Plan 40), `.` none.
//! The `SUMMARY` line is the headline: it is what Plans 11–14 must beat.

use std::path::Path;
use std::sync::Arc;

use world::{CollisionModel, HULL_MAXS, HULL_MINS, MASK_SOLID};

/// How far ahead of the bot the wall-bump detector probes (player-hull box).
pub const BUMP_PROBE: f32 = 48.0;
/// Minimum gap between two logged bumps so a sustained grind logs as a few
/// events, not one per frame.
pub const BUMP_COOLDOWN: f32 = 0.4;
/// Below this grounded horizontal speed (while intending to move) the bot is
/// "hindered" — making no progress against geometry.
pub const HINDER_SPEED: f32 = 100.0;
/// A frame counts as "intending to move" only when the commanded forward intent
/// exceeds this (the hindered + wall-bump gates).
pub const HINDER_INTENT: f32 = 0.5;
/// Within this 3D distance of the scenario goal the bot has "reached" it.
pub const GOAL_TOL: f32 = 48.0;
/// Movement below this (u/frame) is "still" → `move_yaw` is NaN.
const STILL_SPEED: f32 = 5.0;
/// A wrong-turn requires this much frame-to-frame displacement (filters jitter).
const WRONG_TURN_MOVE: f32 = 5.0;

/// A recorded wall collision: where the hull stopped, the surface normal, and how
/// far it got before impact.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WallBump {
    pub endpos: [f32; 3],
    pub normal: [f32; 3],
    pub dist: f32,
}

/// One sampled frame of movement telemetry with all derived quality signals.
#[derive(Debug, Clone)]
pub struct FrameRecord {
    pub t_secs: f32,
    pub frame: i32,
    pub origin: [f32; 3],
    pub velocity: [f32; 3],
    /// `|horizontal velocity|` (u/s).
    pub speed: f32,
    pub view_yaw: f32,
    pub view_pitch: f32,
    /// Yaw of the velocity heading; `NaN` when ~still.
    pub move_yaw: f32,
    /// `|view_yaw − move_yaw|` wrapped to ±180° (`0` when ~still).
    pub facing_move_delta_deg: f32,
    pub waypoint: Option<usize>,
    pub waypoint_dist: Option<f32>,
    pub goal_reached: bool,
    /// `Some` on a frame a wall-bump was logged (throttled), else `None`.
    pub wall_bump: Option<WallBump>,
    pub wrong_turn: bool,
    pub hindered: bool,
    pub grounded: bool,
    /// True when the bot had a combat target / was firing with no confirmed LOS
    /// (Plan 11 phantom-chase marker; always `false` in scenario mode).
    pub phantom_target: bool,
    /// True when the recovery controller issued a non-None action this tick (Plan 13 T4).
    pub recovery: bool,
    /// True when the bot is waist-deep or deeper in water (`waterlevel >= 2`, Plan 40).
    pub swimming: bool,
}

/// Raw inputs the tick gathers for one frame; the recorder derives everything in
/// [`FrameRecord`] from this + its own rolling state.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub t_secs: f32,
    pub frame: i32,
    pub origin: [f32; 3],
    pub velocity: [f32; 3],
    pub view_yaw: f32,
    pub view_pitch: f32,
    pub grounded: bool,
    /// Index of the nav node currently being pursued (if any).
    pub waypoint: Option<usize>,
    /// World position of that node (for wrong-turn / distance detection).
    pub waypoint_pos: Option<[f32; 3]>,
    /// Forward intent commanded this tick ([-1,1]); drives the hindered/bump gates.
    pub intent_forward: f32,
    /// True when the bot has a combat target or `should_fire` but no confirmed LOS
    /// (Plan 11 T4). Marks "phantom chasing" through walls. Always `false` in
    /// scenario mode (no combat); meaningful only in live bot ticks.
    pub phantom_target: bool,
    /// True when the recovery controller issued a non-None action this tick (Plan 13 T4).
    pub recovery: bool,
    /// True when the bot is waist-deep or deeper in water (`waterlevel >= 2`, Plan 40).
    pub swimming: bool,
}

/// Geometry probe for the wall-bump detector. Production wraps
/// [`CollisionModel`]; tests supply a stub so the detectors are unit-testable
/// without building BSP geometry.
pub trait WallProbe: Send + Sync {
    /// Sweep a player hull from `start` along the horizontal `dir` for `dist`.
    /// Returns the bump if it struck a near-vertical surface short of `dist`.
    fn trace_forward(&self, start: [f32; 3], dir: [f32; 3], dist: f32) -> Option<WallBump>;
}

/// [`WallProbe`] over a real [`CollisionModel`] — the production path.
pub struct CmWallProbe {
    cm: Arc<CollisionModel>,
}

impl CmWallProbe {
    pub fn new(cm: Arc<CollisionModel>) -> Self {
        Self { cm }
    }
}

impl WallProbe for CmWallProbe {
    fn trace_forward(&self, start: [f32; 3], dir: [f32; 3], dist: f32) -> Option<WallBump> {
        let end = [
            start[0] + dir[0] * dist,
            start[1] + dir[1] * dist,
            start[2] + dir[2] * dist,
        ];
        let t = self
            .cm
            .trace(&start, &end, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        // A wall = stopped short of full distance, not embedded, with a
        // near-vertical face (floors/steps have |normal.z| ≈ 1).
        if t.fraction < 1.0 && !t.startsolid && t.plane.normal[2].abs() < 0.3 {
            Some(WallBump {
                endpos: t.endpos,
                normal: t.plane.normal,
                dist: t.fraction * dist,
            })
        } else {
            None
        }
    }
}

/// End-of-run movement-quality numbers — what Plans 11–14 must beat.
#[derive(Debug, Clone, PartialEq)]
pub struct RunSummary {
    pub reached: bool,
    pub elapsed_secs: f32,
    /// Cumulative `|Δorigin|` over sampled frames (3D).
    pub distance: f32,
    /// `distance / elapsed` — average speed over the run.
    pub mean_speed: f32,
    /// Max instantaneous horizontal speed across frames.
    pub max_speed: f32,
    pub bumps: u32,
    pub wrong_turns: u32,
    pub hindered_frames: u32,
    /// Frames with `phantom_target=true` (combat/fire with no LOS). Should be ~0
    /// after Plan 11; always 0 in scenario mode (no combat).
    pub phantom_frames: u32,
    /// `straight_line_dist / distance_traveled` (Plan 14 T4). Closer to 1.0 means
    /// less grid zigzag. `0.0` when no frames or the bot didn't move.
    pub path_efficiency: f32,
}

/// The recorder: owns the probe, the goal, and the accumulating frame log.
pub struct MovementRecorder {
    probe: Arc<dyn WallProbe>,
    goal: [f32; 3],
    goal_label: String,
    scenario: String,
    bot: String,
    map: String,
    started_iso: String,
    frames: Vec<FrameRecord>,
    prev_origin: Option<[f32; 3]>,
    start_origin: Option<[f32; 3]>,
    distance: f32,
    max_speed: f32,
    bumps: u32,
    wrong_turns: u32,
    hindered_frames: u32,
    last_bump_t: f32,
    ever_reached: bool,
    phantom_frames: u32,
}

impl MovementRecorder {
    /// New recorder for a scenario driving toward `goal` (label e.g.
    /// `weapon_rocketlauncher`). `started_iso` is an ISO-8601 timestamp supplied
    /// by the caller (the recorder has no clock of its own → stays unit-testable).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        probe: Arc<dyn WallProbe>,
        goal: [f32; 3],
        goal_label: impl Into<String>,
        scenario: impl Into<String>,
        bot: impl Into<String>,
        map: impl Into<String>,
        started_iso: impl Into<String>,
    ) -> Self {
        Self {
            probe,
            goal,
            goal_label: goal_label.into(),
            scenario: scenario.into(),
            bot: bot.into(),
            map: map.into(),
            started_iso: started_iso.into(),
            frames: Vec::new(),
            prev_origin: None,
            start_origin: None,
            distance: 0.0,
            max_speed: 0.0,
            bumps: 0,
            wrong_turns: 0,
            hindered_frames: 0,
            last_bump_t: f32::NEG_INFINITY,
            ever_reached: false,
            phantom_frames: 0,
        }
    }

    /// Record one frame. Runs all detectors, accumulates counters + the log.
    pub fn sample(&mut self, s: Sample) {
        let horiz_speed = (s.velocity[0] * s.velocity[0] + s.velocity[1] * s.velocity[1]).sqrt();

        // Velocity heading yaw (NaN when ~still); facing error vs the view yaw.
        let move_yaw = if horiz_speed >= STILL_SPEED {
            s.velocity[1].atan2(s.velocity[0]).to_degrees()
        } else {
            f32::NAN
        };
        let facing_delta = if horiz_speed >= STILL_SPEED {
            angle_delta_deg(s.view_yaw, move_yaw).abs()
        } else {
            0.0
        };

        // Distance traveled this frame (3D); record start on first frame.
        if let Some(prev) = self.prev_origin {
            self.distance += dist3(s.origin, prev);
        } else {
            self.start_origin = Some(s.origin);
        }
        if horiz_speed > self.max_speed {
            self.max_speed = horiz_speed;
        }

        let goal_reached = dist3(s.origin, self.goal) < GOAL_TOL;
        if goal_reached {
            self.ever_reached = true;
        }

        let intending = s.intent_forward.abs() > HINDER_INTENT;

        // Wall-bump: only when intending to move, throttled by cooldown.
        let wall_bump = if intending && s.t_secs - self.last_bump_t >= BUMP_COOLDOWN {
            let yaw_rad = s.view_yaw.to_radians();
            let dir = [yaw_rad.cos(), yaw_rad.sin(), 0.0];
            match self.probe.trace_forward(s.origin, dir, BUMP_PROBE) {
                Some(b) => {
                    self.last_bump_t = s.t_secs;
                    self.bumps += 1;
                    Some(b)
                }
                None => None,
            }
        } else {
            None
        };

        // Wrong-turn: did we get farther from the current waypoint this frame?
        let wrong_turn = match (self.prev_origin, s.waypoint_pos) {
            (Some(prev), Some(wp)) => {
                let moved = dist3(s.origin, prev);
                moved > WRONG_TURN_MOVE && dist3(s.origin, wp) > dist3(prev, wp)
            }
            _ => false,
        };
        if wrong_turn {
            self.wrong_turns += 1;
        }

        let hindered = s.grounded && intending && horiz_speed < HINDER_SPEED;
        if hindered {
            self.hindered_frames += 1;
        }

        if s.phantom_target {
            self.phantom_frames += 1;
        }

        let waypoint_dist = s.waypoint_pos.map(|wp| dist3(s.origin, wp));

        self.frames.push(FrameRecord {
            t_secs: s.t_secs,
            frame: s.frame,
            origin: s.origin,
            velocity: s.velocity,
            speed: horiz_speed,
            view_yaw: s.view_yaw,
            view_pitch: s.view_pitch,
            move_yaw,
            facing_move_delta_deg: facing_delta,
            waypoint: s.waypoint,
            waypoint_dist,
            goal_reached,
            wall_bump,
            wrong_turn,
            hindered,
            grounded: s.grounded,
            phantom_target: s.phantom_target,
            recovery: s.recovery,
            swimming: s.swimming,
        });

        self.prev_origin = Some(s.origin);
    }

    /// Number of sampled frames so far.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// True when no frames have been sampled.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Aggregate movement-quality numbers for the run.
    pub fn summary(&self) -> RunSummary {
        let elapsed = self.frames.last().map(|f| f.t_secs).unwrap_or(0.0);
        let mean_speed = if elapsed > 1e-6 {
            self.distance / elapsed
        } else {
            0.0
        };
        let path_efficiency = if self.distance > 1e-3 {
            let straight = self
                .start_origin
                .map(|s| dist3(s, self.goal))
                .unwrap_or(0.0);
            (straight / self.distance).min(1.0)
        } else {
            0.0
        };
        RunSummary {
            reached: self.ever_reached,
            elapsed_secs: elapsed,
            distance: self.distance,
            mean_speed,
            max_speed: self.max_speed,
            bumps: self.bumps,
            wrong_turns: self.wrong_turns,
            hindered_frames: self.hindered_frames,
            phantom_frames: self.phantom_frames,
            path_efficiency,
        }
    }

    /// Write the structured per-frame log to `path` (creates parent dirs; on IO
    /// failure logs a warning instead of panicking). Format is finalized/​documented
    /// in Plan 10 T3; this writes header + one row per frame + a SUMMARY line.
    pub fn dump(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    tracing::warn!("recorder: can't create {}: {e}", parent.display());
                }
            }
        }
        let mut out = String::new();
        out.push_str(&format!(
            "# qbots movement log  scenario={}  bot={}  map={}  goal=({:.0},{:.0},{:.0})\n",
            self.scenario, self.bot, self.map, self.goal[0], self.goal[1], self.goal[2]
        ));
        out.push_str(&format!(
            "# goal_classname={}  started={}\n",
            self.goal_label, self.started_iso
        ));
        out.push_str("# t frame x y z vx vy vz speed yaw pitch move_yaw face_delta wp wpd flags\n");
        for f in &self.frames {
            out.push_str(&format!(
                "{:.3} {} {:.0} {:.0} {:.0} {:.0} {:.0} {:.0} {:.0} {:.0} {:.0} {} {:.0} {} {} {}\n",
                f.t_secs,
                f.frame,
                f.origin[0],
                f.origin[1],
                f.origin[2],
                f.velocity[0],
                f.velocity[1],
                f.velocity[2],
                f.speed,
                f.view_yaw,
                f.view_pitch,
                fmt_opt_f32(f.move_yaw, "nan"),
                f.facing_move_delta_deg,
                fmt_opt_usize(f.waypoint),
                fmt_opt_dist(f.waypoint_dist),
                flags(f),
            ));
        }
        let s = self.summary();
        out.push_str(&format!(
            "# SUMMARY reached={} elapsed={:.2} distance={:.0} mean_speed={:.0} max_speed={:.0} bumps={} wrong_turns={} hindered_frames={} phantom_frames={} path_efficiency={:.3}\n",
            s.reached as u8, s.elapsed_secs, s.distance, s.mean_speed, s.max_speed, s.bumps,
            s.wrong_turns, s.hindered_frames, s.phantom_frames, s.path_efficiency
        ));
        std::fs::write(path, out)
    }
}

/// Per-frame flag string: `B`=wall_bump, `W`=wrong_turn, `H`=hindered,
/// `A`=airborne, `P`=phantom_target, `R`=recovery_active, `S`=swimming, `.`=none.
fn flags(f: &FrameRecord) -> String {
    let mut s = String::new();
    if f.wall_bump.is_some() {
        s.push('B');
    }
    if f.wrong_turn {
        s.push('W');
    }
    if f.hindered {
        s.push('H');
    }
    if !f.grounded {
        s.push('A');
    }
    if f.phantom_target {
        s.push('P');
    }
    if f.recovery {
        s.push('R');
    }
    if f.swimming {
        s.push('S');
    }
    if s.is_empty() {
        s.push('.');
    }
    s
}

fn fmt_opt_f32(v: f32, nan_text: &str) -> String {
    if v.is_nan() {
        nan_text.to_string()
    } else {
        format!("{:.0}", v)
    }
}

fn fmt_opt_usize(v: Option<usize>) -> String {
    match v {
        Some(n) => n.to_string(),
        None => "-".to_string(),
    }
}

fn fmt_opt_dist(v: Option<f32>) -> String {
    match v {
        Some(d) => format!("{:.0}", d),
        None => "-".to_string(),
    }
}

fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// Signed shortest-arc difference `target − current`, wrapped to ±180°.
fn angle_delta_deg(current: f32, target: f32) -> f32 {
    (target - current + 540.0).rem_euclid(360.0) - 180.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A probe that always reports a clear path (no walls). Use for the
    /// straight-line-to-goal scenario.
    struct ClearProbe;
    impl WallProbe for ClearProbe {
        fn trace_forward(&self, _start: [f32; 3], _dir: [f32; 3], _dist: f32) -> Option<WallBump> {
            None
        }
    }

    /// A probe that reports a wall directly ahead of a fixed `block_x` plane for
    /// any forward (yaw≈0) trace past it.
    struct WallAheadProbe {
        block_x: f32,
    }
    impl WallProbe for WallAheadProbe {
        fn trace_forward(&self, start: [f32; 3], dir: [f32; 3], dist: f32) -> Option<WallBump> {
            // Only "wall" when moving roughly +x (the dir we walk into it).
            if dir[0] > 0.5 && start[0] < self.block_x && start[0] + dir[0] * dist > self.block_x {
                let traveled = self.block_x - start[0];
                Some(WallBump {
                    endpos: [self.block_x, start[1], start[2]],
                    normal: [-1.0, 0.0, 0.0],
                    dist: traveled,
                })
            } else {
                None
            }
        }
    }

    fn rec(probe: Arc<dyn WallProbe>, goal: [f32; 3]) -> MovementRecorder {
        MovementRecorder::new(
            probe,
            goal,
            "goal",
            "test",
            "qb0",
            "q2dm1",
            "1970-01-01T00:00:00Z",
        )
    }

    /// Move straight along +x toward a goal at 300 u/s, no wall → clean run.
    #[test]
    fn straight_line_to_goal_is_clean() {
        let probe: Arc<dyn WallProbe> = Arc::new(ClearProbe);
        let goal = [1000.0, 0.0, 0.0];
        let mut r = rec(probe, goal);
        // 300 u/s along +x, 0.1 s steps, facing yaw 0.
        let steps = 40;
        for i in 0..steps {
            let t = i as f32 * 0.1;
            let x = i as f32 * 30.0; // 300 u/s * 0.1s = 30 u/step
            r.sample(Sample {
                t_secs: t,
                frame: 1000 + i,
                origin: [x, 0.0, 0.0],
                velocity: [300.0, 0.0, 0.0],
                view_yaw: 0.0,
                view_pitch: 0.0,
                grounded: true,
                waypoint: None,
                waypoint_pos: None,
                intent_forward: 1.0,
                phantom_target: false,
                recovery: false,
                swimming: false,
            });
        }
        let s = r.summary();
        assert!(s.reached, "should reach the goal within GOAL_TOL");
        assert_eq!(s.bumps, 0);
        assert_eq!(s.wrong_turns, 0);
        assert_eq!(s.hindered_frames, 0);
        assert!(
            (s.mean_speed - 300.0).abs() < 10.0,
            "mean_speed ~300, got {}",
            s.mean_speed
        );
        assert!(s.max_speed >= 299.0);
    }

    /// Walking into a wall (probe blocks +x) while intending forward → bumps
    /// logged and the stalled frames count as hindered.
    #[test]
    fn walking_into_wall_logs_bumps_and_hinder() {
        let probe: Arc<dyn WallProbe> = Arc::new(WallAheadProbe { block_x: 100.0 });
        let goal = [1000.0, 0.0, 0.0];
        let mut r = rec(probe, goal);
        // Bot creeps forward at 50 u/s (below HINDER_SPEED) into the wall at x=100.
        for i in 0..30 {
            let t = i as f32 * 0.1;
            let x = (i as f32 * 5.0).min(99.0); // stalls just shy of the wall
            r.sample(Sample {
                t_secs: t,
                frame: 1000 + i,
                origin: [x, 0.0, 0.0],
                velocity: [50.0, 0.0, 0.0],
                view_yaw: 0.0,
                view_pitch: 0.0,
                grounded: true,
                waypoint: None,
                waypoint_pos: None,
                intent_forward: 1.0,
                phantom_target: false,
                recovery: false,
                swimming: false,
            });
        }
        let s = r.summary();
        assert!(
            s.bumps >= 1,
            "should log at least one bump, got {}",
            s.bumps
        );
        assert!(
            s.hindered_frames >= 1,
            "stalled frames should be hindered, got {}",
            s.hindered_frames
        );
        assert!(!s.reached, "never reaches the goal behind the wall");
    }

    /// Bump throttle: a sustained grind logs a few bumps, not one per frame.
    #[test]
    fn bump_cooldown_throttles_sustained_grind() {
        let probe: Arc<dyn WallProbe> = Arc::new(WallAheadProbe { block_x: 100.0 });
        let goal = [1000.0, 0.0, 0.0];
        let mut r = rec(probe, goal);
        // 5 s grinding into the wall at 0.1 s cadence = 50 frames.
        for i in 0..50 {
            r.sample(Sample {
                t_secs: i as f32 * 0.1,
                frame: i,
                origin: [99.0, 0.0, 0.0],
                velocity: [50.0, 0.0, 0.0],
                view_yaw: 0.0,
                view_pitch: 0.0,
                grounded: true,
                waypoint: None,
                waypoint_pos: None,
                intent_forward: 1.0,
                phantom_target: false,
                recovery: false,
                swimming: false,
            });
        }
        let s = r.summary();
        // 5 s / 0.4 s cooldown ≈ 13 bumps max; far fewer than 50 frames.
        assert!(s.bumps <= 14, "throttled: <=~13 bumps, got {}", s.bumps);
        assert!(s.bumps >= 10, "but still periodic, got {}", s.bumps);
    }

    /// Wrong-turn: moving away from the waypoint while displaced counts.
    #[test]
    fn moving_away_from_waypoint_is_wrong_turn() {
        let probe: Arc<dyn WallProbe> = Arc::new(ClearProbe);
        let goal = [1000.0, 0.0, 0.0];
        let mut r = rec(probe, goal);
        let wp = [200.0, 0.0, 0.0];
        // Frame 0: at x=100. Frame 1: retreat to x=80 (away from wp at 200).
        r.sample(Sample {
            t_secs: 0.0,
            frame: 0,
            origin: [100.0, 0.0, 0.0],
            velocity: [0.0, 0.0, 0.0],
            view_yaw: 0.0,
            view_pitch: 0.0,
            grounded: true,
            waypoint: Some(5),
            waypoint_pos: Some(wp),
            intent_forward: 1.0,
            phantom_target: false,
            recovery: false,
            swimming: false,
        });
        r.sample(Sample {
            t_secs: 0.1,
            frame: 1,
            origin: [80.0, 0.0, 0.0],
            velocity: [-200.0, 0.0, 0.0],
            view_yaw: 0.0,
            view_pitch: 0.0,
            grounded: true,
            waypoint: Some(5),
            waypoint_pos: Some(wp),
            intent_forward: 1.0,
            phantom_target: false,
            recovery: false,
            swimming: false,
        });
        let s = r.summary();
        assert_eq!(
            s.wrong_turns, 1,
            "retreating from the waypoint is a wrong turn"
        );
    }

    /// Distance is monotonic non-decreasing across samples.
    #[test]
    fn distance_is_monotonic() {
        let probe: Arc<dyn WallProbe> = Arc::new(ClearProbe);
        let mut r = rec(probe, [1000.0, 0.0, 0.0]);
        let mut prev = -1.0;
        for i in 0..10 {
            r.sample(Sample {
                t_secs: i as f32 * 0.1,
                frame: i,
                origin: [i as f32 * 10.0, 0.0, 0.0],
                velocity: [100.0, 0.0, 0.0],
                view_yaw: 0.0,
                view_pitch: 0.0,
                grounded: true,
                waypoint: None,
                waypoint_pos: None,
                intent_forward: 1.0,
                phantom_target: false,
                recovery: false,
                swimming: false,
            });
            let d = r.summary().distance;
            assert!(d >= prev, "distance must not decrease");
            prev = d;
        }
    }

    /// `dump()` writes a file whose SUMMARY line parses with the documented regex.
    #[test]
    fn dump_writes_parseable_summary() {
        let probe: Arc<dyn WallProbe> = Arc::new(ClearProbe);
        let mut r = rec(probe, [1000.0, 0.0, 0.0]);
        r.sample(Sample {
            t_secs: 0.1,
            frame: 1,
            origin: [30.0, 0.0, 0.0],
            velocity: [300.0, 0.0, 0.0],
            view_yaw: 0.0,
            view_pitch: 0.0,
            grounded: true,
            waypoint: Some(0),
            waypoint_pos: Some([1000.0, 0.0, 0.0]),
            intent_forward: 1.0,
            phantom_target: false,
            recovery: false,
            swimming: false,
        });
        let dir = std::env::temp_dir().join(format!("qbots-rec-test-{}", std::process::id()));
        let path = dir.join("run.qb0.log");
        r.dump(&path).expect("dump ok");
        let text = std::fs::read_to_string(&path).expect("read back");
        let summary = text
            .lines()
            .find(|l| l.starts_with("# SUMMARY"))
            .expect("a SUMMARY line exists");
        // Documented schema: reached=(\d) elapsed=([\d.]+) ...
        let reached = regex_capture(summary, "reached=");
        let elapsed = regex_capture(summary, "elapsed=");
        assert_eq!(reached.as_deref(), Some("0")); // not reached yet
        assert!(elapsed.is_some(), "elapsed parses");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The full documented schema round-trips: both `#` header lines are present,
    /// the column header names every positional field, a frame row has the right
    /// token count, and the SUMMARY line carries every headline metric.
    #[test]
    fn dump_matches_documented_schema() {
        let probe: Arc<dyn WallProbe> = Arc::new(ClearProbe);
        let mut r = rec(probe, [1000.0, 0.0, 0.0]);
        r.sample(Sample {
            t_secs: 0.1,
            frame: 1,
            origin: [30.0, 0.0, 0.0],
            velocity: [300.0, 0.0, 0.0],
            view_yaw: 0.0,
            view_pitch: 0.0,
            grounded: true,
            waypoint: Some(0),
            waypoint_pos: Some([1000.0, 0.0, 0.0]),
            intent_forward: 1.0,
            phantom_target: false,
            recovery: false,
            swimming: false,
        });
        let dir = std::env::temp_dir().join(format!("qbots-schema-{}", std::process::id()));
        let path = dir.join("run.qb0.log");
        r.dump(&path).expect("dump ok");
        let text = std::fs::read_to_string(&path).expect("read back");
        let lines: Vec<&str> = text.lines().collect();

        // Two metadata header lines, then the column header.
        assert!(lines[0].starts_with("# qbots movement log  scenario="));
        assert!(lines[1].starts_with("# goal_classname="));
        let cols = lines[2]
            .trim_start_matches("# ")
            .split_whitespace()
            .collect::<Vec<_>>();
        assert_eq!(
            cols,
            vec![
                "t",
                "frame",
                "x",
                "y",
                "z",
                "vx",
                "vy",
                "vz",
                "speed",
                "yaw",
                "pitch",
                "move_yaw",
                "face_delta",
                "wp",
                "wpd",
                "flags"
            ],
            "column header names every positional field"
        );

        // The one frame row has exactly that many tokens (16 columns).
        let row = lines[3].split_whitespace().collect::<Vec<_>>();
        assert_eq!(row.len(), 16, "frame row has 16 positional columns");

        // SUMMARY line carries every headline metric.
        let summary = lines.last().expect("a SUMMARY line exists");
        assert!(summary.starts_with("# SUMMARY"));
        for key in [
            "reached=",
            "elapsed=",
            "distance=",
            "mean_speed=",
            "max_speed=",
            "bumps=",
            "wrong_turns=",
            "hindered_frames=",
            "phantom_frames=",
            "path_efficiency=",
        ] {
            assert!(summary.contains(key), "SUMMARY has {key}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn angle_delta_wraps_shortest_arc() {
        assert!((angle_delta_deg(0.0, 10.0) - 10.0).abs() < 1e-3);
        assert!(
            (angle_delta_deg(350.0, 10.0) - 20.0).abs() < 1e-3,
            "+20 not -340"
        );
        assert!((angle_delta_deg(10.0, 350.0) + 20.0).abs() < 1e-3, "-20");
    }

    /// Tiny hand-rolled "regex" — pull the token after `prefix=` up to the next
    /// space. Avoids pulling the `regex` crate into the brain dev-deps for one test.
    fn regex_capture(line: &str, prefix: &str) -> Option<String> {
        let needle = prefix.to_string();
        let idx = line.find(&needle)?;
        let rest = &line[idx + needle.len()..];
        let val: String = rest.chars().take_while(|c| !c.is_whitespace()).collect();
        if val.is_empty() {
            None
        } else {
            Some(val)
        }
    }
}
