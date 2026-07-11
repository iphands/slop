//! Ground-hazard probe — "is walking that way deadly?" (Plan 48 T2).
//!
//! Combat movement (circle-strafe, backpedal, kite), projectile dodges, and stuck-recovery
//! strafes emit **world-space directions that never came from the nav graph**, so none of the
//! graph's walkability guarantees apply. On q2dm3 the classic death was backpedaling away
//! from an enemy straight into the central lava. This module gives those call sites one
//! cheap question to ask before committing: does the ground ahead in that direction exist,
//! and is it survivable?
//!
//! Deliberately NOT used for route pursuit — path polylines are validated by
//! `pursue_target_safe`/`segment_has_floor` (lava-aware since Plan 48 T1), and traversal
//! legs (jump/swim/ride) intentionally cross what this probe would flag.

use glam::Vec3;
use world::{CollisionModel, CONTENTS_LAVA, CONTENTS_SLIME, MASK_SOLID};

use crate::steer::view_right;

/// Q2 pmove auto-step height (`pmove.c` STEPSIZE) — samples are lifted by this so a
/// walkable step edge doesn't read as a wall/void.
const STEPSIZE: f32 = 18.0;
/// Horizontal sample distances along the candidate direction. Two samples: the near one
/// catches an edge already underfoot; the far one gives ~3 ticks of warning at run speed.
const SAMPLE_DISTS: [f32; 2] = [24.0, 48.0];
/// How far below a sample the floor may be before the direction counts as a blind drop.
/// Generous (a big step-down or short hop is fine); a real ledge/pit exceeds it.
const DROP_PROBE: f32 = 128.0;

/// True if walking from `pos` along `world_dir` (XY) leads into lava/slime or off a blind
/// drop (no floor within [`DROP_PROBE`]). Walls are NOT hazards — a wall stops the bot and
/// is the stuck detector's business; this probe only vetoes directions that *kill*.
///
/// `world_dir` need not be normalized; a near-zero vector is never hazardous.
pub fn dir_is_hazardous(cm: &CollisionModel, pos: Vec3, world_dir: Vec3) -> bool {
    let flat = Vec3::new(world_dir.x, world_dir.y, 0.0);
    let Some(dir) = flat.try_normalize() else {
        return false;
    };
    let zero = [0.0f32; 3];
    for dist in SAMPLE_DISTS {
        let s = pos + dir * dist;
        let sample = [s.x, s.y, s.z + STEPSIZE];
        if cm.point_contents(&sample) & MASK_SOLID != 0 {
            // Inside a wall — the bot can't get there, so nothing beyond it matters.
            return false;
        }
        if cm.point_contents(&sample) & (CONTENTS_LAVA | CONTENTS_SLIME) != 0 {
            return true;
        }
        let down = [sample[0], sample[1], sample[2] - STEPSIZE - DROP_PROBE];
        let t = cm.trace(&sample, &down, &zero, &zero, MASK_SOLID);
        if t.fraction >= 1.0 && !t.startsolid {
            return true; // blind drop — no floor within DROP_PROBE
        }
        let above = [t.endpos[0], t.endpos[1], t.endpos[2] + 1.0];
        if cm.point_contents(&above) & (CONTENTS_LAVA | CONTENTS_SLIME) != 0 {
            return true; // the "floor" is a lava/slime bed
        }
    }
    false
}

/// Pick a survivable variant of a combat move direction: `normalize(radial + tangential)`
/// times `scale`, then the same with the tangential (strafe) component mirrored, else
/// `None` (hold position — standing beats swimming in lava). `radial` and `tangential` are
/// the two world-space components the caller composed (either may be `Vec3::ZERO`);
/// `scale` is the caller's speed damping (0.7 hold-strafe), applied after normalization
/// exactly as the pre-Plan-48 call sites did.
pub fn safe_combat_dir(
    cm: Option<&CollisionModel>,
    pos: Vec3,
    radial: Vec3,
    tangential: Vec3,
    scale: f32,
) -> Option<Vec3> {
    let primary = (radial + tangential).normalize_or_zero();
    let Some(cm) = cm else {
        return Some(primary * scale);
    };
    if !dir_is_hazardous(cm, pos, primary) {
        return Some(primary * scale);
    }
    let mirrored = (radial - tangential).normalize_or_zero();
    if mirrored.length_squared() > 1e-6 && !dir_is_hazardous(cm, pos, mirrored) {
        return Some(mirrored * scale);
    }
    None
}

/// Survivable variant of a view-relative side-step (`RecoveryAction::Strafe`): the
/// requested `dir` (±1) if its world direction is safe, the flipped side if not, `0.0`
/// when both sides are deadly.
pub fn safe_strafe_dir(cm: Option<&CollisionModel>, pos: Vec3, view_yaw: f32, dir: f32) -> f32 {
    let Some(cm) = cm else {
        return dir;
    };
    let world = view_right(view_yaw) * dir;
    if !dir_is_hazardous(cm, pos, world) {
        return dir;
    }
    if !dir_is_hazardous(cm, pos, -world) {
        return -dir;
    }
    0.0
}
