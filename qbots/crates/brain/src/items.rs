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

use crate::brains::core::MapItem;
use crate::perception::{classify_item_classname, EntityClass, Worldview};
use crate::skill::BotSkill;
use crate::weapons::Weapon;
use glam::Vec3;

/// Build the static item table (Plan 30) from the map's BSP entity lump: every `item_*`/
/// `weapon_*`/`ammo_*` spawn entity, classified to an [`EntityClass`] and resolved to its nearest
/// nav-graph node (for A*-distance scoring). Built once per map, shared read-only via
/// [`BrainMap::items`](crate::brains::core::BrainMap::items). Entities without a parseable origin
/// or a non-item classname are skipped.
pub fn build_map_items(bsp: &world::Bsp, graph: &world::NavGraph) -> Vec<MapItem> {
    bsp.entities
        .iter()
        .filter_map(|e| {
            let class = classify_item_classname(&e.classname)?;
            let origin = e.origin()?;
            Some(MapItem {
                class,
                origin: Vec3::from(origin),
                nav_node: graph.nearest(&origin),
            })
        })
        .collect()
}

/// Q2 item respawn delay (seconds) by class — how long a pad stays empty after pickup before
/// the item returns (`vendor/yquake2/src/game/g_items.c`: most items `SetRespawn(…, 30)`; armor
/// 20 s; powerups 60 s+). Used by [`ItemMemory`] to stop bots running to a pad they just saw empty.
fn respawn_time(class: EntityClass) -> f32 {
    match class {
        EntityClass::ItemArmor => 20.0,
        EntityClass::ItemPowerup => 60.0,
        // weapons / ammo / health
        _ => 30.0,
    }
}

/// Distance within which we trust the PVS to tell us an item is present/absent: if the bot is
/// this close to a known item spawn and the server is NOT transmitting the item entity, the pad
/// is genuinely empty (PVS would carry a nearby item), so it was taken. Beyond this the absence
/// is uninformative (PVS-culled ≠ taken), so we leave the item "assumed present".
const ITEM_TRUST_RANGE: f32 = 500.0;
/// An item entity within this distance of a known spawn origin counts as "that pad is stocked".
const ITEM_PAD_RADIUS: f32 = 48.0;

/// True for a pickup class the bot would seek (excludes players/projectiles/unknown).
fn is_pickup(class: EntityClass) -> bool {
    matches!(
        class,
        EntityClass::ItemHealth
            | EntityClass::ItemArmor
            | EntityClass::ItemWeapon
            | EntityClass::ItemPowerup
    )
}

/// Per-bot memory of which map items are currently taken (Plan 30 T2). PVS-honest: it only
/// records what **this bot** has itself observed (a spawn pad seen empty within trusted range),
/// decaying back to "assume present" after the class respawn timer — never shared omniscience.
///
/// Keyed by index into the [`BrainMap::items`](crate::brains::core::BrainMap::items) table.
#[derive(Debug, Default, Clone)]
pub struct ItemMemory {
    /// item-index → wall-clock seconds when we last saw its pad empty.
    taken: std::collections::HashMap<usize, f32>,
}

impl ItemMemory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update the memory from this frame: for every known item within [`ITEM_TRUST_RANGE`], mark
    /// it taken (or refresh the timer) if no matching item entity is on the pad, or clear the mark
    /// if we can see it's stocked. Items out of trust range are left untouched. `now` is the bot's
    /// monotonic clock (seconds).
    pub fn observe(&mut self, items: &[MapItem], view: &Worldview, now: f32) {
        let bot = view.self_state().origin;
        for (i, item) in items.iter().enumerate() {
            if !is_pickup(item.class) {
                continue;
            }
            if (bot - item.origin).length() > ITEM_TRUST_RANGE {
                continue; // too far to trust the PVS absence
            }
            let stocked = view
                .entities()
                .any(|e| is_pickup(e.class) && (e.origin - item.origin).length() < ITEM_PAD_RADIUS);
            if stocked {
                self.taken.remove(&i); // seen present → forget any stale "taken" mark
            } else {
                // Pad empty within trust range. Mark it taken; if it was already marked and the
                // respawn timer has *elapsed* yet the pad is STILL empty, re-arm the timer (someone
                // grabbed it again the instant it returned) so we don't route to an empty pad.
                let refresh = self
                    .taken
                    .get(&i)
                    .is_none_or(|&t| now - t > respawn_time(item.class));
                if refresh {
                    self.taken.insert(i, now);
                }
            }
        }
    }

    /// Is map item `i` (of `class`) likely available now? `true` if never seen taken, or the
    /// respawn timer has elapsed since we last saw it empty.
    pub fn available(&self, i: usize, class: EntityClass, now: f32) -> bool {
        self.taken
            .get(&i)
            .is_none_or(|&t| now - t > respawn_time(class))
    }
}

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

