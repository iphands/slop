//! # brain::brains::jev::pads — out-of-sight pickup candidates (Plan 74 T3)
//!
//! q3 only heads for pickups it can see (`Q3Brain::roam_goal` over `view.items()`) and drops the
//! BSP item table. This module is what the jev policy offers the model instead: per class, the
//! pad with the **lowest travel cost** from where the bot stands that is
//! - **worth having** — a weapon stronger than the held one (`Weapon::power_tier`; an
//!   unrecognised held weapon counts as tier 0), health below [`FULL_HEALTH`], armor below
//!   [`FULL_ARMOR`];
//! - **not known empty** — `ItemMemory::available` (Q2 respawn times; the arena server runs
//!   without `DF_WEAPONS_STAY`, so a taken weapon pad is empty for 30 s);
//! - **reachable** — finite cost in one `NavGraph::flood_costs` pass from the bot's nearest node
//!   (one Dijkstra prices every pad; called per ask, never per tick).
//!
//! Ammo pads are never weapon candidates (`MapItem::weapon` is `None` for them — T2).

use glam::Vec3;
use world::NavGraph;

use crate::brains::core::MapItem;
use crate::items::ItemMemory;
use crate::jev::question::Offered;
use crate::jev::state::{DistBand, PadFacts};
use crate::perception::{EntityClass, Worldview};
use crate::weapons::Weapon;

/// Health below this is worth topping up.
pub const FULL_HEALTH: i32 = 100;
/// Armor below this is worth topping up.
pub const FULL_ARMOR: i32 = 100;

/// A pad the bot could walk to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pad {
    /// Index into the map's item table (`BrainMap::items`).
    pub item: usize,
    /// The pad's origin — the goal the objective walks to.
    pub origin: Vec3,
    /// Travel cost from the bot's nearest node (graph edge-cost units ≈ world units).
    pub cost: f32,
}

/// The best pad per class, if any.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PadCandidates {
    /// Best weapon pad stronger than the held weapon.
    pub weapon: Option<Pad>,
    /// Best health pad.
    pub health: Option<Pad>,
    /// Best armor pad.
    pub armor: Option<Pad>,
}

impl PadCandidates {
    /// No candidate in any class — nothing worth asking about.
    pub fn is_empty(&self) -> bool {
        self.weapon.is_none() && self.health.is_none() && self.armor.is_none()
    }

    /// Which classes an objective ask offers.
    pub fn offered(&self) -> Offered {
        Offered {
            weapon: self.weapon.is_some(),
            health: self.health.is_some(),
            armor: self.armor.is_some(),
        }
    }

    /// The pads behind the offered options, **in [`Offered::labels`] order** — `None` for the
    /// trailing `carry_on`. An answer's option index resolves through this list.
    pub fn option_pads(&self) -> Vec<Option<Pad>> {
        [self.weapon, self.health, self.armor]
            .into_iter()
            .flatten()
            .map(Some)
            .chain([None])
            .collect()
    }

    /// The `jvr` control's fixed rule (Plan 74 T5), from the same candidates the model sees:
    /// a weak held weapon (blaster / shotgun / machinegun — the state's own definition) and a
    /// weapon pad → that pad; else health below 50 and a health pad → it; else armor below 50
    /// and an armor pad → it; else `None` (`carry_on`).
    pub fn rule_pick(&self, held: Option<Weapon>, hp: i32, armor: i32) -> Option<Pad> {
        let weak = matches!(
            held,
            Some(Weapon::Blaster | Weapon::Shotgun | Weapon::Machinegun)
        );
        [
            (weak, self.weapon),
            (hp < 50, self.health),
            (armor < 50, self.armor),
        ]
        .into_iter()
        .find_map(|(want, pad)| want.then_some(pad).flatten())
    }

    /// The closed facts the state carries: each candidate's travel-cost band.
    pub fn facts(&self) -> PadFacts {
        let band = |p: Option<Pad>| p.map(|p| DistBand::of(p.cost));
        PadFacts {
            weapon: band(self.weapon),
            health: band(self.health),
            armor: band(self.armor),
        }
    }
}

