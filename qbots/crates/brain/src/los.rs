//! Line-of-sight — a BSP-trace gate on whether the bot can *actually* see a point.
//!
//! Plan 11. The FOV cone in [`crate::perception::Worldview::nearest_enemy`] selects
//! the nearest enemy whose *direction* is in view — with **no geometry test**, so a
//! bot "sees" and chases/fires at enemies behind walls. These helpers wrap
//! [`world::CollisionModel::trace`] so enemy selection, the FSM's Engage
//! transition, the fire decision, and nav-to-enemy goals can all require a clear
//! line of sight.
//!
//! Uses a **zero-size** (`mins=maxs=0`) trace: we care whether the *line* is clear,
//! not whether a player box fits along it. Hull traces stay for movement (Plans 12/13).

use world::{CollisionModel, MASK_SOLID};

/// Standing eye height above the bot origin (`pm_viewheight ≈ 22`; the origin sits
/// ~24 above the floor).
pub const EYE_Z: f32 = 22.0;
/// Enemy "chest" offset above its origin — the upper LOS target point.
const CHEST_Z: f32 = 12.0;
/// Enemy "feet" offset below its origin — the lower LOS target point (low cover).
const FEET_Z: f32 = -20.0;

/// The eye origin (bot origin lifted by [`EYE_Z`]) traces start from.
pub fn eye_origin(self_origin: [f32; 3]) -> [f32; 3] {
    [self_origin[0], self_origin[1], self_origin[2] + EYE_Z]
}

/// True if the line `eye → target` is unobstructed: the trace reaches the full
/// distance (`fraction >= 1.0`) and didn't start embedded (`!startsolid`). A
/// `startsolid` result (eye inside geometry) is treated as blocked.
pub fn has_los(cm: &CollisionModel, eye: [f32; 3], target: [f32; 3]) -> bool {
    let t = cm.trace(&eye, &target, &[0.0; 3], &[0.0; 3], MASK_SOLID);
    t.fraction >= 1.0 && !t.startsolid
}

/// Two-point player LOS (Eraser-style): visible if **either** eye→chest or
/// eye→feet is clear, so an enemy partially behind low cover still counts as seen.
/// `enemy_origin` is the enemy's bot origin (feet-relative, like ours).
pub fn has_los_player(cm: &CollisionModel, eye: [f32; 3], enemy_origin: [f32; 3]) -> bool {
    let chest = [enemy_origin[0], enemy_origin[1], enemy_origin[2] + CHEST_Z];
    let feet = [enemy_origin[0], enemy_origin[1], enemy_origin[2] + FEET_Z];
    has_los(cm, eye, chest) || has_los(cm, eye, feet)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A vertical wall at x=0 (x<0 solid). Everything at x>0 is clear sight.
    fn wall() -> CollisionModel {
        CollisionModel::half_space([1.0, 0.0, 0.0], 0.0)
    }

    #[test]
    fn clear_line_has_los() {
        let cm = wall();
        // Eye and target both well into the empty side (x>0), same height.
        assert!(has_los(&cm, [50.0, 0.0, EYE_Z], [100.0, 0.0, EYE_Z]));
        assert!(has_los_player(&cm, [50.0, 0.0, EYE_Z], [100.0, 0.0, 0.0]));
    }

    #[test]
    fn line_through_wall_has_no_los() {
        let cm = wall();
        // Eye at x=50, target across the wall at x=-50 → blocked.
        assert!(!has_los(&cm, [50.0, 0.0, EYE_Z], [-50.0, 0.0, EYE_Z]));
        assert!(!has_los_player(&cm, [50.0, 0.0, EYE_Z], [-50.0, 0.0, 0.0]));
    }

    #[test]
    fn eye_embedded_is_not_los() {
        let cm = wall();
        // Eye inside the solid half (x<0) → startsolid → no LOS even to a nearby point.
        assert!(!has_los(&cm, [-50.0, 0.0, EYE_Z], [-40.0, 0.0, EYE_Z]));
    }

    #[test]
    fn eye_origin_lifts_by_viewheight() {
        assert_eq!(eye_origin([1.0, 2.0, 3.0]), [1.0, 2.0, 3.0 + EYE_Z]);
    }
}
