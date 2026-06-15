//! Steering controller — turn-rate limiting, move-vector decomposition, arrive, circle-strafe.
//!
//! Ported from Eraser's `bot_ChangeYaw` / `M_ChangeYaw` and adapted for an external client
//! that controls motion via view-relative `(forwardmove, sidemove)` rather than server-side
//! velocity. See `context/distilled.md §Eraser` and `Plan 12`.

use glam::Vec3;

/// Maximum turn rate at combat skill 0 (deg/s). 720°/s → 180° in 0.25 s.
pub const YAW_SPEED_BASE: f32 = 720.0;
/// Additional deg/s per combat-skill level above 0 (range 0–4 → +0..+480).
const YAW_SPEED_PER_LEVEL: f32 = 120.0;
/// Scale forward down inside this distance of the final goal to avoid overshoot-orbit.
pub const ARRIVE_RADIUS: f32 = 80.0;
/// Minimum arrive throttle so the bot doesn't stop short.
const ARRIVE_MIN: f32 = 0.25;
/// Strafe direction flips every this many seconds (Eraser `strafe_changedir_time`).
pub const STRAFE_PERIOD_SECS: f32 = 3.0;

/// Per-bot steering state. One instance per bot; created next to `MovementController`.
#[derive(Debug, Clone)]
pub struct Steering {
    /// Last commanded view yaw in degrees — the integrator stepped by `change_yaw`.
    view_yaw: f32,
    /// Max turn rate in deg/s, skill-scaled via `combat_skill`.
    yaw_speed_dps: f32,
    /// Circle-strafe direction (+1 = left-strafe, −1 = right-strafe).
    strafe_dir: f32,
    /// Accumulated time in the current strafe direction (seconds).
    strafe_elapsed: f32,
}

impl Steering {
    /// Create a new controller. `combat_skill_f` is the Eraser combat rating ∈ [1.0, 5.0].
    /// `YAW_SPEED_BASE + (combat_skill_f − 1) × YAW_SPEED_PER_LEVEL`
    /// → combat 1 = 720°/s, combat 5 = 1200°/s.
    pub fn new(combat_skill_f: f32) -> Self {
        let level = (combat_skill_f - 1.0).max(0.0);
        let yaw_speed_dps = YAW_SPEED_BASE + level * YAW_SPEED_PER_LEVEL;
        Self {
            view_yaw: 0.0,
            yaw_speed_dps,
            strafe_dir: 1.0,
            strafe_elapsed: 0.0,
        }
    }

    /// Current view yaw (degrees).
    pub fn view_yaw(&self) -> f32 {
        self.view_yaw
    }

    /// Force-set the view yaw without turning (e.g., on spawn/reset).
    pub fn set_view_yaw(&mut self, yaw: f32) {
        self.view_yaw = yaw;
    }

    /// Step `view_yaw` toward `ideal_yaw` by at most `yaw_speed_dps * dt` using the
    /// shortest arc. Returns the new view yaw.
    ///
    /// Port of Eraser `M_ChangeYaw` / `bot_ChangeYaw`:
    ///   `diff = ((ideal - current + 540) % 360) - 180`; `step = clamp(diff, ±max_step)`.
    pub fn change_yaw(&mut self, ideal_yaw: f32, dt: f32) -> f32 {
        let max_step = self.yaw_speed_dps * dt;
        let diff = ((ideal_yaw - self.view_yaw + 540.0) % 360.0) - 180.0;
        let step = diff.clamp(-max_step, max_step);
        self.view_yaw += step;
        // Normalise to [-180, 180).
        self.view_yaw = ((self.view_yaw + 540.0) % 360.0) - 180.0;
        self.view_yaw
    }