/// **`main`-brain-only** loadout/health-aware item picker (Plan 45). Extends
/// [`best_item_goal`] with two strategic biases so the bot *builds up* instead of
/// grabbing whatever is nearest:
/// - **Weapon hunger** when we hold only the spawn Blaster (×4) or a bare
///   Machinegun/Chaingun (×2) — a real weapon is the single biggest survivability
///   upgrade against a precise opponent.
/// - **Health/armor hunger** that ramps as our health/armor drops.
///
/// `q3`'s [`best_item_goal`] is intentionally left as the neutral baseline so the
/// competing brain is untouched. Returns the best item's origin + class, or `None`.
pub fn best_item_goal_weighted(
    view: &Worldview,
    skill: &BotSkill,
    held_weapon: Option<Weapon>,
    health: i32,
    armor: i32,
) -> Option<(Vec3, EntityClass)> {
    let origin = view.self_state().origin;

    // A real weapon is worth detouring for when we're stuck on the spawn loadout.
    let weapon_mult = match held_weapon {
        None | Some(Weapon::Blaster) => 4.0,
        Some(Weapon::Machinegun) | Some(Weapon::Chaingun) => 2.0,
        _ => 1.0,
    };
    // Health / armor hunger ramps as we drop below full.
    let health_mult = if health < 50 {
        3.0
    } else if health < 80 {
        1.6
    } else {
        1.0
    };
    let armor_mult = if armor < 30 {
        2.5
    } else if armor < 80 {
        1.4
    } else {
        1.0
    };

    view.items()
        .map(|e| {
            let dist = (e.origin - origin).length().max(1.0);
            let mut val = item_value(e.class, skill);
            match e.class {
                EntityClass::ItemWeapon => val *= weapon_mult,
                EntityClass::ItemHealth => val *= health_mult,
                EntityClass::ItemArmor => val *= armor_mult,
                _ => {}
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

    #[test]
    fn weighted_goal_none_when_no_items() {
        use client::parse::ConfigStrings;
        use q2proto::Frame;
        let view = Worldview::from_frame(&Frame::default(), &ConfigStrings::default(), 0);
        let skill = BotSkill::default();
        assert!(
            best_item_goal_weighted(&view, &skill, Some(Weapon::Blaster), 100, 100).is_none(),
            "no items in view → no goal"
        );
    }

    // ── ItemMemory (Plan 30 T2) ────────────────────────────────────────────────────────────
    use client::parse::ConfigStrings;
    use q2proto::{EntityState, Frame};

    /// CS_MODELS base (perception's private const) — a model configstring is at `32 + modelindex`.
    const CS_MODELS: usize = 32;

    /// Worldview with the bot at `self_origin` and the given item entities (each a `(origin,
    /// model_string)`; the model string is classified by the perception layer — use one containing
    /// "health" for an `ItemHealth`).
    fn view_with(self_origin: [f32; 3], items: &[([f32; 3], &str)]) -> Worldview {
        let mut cs = ConfigStrings::default();
        let mut frame = Frame::default();
        frame.playerstate.pmove.origin = [
            (self_origin[0] * 8.0) as i16,
            (self_origin[1] * 8.0) as i16,
            (self_origin[2] * 8.0) as i16,
        ];
        for (i, (origin, model)) in items.iter().enumerate() {
            let modelindex = 40 + i;
            cs.set(CS_MODELS + modelindex, *model);
            frame.entities.push(EntityState {
                number: 60 + i as i32,
                modelindex: modelindex as i32,
                origin: *origin,
                ..Default::default()
            });
        }
        Worldview::from_frame(&frame, &cs, 0)
    }

    fn health_item(origin: [f32; 3]) -> MapItem {
        MapItem {
            class: EntityClass::ItemHealth,
            origin: Vec3::from(origin),
            nav_node: None,
        }
    }

    #[test]
    fn empty_pad_in_range_is_taken_then_respawns() {
        let mut mem = ItemMemory::new();
        let items = [health_item([100.0, 0.0, 0.0])];
        // Bot near the pad (within trust range), no item entity present → pad is taken.
        let view = view_with([120.0, 0.0, 0.0], &[]);
        mem.observe(&items, &view, 10.0);
        assert!(
            !mem.available(0, EntityClass::ItemHealth, 10.0),
            "just seen empty"
        );
        // Health respawns after 30 s.
        assert!(
            !mem.available(0, EntityClass::ItemHealth, 39.0),
            "still within 30 s"
        );
        assert!(
            mem.available(0, EntityClass::ItemHealth, 41.0),
            "respawned after 30 s"
        );
    }

    #[test]
    fn far_pad_absence_is_uninformative() {
        let mut mem = ItemMemory::new();
        let items = [health_item([5000.0, 0.0, 0.0])]; // way beyond ITEM_TRUST_RANGE
        let view = view_with([0.0, 0.0, 0.0], &[]);
        mem.observe(&items, &view, 10.0);
        assert!(
            mem.available(0, EntityClass::ItemHealth, 10.0),
            "far pad's absence is PVS-culling, not taken → assume present"
        );
    }

    #[test]
    fn stocked_pad_clears_a_stale_taken_mark() {
        let mut mem = ItemMemory::new();
        let items = [health_item([100.0, 0.0, 0.0])];
        // First: seen empty → taken.
        mem.observe(&items, &view_with([120.0, 0.0, 0.0], &[]), 10.0);
        assert!(!mem.available(0, EntityClass::ItemHealth, 12.0));
        // Later: bot returns and the pad is stocked (a health entity sits on it) → cleared.
        let stocked = view_with(
            [120.0, 0.0, 0.0],
            &[([100.0, 0.0, 0.0], "models/items/health")],
        );
        mem.observe(&items, &stocked, 15.0);
        assert!(
            mem.available(0, EntityClass::ItemHealth, 15.0),
            "seen stocked → available again immediately"
        );
    }
}
