//! Water-state detection (Plan 40).
//!
//! The Q2 wire protocol does not carry the player's `waterlevel`, but the brain owns the
//! [`CollisionModel`] and its own origin, so we recompute it exactly the way `pmove` does
//! (`PM_CategorizePosition`, `pmove.c:765-790`): sample `CONTENTS_WATER` at three heights —
//! feet, mid, eye — and count how many (bottom-up) are submerged. Only `CONTENTS_WATER`
//! counts; lava/slime are deadly and never swum (Plan 39 sampled swim nodes the same way).

use glam::Vec3;
use world::{CollisionModel, CONTENTS_WATER};

/// Standing hull `mins[2]` (`VEC_HULL_MIN.z`); the feet sample sits 1u above it.
const MINS_Z: f32 = -24.0;
/// Standard standing `viewheight` (`pmove.c` / `client.c`); the eye sample sits here.
const VIEWHEIGHT: f32 = 22.0;

/// Waterlevel ∈ {0,1,2,3} at `origin` (Plan 40), mirroring `PM_CategorizePosition`:
/// `0` dry, `1` feet wet, `2` waist-deep (**swimming**), `3` head under (fully submerged).
pub fn water_level(cm: &CollisionModel, origin: Vec3) -> u8 {
    let sample2 = VIEWHEIGHT - MINS_Z; // eye offset above mins (≈46)
    let sample1 = sample2 / 2.0; // mid offset (≈23)
    let at =
        |dz: f32| cm.point_contents(&[origin.x, origin.y, origin.z + dz]) & CONTENTS_WATER != 0;
    if !at(MINS_Z + 1.0) {
        return 0;
    }
    if !at(MINS_Z + sample1) {
        return 1;
    }
    if !at(MINS_Z + sample2) {
        return 2;
    }
    3
}

/// True once the bot is waterborne enough to swim (waist-deep or more): `waterlevel >= 2`.
/// At this depth Q2 switches to `PM_WaterMove`, so the brain must drive vertical thrust.
#[inline]
pub fn is_swimming(level: u8) -> bool {
    level >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A z-layered world: solid below z=0, water 0..120, air above (in the central channel).
    fn pool() -> CollisionModel {
        world::water_channel_world()
    }

    #[test]
    fn levels_track_depth() {
        let cm = pool();
        // Deep: origin high enough that feet/mid/eye are all in water (water tops at z=120).
        // origin.z=60 → feet=37, mid=59, eye=82 — all in (0,120) water.
        assert_eq!(water_level(&cm, Vec3::new(0.0, 0.0, 60.0)), 3);
        // Dry side ledge (x=100, no water): level 0.
        assert_eq!(water_level(&cm, Vec3::new(100.0, 0.0, 30.0)), 0);
        // Surface: origin.z=110 → feet=87 (water), mid=109 (water), eye=132 (>120, air) → 2.
        assert_eq!(water_level(&cm, Vec3::new(0.0, 0.0, 110.0)), 2);
    }

    #[test]
    fn is_swimming_threshold() {
        assert!(!is_swimming(0));
        assert!(!is_swimming(1));
        assert!(is_swimming(2));
        assert!(is_swimming(3));
    }
}
