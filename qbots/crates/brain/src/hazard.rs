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

use crate::recover::find_best_direction;
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

/// Inward push away from nearby deadly rims, or `None` when no rim is near.
///
/// Plan 63 (q2dm6 telemetry): **81% of lava entries had damage in the prior 1.5 s** —
/// bots dueling ON walkway rims get rocket-juggled or strafe-drift in, and the basin
/// walls are sheer (100–280u), so entry ≈ death. The per-direction gates only veto the
/// COMMANDED direction; knockback + tracking drift need standing clearance. This probes
/// 8 compass directions and sums the inward opposites of every hazardous one — combat
/// movement adds the bias so fights slide away from rims instead of along them.
pub fn rim_pressure(cm: &CollisionModel, pos: Vec3) -> Option<Vec3> {
    let mut inward = Vec3::ZERO;
    for i in 0..8 {
        let a = (i as f32) * std::f32::consts::FRAC_PI_4;
        let dir = Vec3::new(a.cos(), a.sin(), 0.0);
        if dir_is_hazardous(cm, pos, dir) {
            inward -= dir;
        }
    }
    inward.try_normalize()
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

/// Speed multiplier for walking a hazard-bordered stretch (creep, don't sprint).
const CREEP: f32 = 0.35;

/// Milder governor for hazard BESIDE the move direction (rim-parallel running).
const CREEP_LATERAL: f32 = 0.55;

/// Per-tick speed governor for path following near deadly ground (Plan 50). q2dm3's
/// central maze is walkways at z≈−48 with lava 16 u below — a LEGAL step-down, so the
/// nav polyline is valid, but at full sprint the 10 Hz control wobble (yaw rate limit,
/// arrive overshoot, corner momentum) steps the bot off the edge. Humans slow down on
/// lava walkways; so do we: when the move direction's short probe borders a hazard,
/// return [`CREEP`], else 1.0. This never vetoes the move — the path is valid — it
/// shrinks the tracking error until the stretch is past.
///
/// Plan 63: the frontal probe misses **rim-parallel** sprints — q2dm6 telemetry shows
/// lava entries at 200–350 u/s with the commanded direction ALONG the walkway (safe) and
/// the lava beside it; 10 Hz drift then carries the bot off sideways. When either
/// perpendicular borders a hazard, apply the milder [`CREEP_LATERAL`].
pub fn creep_scale(cm: Option<&CollisionModel>, pos: Vec3, world_dir: Vec3) -> f32 {
    let Some(c) = cm else { return 1.0 };
    if dir_is_hazardous(c, pos, world_dir) {
        return CREEP;
    }
    let flat = Vec3::new(world_dir.x, world_dir.y, 0.0);
    if flat.length_squared() > 1e-6 {
        let side = Vec3::new(-flat.y, flat.x, 0.0);
        if dir_is_hazardous(c, pos, side) || dir_is_hazardous(c, pos, -side) {
            return CREEP_LATERAL;
        }
    }
    1.0
}

/// Last-resort combat move when BOTH strafe variants are deadly (fighting at a pool rim):
/// the most open lava/ledge-free heading, as a world direction. A bot that stands still at
/// a rim under rocket fire gets juggled straight into the pool (Plan 50 soak: lava entries
/// ROSE after the stand-and-fight fallback shipped) — retreating from the rim while the
/// view stays locked on the enemy is strictly safer. `None` when every direction is bad.
pub fn rim_retreat_dir(cm: &CollisionModel, pos: Vec3, view_yaw: f32) -> Option<Vec3> {
    let (yaw, _) = find_best_direction(cm, pos, view_yaw)?;
    let r = yaw.to_radians();
    Some(Vec3::new(r.cos(), r.sin(), 0.0))
}

/// Escape direction when the bot is STANDING IN lava/slime, or `None` when it isn't.
///
/// Everything else in this module keeps bots OUT of hazards; once a bot is in one
/// (knockback, a fall, a fight gone wrong), those same gates freeze it — every direction
/// reads hazardous, `find_best_direction` rejects all wet endpoints, and lava is not
/// `CONTENTS_WATER` so no swim machinery runs. The Plan 48 soak showed bots burning
/// 100→0 over 15 s without moving out (Plan 50 E2). This is the counterpart: fan 16
/// yaws, march outward (32..192 u), and return the direction of the CLOSEST safe
/// standable floor (within ±64 u of our height — pool rims sit above the surface).
/// Callers must treat it as a survival override: face it, sprint, jump.
pub fn escape_from_lava(cm: &CollisionModel, pos: Vec3) -> Option<Vec3> {
    const DEADLY: i32 = CONTENTS_LAVA | CONTENTS_SLIME;
    let feet = [pos.x, pos.y, pos.z - 20.0];
    if cm.point_contents(&[pos.x, pos.y, pos.z]) & DEADLY == 0
        && cm.point_contents(&feet) & DEADLY == 0
    {
        return None;
    }
    let zero = [0.0f32; 3];
    let mut best: Option<(f32, Vec3)> = None;
    for i in 0..16 {
        let yaw = (i as f32) * (std::f32::consts::TAU / 16.0);
        let dir = Vec3::new(yaw.cos(), yaw.sin(), 0.0);
        for dist in [32.0f32, 64.0, 96.0, 128.0, 160.0, 192.0] {
            if best.is_some_and(|(bd, _)| dist >= bd) {
                break; // can't beat the current best along this ray
            }
            let p = pos + dir * dist;
            let top = [p.x, p.y, pos.z + 64.0];
            if cm.point_contents(&top) & MASK_SOLID != 0 {
                continue; // inside the pool wall at this range — try farther out
            }
            let bot = [p.x, p.y, pos.z - 64.0];
            let t = cm.trace(&top, &bot, &zero, &zero, MASK_SOLID);
            if t.startsolid || t.fraction >= 1.0 {
                continue; // no floor in the band — still over the pool
            }
            let above = [t.endpos[0], t.endpos[1], t.endpos[2] + 1.0];
            if cm.point_contents(&above) & DEADLY != 0 {
                continue; // that floor is more lava
            }
            let origin = [t.endpos[0], t.endpos[1], t.endpos[2] + 24.0];
            if cm.point_contents(&origin) != 0 {
                continue; // no headroom for a standing bot
            }
            best = Some((dist, dir));
            break;
        }
    }
    best.map(|(_, d)| d)
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