/// Which candidate slot `item` would fill for this bot, if it is worth having.
fn slot<'a>(
    out: &'a mut PadCandidates,
    item: &MapItem,
    held_tier: u8,
    hp: i32,
    armor: i32,
) -> Option<&'a mut Option<Pad>> {
    match item.class {
        EntityClass::ItemWeapon if item.weapon.is_some_and(|w| w.power_tier() > held_tier) => {
            Some(&mut out.weapon)
        }
        EntityClass::ItemHealth if hp < FULL_HEALTH => Some(&mut out.health),
        EntityClass::ItemArmor if armor < FULL_ARMOR => Some(&mut out.armor),
        _ => None,
    }
}

/// The best pad per class for the bot in `view` (see the module doc). Empty before the bot can
/// be placed on the graph.
pub fn candidates(
    items: &[MapItem],
    memory: &ItemMemory,
    graph: &NavGraph,
    view: &Worldview,
    now: f32,
) -> PadCandidates {
    let s = view.self_state();
    let mut out = PadCandidates::default();
    let Some(from) = graph.nearest(&s.origin.into()) else {
        return out;
    };
    let costs = graph.flood_costs(from);
    let held_tier = s.held_weapon.map_or(0, Weapon::power_tier);
    for (i, item) in items.iter().enumerate() {
        if !memory.available(i, item.class, now) {
            continue;
        }
        let Some(cost) = item
            .nav_node
            .and_then(|n| costs.get(n).copied())
            .filter(|c| c.is_finite())
        else {
            continue;
        };
        let Some(best) = slot(&mut out, item, held_tier, s.health, s.armor) else {
            continue;
        };
        if best.is_none_or(|p| cost < p.cost) {
            *best = Some(Pad {
                item: i,
                origin: item.origin,
                cost,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use client::parse::ConfigStrings;
    use q2proto::{EntityState, Frame};

    /// A corridor of 5 nodes 256 u apart along +x; node 4 is disconnected (unreachable).
    fn graph() -> NavGraph {
        let nodes = (0..5).map(|i| [i as f32 * 256.0, 0.0, 0.0]).collect();
        let link = |a: usize, b: usize| (b, ((a as f32) - (b as f32)).abs() * 256.0);
        let adj = vec![
            vec![link(0, 1)],
            vec![link(1, 0), link(1, 2)],
            vec![link(2, 1), link(2, 3)],
            vec![link(3, 2)],
            Vec::new(),
        ];
        NavGraph::from_raw(nodes, adj)
    }

    fn pad(class: EntityClass, weapon: Option<Weapon>, node: usize) -> MapItem {
        MapItem {
            class,
            weapon,
            origin: Vec3::new(node as f32 * 256.0, 0.0, 0.0),
            nav_node: Some(node),
        }
    }

    /// A bot at the origin holding `view_model`, with `health`/`armor`, seeing `entities`.
    fn view(view_model: &str, health: i16, armor: i16, entities: Vec<EntityState>) -> Worldview {
        let mut cs = ConfigStrings::default();
        cs.set(32 + 1, view_model);
        cs.set(32 + 2, "models/items/armor/body/tris.md2");
        let mut frame = Frame::default();
        frame.playerstate.gunindex = 1;
        frame.playerstate.stats[1] = health;
        frame.playerstate.stats[3] = 10;
        frame.playerstate.stats[5] = armor;
        frame.entities = entities;
        Worldview::from_frame(&frame, &cs, 0)
    }

    const BLASTER: &str = "models/weapons/v_blast/tris.md2";
    const RAIL: &str = "models/weapons/v_rail/tris.md2";

    #[test]
    fn jev_pads_out_of_sight_weapon_pad_is_a_candidate_ammo_is_not() {
        let items = [
            pad(EntityClass::ItemWeapon, None, 1), // ammo box, nearer
            pad(EntityClass::ItemWeapon, Some(Weapon::RocketLauncher), 3),
            pad(EntityClass::ItemWeapon, Some(Weapon::Railgun), 4), // unreachable
        ];
        // Nothing in view: the rocket pad is known only from the BSP table.
        let c = candidates(
            &items,
            &ItemMemory::new(),
            &graph(),
            &view(BLASTER, 100, 0, vec![]),
            0.0,
        );
        let w = c.weapon.expect("rocket pad offered");
        assert_eq!((w.item, w.cost), (1, 768.0));
        assert_eq!(c.facts().weapon, Some(DistBand::Mid));
        assert_eq!((c.health, c.armor), (None, None));
    }

    #[test]
    fn jev_pads_nearest_by_travel_cost_wins() {
        let items = [
            pad(EntityClass::ItemHealth, None, 3),
            pad(EntityClass::ItemHealth, None, 1),
            pad(EntityClass::ItemArmor, None, 2),
        ];
        let c = candidates(
            &items,
            &ItemMemory::new(),
            &graph(),
            &view(BLASTER, 40, 0, vec![]),
            0.0,
        );
        assert_eq!(c.health.map(|p| p.item), Some(1));
        assert_eq!(c.armor.map(|p| p.item), Some(2));
        assert_eq!(
            c.facts(),
            PadFacts {
                weapon: None,
                health: Some(DistBand::Near),
                armor: Some(DistBand::Mid),
            }
        );
    }

    #[test]
    fn jev_pads_railgun_holder_gets_no_weaker_weapon_and_full_bot_no_health() {
        let items = [
            pad(EntityClass::ItemWeapon, Some(Weapon::Shotgun), 1),
            pad(EntityClass::ItemWeapon, Some(Weapon::RocketLauncher), 2),
            pad(EntityClass::ItemHealth, None, 1),
            pad(EntityClass::ItemArmor, None, 1),
        ];
        let c = candidates(
            &items,
            &ItemMemory::new(),
            &graph(),
            &view(RAIL, 100, 100, vec![]),
            0.0,
        );
        assert!(
            c.is_empty(),
            "rail (95) beats shotgun and rockets (≤90); full hp + armor: {c:?}"
        );
    }

    #[test]
    fn jev_rule_pick_follows_the_table() {
        let p = |item| {
            Some(Pad {
                item,
                origin: Vec3::ZERO,
                cost: 1.0,
            })
        };
        let all = PadCandidates {
            weapon: p(0),
            health: p(1),
            armor: p(2),
        };
        let pick = |c: &PadCandidates, held, hp, ar| c.rule_pick(held, hp, ar).map(|p| p.item);
        assert_eq!(
            pick(&all, Some(Weapon::Blaster), 20, 0),
            Some(0),
            "weak weapon first"
        );
        assert_eq!(
            pick(&all, Some(Weapon::Railgun), 20, 0),
            Some(1),
            "then low health"
        );
        assert_eq!(
            pick(&all, Some(Weapon::Railgun), 90, 10),
            Some(2),
            "then low armor"
        );
        assert_eq!(
            pick(&all, Some(Weapon::Railgun), 90, 60),
            None,
            "else carry_on"
        );
        assert_eq!(pick(&all, None, 90, 60), None, "unknown held is not weak");
        let no_weapon = PadCandidates {
            weapon: None,
            ..all
        };
        assert_eq!(pick(&no_weapon, Some(Weapon::Shotgun), 40, 0), Some(1));
    }

    #[test]
    fn jev_pads_pad_seen_empty_is_skipped_until_it_respawns() {
        // The bot stands 50 u from a health pad (inside ItemMemory's trust range) and the pad is
        // not transmitted: taken. A farther health pad is offered instead until 30 s pass.
        let items = [
            MapItem {
                origin: Vec3::new(50.0, 0.0, 0.0),
                ..pad(EntityClass::ItemHealth, None, 0)
            },
            pad(EntityClass::ItemHealth, None, 3),
        ];
        let v = view(BLASTER, 40, 0, vec![]);
        let mut mem = ItemMemory::new();
        mem.observe(&items, &v, 10.0);
        let g = graph();
        assert_eq!(
            candidates(&items, &mem, &g, &v, 10.0)
                .health
                .map(|p| p.item),
            Some(1)
        );
        assert_eq!(
            candidates(&items, &mem, &g, &v, 41.0)
                .health
                .map(|p| p.item),
            Some(0)
        );
        // Seen stocked again: available at once.
        let stocked = vec![EntityState {
            number: 20,
            origin: [50.0, 0.0, 0.0],
            modelindex: 2,
            ..Default::default()
        }];
        let mut mem = ItemMemory::new();
        mem.observe(&items, &v, 10.0);
        mem.observe(&items, &view(BLASTER, 40, 0, stocked), 11.0);
        assert_eq!(
            candidates(&items, &mem, &g, &v, 11.0)
                .health
                .map(|p| p.item),
            Some(0)
        );
    }
}
