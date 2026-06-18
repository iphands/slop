//! Reactive stuck recovery — unified stuck detector, 7-direction fan-out trace,
//! and a `RecoveryAction` for the steering pipeline.
//!
//! Port of Eraser's `botRoamFindBestDirection` + `bot_move` stuck branch (distilled §3, §9).
//! Plan 13 T1–T3.

use glam::Vec3;
use world::{CollisionModel, HULL_MAXS, HULL_MINS, MASK_SOLID, MASK_WATER};

// ── StuckDetector (T1) ────────────────────────────────────────────────────────

/// Deadband: if the bot moves less than this (units) in `SAMPLE_EVERY_SECS`, it is
/// considered stuck. Original Eraser value was 4 u, but bots oscillating against
/// walls can move ~10 u/s (bouncing back and forth) and still make zero progress.
/// 16 u/s is still well below normal walking speed (~300 u/s) so this catches all
/// genuinely stuck bots without false positives.
const DEADBAND: f32 = 16.0;
/// How often to check for stall (seconds). Eraser checks every 1 s.
const SAMPLE_EVERY_SECS: f32 = 1.0;
/// After this many stuck seconds → `Mild` (jump once).
const JUMP_AFTER_SECS: f32 = 1.0;
/// After this many stuck seconds → `Hard` (force repath). Replaces Eraser's suicide.
const HARD_REPATH_SECS: f32 = 3.5;

/// How stuck the bot is based on origin measurements over time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StuckLevel {
    None,
    /// Stalled ~1 s — try jumping.
    Mild,
    /// Stalled ~5 s — force repath and clear blacklist.
    Hard,
}

/// Unified stuck detector. One instance per bot; replaces `nav.is_stuck()` and the
/// `stuck_frames` counter in `bot_task`. (Plan 13 T1)
#[derive(Debug, Clone)]
pub struct StuckDetector {
    /// Sampled position at the previous 1-second checkpoint.
    last_sample_pos: Option<Vec3>,
    /// Accumulated time toward the next sample.
    time_acc: f32,
    /// Total seconds the bot has been continuously stuck (reset on movement).
    stuck_secs: f32,
}

impl StuckDetector {
    pub fn new() -> Self {
        Self {
            last_sample_pos: None,
            time_acc: 0.0,
            stuck_secs: 0.0,
        }
    }

    /// Feed the current position and elapsed frame time. Returns the current stuck level.
    /// Call once per tick.
    pub fn update(&mut self, pos: Vec3, dt: f32) -> StuckLevel {
        self.time_acc += dt;
        if self.time_acc < SAMPLE_EVERY_SECS {
            return self.level();
        }
        self.time_acc -= SAMPLE_EVERY_SECS;

        let moved = self
            .last_sample_pos
            .map(|prev| (pos - prev).length())
            .unwrap_or(f32::MAX);

        if moved < DEADBAND {
            self.stuck_secs += SAMPLE_EVERY_SECS;
        } else {
            self.stuck_secs = 0.0;
        }
        self.last_sample_pos = Some(pos);

        self.level()
    }

    /// Reset all stuck state (call after successful recovery or on respawn).
    pub fn reset(&mut self) {
        self.stuck_secs = 0.0;
        self.time_acc = 0.0;
        self.last_sample_pos = None;
    }

    fn level(&self) -> StuckLevel {
        if self.stuck_secs >= HARD_REPATH_SECS {
            StuckLevel::Hard
        } else if self.stuck_secs >= JUMP_AFTER_SECS {
            StuckLevel::Mild
        } else {
            StuckLevel::None
        }
    }
}

impl Default for StuckDetector {
    fn default() -> Self {
        Self::new()
    }
}

// ── find_best_direction (T2) ──────────────────────────────────────────────────

