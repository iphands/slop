//! Item rating and goal selection.
//!
//! Ports Eraser's `dist_divide` item weighting (`RoamFindBestItem`,
//! `bot_items.c:100-326`; see `distilled/eraser.md` §8). Eraser scores each
//! pickup as `value / dist_divide`; lower `dist_divide` = more eager to grab it.
//! We expose the per-class value directly and a goal picker.
//!
//! Eraser's `dist_divide` defaults to `1`, but real powerups warrant explicit
//! higher values (one of this plan's "fix Eraser's gaps" items): Quad/Invuln are
//! game-changing, mega-health is a big swing, etc. The `quad_freak` personality
//! doubles the Quad rating.

use crate::perception::{EntityClass, Worldview};
use crate::skill::BotSkill;
use glam::Vec3;

/// Base desirability of an item class (higher = more worth detouring for).
/// Loosely Eraser's `1 / dist_divide`: Quad/Invuln dominate, then mega/armor,
/// then weapons, then plain health.
fn base_value(class: EntityClass) -> f32 {
    match class {
        // Powerups (Eraser left these at default 1; we value them explicitly).
        EntityClass::ItemPowerup => 5.0,
        EntityClass::ItemArmor => 4.0,
        EntityClass::ItemWeapon => 3.0,
        EntityClass::ItemHealth => 2.0,
        _ => 0.0,
    }
}

/// Effective item value, applying personality (`quad_freak` over-weights powerups).
pub fn item_value(class: EntityClass, skill: &BotSkill) -> f32 {
    let v = base_value(class);
    if skill.quad_freak && class == EntityClass::ItemPowerup {
        v * 2.0
    } else {
        v
    }
}

/// The best item to navigate toward: highest `value / distance`, considering the
/// bot's health need (a low-health bot weights health/armor up). Returns the
/// item's origin and class, or `None` if no item is visible.
pub fn best_item_goal(view: &Worldview, skill: &BotSkill) -> Option<(Vec3, EntityClass)> {
    let origin = view.self_state().origin;
    let low_health = view.is_low_health();
    view.items()
        .map(|e| {
            let dist = (e.origin - origin).length().max(1.0);
            let mut val = item_value(e.class, skill);
            // A hurt bot prioritizes health/armor.
            if low_health && matches!(e.class, EntityClass::ItemHealth | EntityClass::ItemArmor) {
                val *= 2.0;
            }
            (val / dist, e.origin, e.class)
        })
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, origin, class)| (origin, class))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powerup_outranks_health() {
        let skill = BotSkill::default();
        assert!(
            item_value(EntityClass::ItemPowerup, &skill)
                > item_value(EntityClass::ItemHealth, &skill)
        );
        assert!(
            item_value(EntityClass::ItemArmor, &skill)
                > item_value(EntityClass::ItemWeapon, &skill)
        );
    }

    #[test]
    fn quad_freak_doubles_powerup() {
        let mut skill = BotSkill::default();
        let base = item_value(EntityClass::ItemPowerup, &skill);
        skill.quad_freak = true;
        assert!((item_value(EntityClass::ItemPowerup, &skill) - base * 2.0).abs() < 0.001);
        // Non-powerups unaffected.
        assert!(
            (item_value(EntityClass::ItemHealth, &skill)
                - item_value(EntityClass::ItemHealth, &BotSkill::default()))
            .abs()
                < 0.001
        );
    }

    #[test]
    fn unknown_item_is_worthless() {
        let skill = BotSkill::default();
        assert_eq!(item_value(EntityClass::Unknown, &skill), 0.0);
    }
}