    /// Arrive throttle: scale forward magnitude to `[ARRIVE_MIN, 1]` inside `ARRIVE_RADIUS`
    /// of the final goal. Returns 1.0 when outside the radius.
    pub fn arrive_scale(dist_to_goal: f32) -> f32 {
        if dist_to_goal < ARRIVE_RADIUS {
            (dist_to_goal / ARRIVE_RADIUS).clamp(ARRIVE_MIN, 1.0)
        } else {
            1.0
        }
    }

    /// Tick the circle-strafe direction (call once per bot tick with measured `dt`).
    /// Returns the current strafe direction (+1 left, -1 right) after flipping if the
    /// period has elapsed. Only meaningful in `Engage` + LOS; callers gate on that.
    pub fn strafe_tick(&mut self, dt: f32) -> f32 {
        self.strafe_elapsed += dt;
        if self.strafe_elapsed >= STRAFE_PERIOD_SECS {
            self.strafe_dir *= -1.0;
            self.strafe_elapsed = 0.0;
        }
        self.strafe_dir
    }
}

// ── View-basis helpers (public for scenario use) ──────────────────────────────

/// World-space forward unit vector for the given yaw (degrees).
/// Q2 convention: yaw 0 → +X, yaw 90 → +Y.
pub fn view_forward(yaw_deg: f32) -> Vec3 {
    let r = yaw_deg.to_radians();
    Vec3::new(r.cos(), r.sin(), 0.0)
}

/// World-space right unit vector for the given yaw (degrees).
/// 90° clockwise from forward: yaw 0 → +Y strafe-right is actually −Y (Q2 right = -Y).
/// In Q2, `right = (sin(yaw), -cos(yaw), 0)` from `AngleVectors` in `mathlib.c`.
pub fn view_right(yaw_deg: f32) -> Vec3 {
    let r = yaw_deg.to_radians();
    Vec3::new(r.sin(), -r.cos(), 0.0)
}

// ── Move decomposition ─────────────────────────────────────────────────────────

/// Decompose a desired world-space move direction into view-relative `(forward, side)`.
///
/// **`face_then_go = true`** (path following): forward is throttled by the bot's alignment
/// with the move direction — the bot first turns, then accelerates. Prevents
/// "moving full speed while facing the wrong way". Moonwalking (fwd < 0) is suppressed.
///
/// **`face_then_go = false`** (circle-strafe): raw dot-products — the bot walks in the
/// desired world direction regardless of which way it faces, enabling perpendicular strafing
/// while locked on an enemy.
///
/// The output magnitude is normalised to ≤ 1.0 to respect `pm_maxspeed` on diagonal moves
/// (`PM_AirAccelerate` / `PM_Accelerate` use the combined wish-vector — no sqrt-2 clamp
/// server-side, so we must do it ourselves). See `context/distilled.md §physics`.
pub fn move_from_world_dir(world_move_dir: Vec3, view_yaw: f32, face_then_go: bool) -> (f32, f32) {
    if world_move_dir.length_squared() < 1e-6 {
        return (0.0, 0.0);
    }
    let fwd_axis = view_forward(view_yaw);
    let right_axis = view_right(view_yaw);
    let fwd = world_move_dir.dot(fwd_axis);
    let side = world_move_dir.dot(right_axis);

    let (out_fwd, out_side) = if face_then_go {
        // `align` = how much we're facing the target (0 = sideways/backward, 1 = head-on).
        // `fwd.abs() * align` grows from 0 to 1 as the bot turns toward the direction.
        let align = fwd.max(0.0);
        let out_fwd = fwd.abs().min(1.0) * align;
        let out_side = side.clamp(-1.0, 1.0);
        (out_fwd, out_side)
    } else {
        (fwd.clamp(-1.0, 1.0), side.clamp(-1.0, 1.0))
    };

    // Normalise diagonal to magnitude ≤ 1.0.
    let mag_sq = out_fwd * out_fwd + out_side * out_side;
    if mag_sq > 1.0 {
        let inv = 1.0 / mag_sq.sqrt();
        (out_fwd * inv, out_side * inv)
    } else {
        (out_fwd, out_side)
    }
}