/// Forward trace distance for the direction fan-out (Eraser: 256 u).
const TRACE_DIST: f32 = 256.0;
/// Trace-origin **lift** to clear ground clutter during the direction fan-out (not the
/// Q2 step-climb height). 24 u is borrowed from Eraser's own `STEPSIZE` constant
/// (`bot_nav.c:99`), which is Eraser's step budget, NOT Q2's `pmove.c` `STEPSIZE=18`.
/// The two are independent: this lifts the trace ray off the floor so it doesn't clip
/// into tiny surface irregularities; `world::navgraph::STEP=18` gates walkable edges.
const STEPSIZE: f32 = 24.0;
/// Fall fraction beyond which the endpoint is flagged as a ledge and the score is halved.
const LEDGE_FRAC: f32 = 0.4;
/// How far to probe downward when checking for a ledge below the trace endpoint.
const DOWN_DIST: f32 = 256.0;

/// Test 6 directions fanning out from `view_yaw` (±45°, ±90°, ±135°, 0° — skip ±180°).
/// Returns the `(yaw_degrees, score)` of the most open direction, or `None` if all blocked.
///
/// Port of Eraser `botRoamFindBestDirection` (`bot_nav.c:96-176`). (Plan 13 T2)
pub fn find_best_direction(cm: &CollisionModel, origin: Vec3, view_yaw: f32) -> Option<(f32, f32)> {
    // 6 angular offsets relative to view_yaw (skip ±180°).
    const OFFSETS_DEG: [f32; 6] = [0.0, 45.0, -45.0, 90.0, -90.0, 135.0];

    let lifted = [origin.x, origin.y, origin.z + STEPSIZE];
    let mut best: Option<(f32, f32)> = None;

    for &offset in &OFFSETS_DEG {
        let yaw = view_yaw + offset;
        let r = yaw.to_radians();
        let dir = [r.cos() * TRACE_DIST, r.sin() * TRACE_DIST, 0.0];
        let end = [lifted[0] + dir[0], lifted[1] + dir[1], lifted[2] + dir[2]];

        let t = cm.trace(&lifted, &end, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        if t.startsolid {
            continue;
        }

        let mut score = t.fraction * TRACE_DIST;

        // Down-probe from the endpoint: penalise long falls (ledge risk).
        let ep = t.endpos;
        let down_end = [ep[0], ep[1], ep[2] - DOWN_DIST];
        let down = cm.trace(
            &ep,
            &down_end,
            &[0.0; 3],
            &[0.0; 3],
            MASK_SOLID | MASK_WATER,
        );

        if down.fraction > LEDGE_FRAC {
            score *= 0.5; // punish ledge
        }

        // Skip if water/lava below the endpoint.
        let ep_contents = cm.point_contents(&down.endpos);
        if ep_contents & MASK_WATER != 0 {
            continue;
        }

        let is_better = best.map(|(_, s)| score > s).unwrap_or(true);
        if is_better {
            best = Some((yaw, score));
        }

        // Early-out: fully open in this direction.
        if score >= TRACE_DIST - 1.0 {
            break;
        }
    }

    best
}

// ── RecoveryAction / Recovery (T3) ───────────────────────────────────────────

/// What the recovery controller wants the bot to do this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RecoveryAction {
    /// No recovery needed.
    None,
    /// Mild stuck: jump once to clear a step or small ledge.
    Jump,
    /// Stuck + wall ahead: strafe sideways. `+1` = left, `-1` = right (sign flips every 3 s).
    Strafe { dir: f32 },
    /// Sustained stuck: reverse half-speed + repath.
    BackOffThenRepath,
    /// No nav node nearby — steer at the open yaw found by `find_best_direction`.
    UseHeading(f32),
}

impl RecoveryAction {
    /// True if any recovery action is active.
    pub fn is_active(&self) -> bool {
        !matches!(self, RecoveryAction::None)
    }

    /// Short label for the recorder (Plan 13 T4).
    pub fn label(&self) -> Option<&'static str> {
        match self {
            RecoveryAction::None => None,
            RecoveryAction::Jump => Some("jump"),
            RecoveryAction::Strafe { dir } => {
                if *dir > 0.0 {
                    Some("strafeL")
                } else {
                    Some("strafeR")
                }
            }
            RecoveryAction::BackOffThenRepath => Some("backoff"),
            RecoveryAction::UseHeading(_) => Some("heading"),
        }
    }
}

/// Per-bot recovery state. Owns the `StuckDetector` and the strafe-flip timer.
#[derive(Debug, Clone)]
pub struct Recovery {
    pub stuck: StuckDetector,
    /// Strafe direction (±1) for Eraser-style zig-zag; flips every 3 s.
    strafe_dir: f32,
    /// Time accumulator for the strafe flip.
    strafe_elapsed: f32,
}

impl Recovery {
    pub fn new() -> Self {
        Self {
            stuck: StuckDetector::new(),
            strafe_dir: 1.0,
            strafe_elapsed: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.stuck.reset();
    }

    /// Evaluate the recovery action for this tick.
    ///
    /// - `pos`: current bot position.
    /// - `dt`: elapsed seconds since last tick.
    /// - `cm`: collision model for wall/ledge probes (may be `None` before map load).
    /// - `view_yaw`: current view yaw (for wall probe + strafe axis).
    /// - `has_nav_target`: true when `pursue_target` returned a non-None value.
    /// - `engaging`: true when in `BehaviorState::Engage` (gate `BackOffThenRepath` while in combat).
    pub fn evaluate(
        &mut self,
        pos: Vec3,
        dt: f32,
        cm: Option<&CollisionModel>,
        view_yaw: f32,
        has_nav_target: bool,
        engaging: bool,
    ) -> RecoveryAction {
        // If no nav target, suggest a free-space heading (only when we have a CM).
        if !has_nav_target {
            if let Some(cm) = cm {
                if let Some((yaw, _)) = find_best_direction(cm, pos, view_yaw) {
                    return RecoveryAction::UseHeading(yaw);
                }
            }
        }

        let level = self.stuck.update(pos, dt);
        match level {
            StuckLevel::None => RecoveryAction::None,
            StuckLevel::Mild => {
                // Check if there's a wall directly ahead — if so, strafe; otherwise jump.
                let has_wall_ahead = cm.map(|cm| wall_ahead(cm, pos, view_yaw)).unwrap_or(false);
                if has_wall_ahead {
                    self.tick_strafe(dt);
                    RecoveryAction::Strafe {
                        dir: self.strafe_dir,
                    }
                } else {
                    RecoveryAction::Jump
                }
            }
            StuckLevel::Hard => {
                // BackOffThenRepath only when not actively fighting (to avoid abandoning combat).
                if !engaging {
                    self.stuck.reset();
                    RecoveryAction::BackOffThenRepath
                } else {
                    // In combat: settle for a strafe rather than repath.
                    self.tick_strafe(dt);
                    RecoveryAction::Strafe {
                        dir: self.strafe_dir,
                    }
                }
            }
        }
    }

    fn tick_strafe(&mut self, dt: f32) {
        const STRAFE_FLIP_SECS: f32 = 3.0;
        self.strafe_elapsed += dt;
        if self.strafe_elapsed >= STRAFE_FLIP_SECS {
            self.strafe_dir *= -1.0;
            self.strafe_elapsed = 0.0;
        }
    }
}

impl Default for Recovery {
    fn default() -> Self {
        Self::new()
    }
}

/// True if there is solid geometry directly ahead within 32 u (wall contact heuristic).
fn wall_ahead(cm: &CollisionModel, origin: Vec3, view_yaw: f32) -> bool {
    const PROBE: f32 = 32.0;
    let r = view_yaw.to_radians();
    let o = [origin.x, origin.y, origin.z];
    let e = [
        origin.x + r.cos() * PROBE,
        origin.y + r.sin() * PROBE,
        origin.z,
    ];
    let t = cm.trace(&o, &e, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
    t.fraction < 0.5 // less than 16u clear → treat as wall
}
