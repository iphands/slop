//! # brain::brains::jev::pads — out-of-sight pickup candidates (Plan 74 T3)
//!
//! q3 only heads for pickups it can see (`Q3Brain::roam_goal` over `view.items()`) and drops the
//! BSP item table. This module is what the jev policy offers the model instead: per class, the
//! pad with the **lowest travel cost** from where the bot stands that is
//! - **worth having** — a *strong* weapon (never blaster / shotgun / machinegun — Plan 74 B3)
//!   stronger than the held one (`Weapon::power_tier`; an unrecognised held weapon counts as
//!   tier 0), health below [`FULL_HEALTH`], armor below [`FULL_ARMOR`];
//! - **ranked** by travel cost — for weapons scaled by `100 / power_tier`, so power is weighed
//!   against distance;
//! - **not known empty** — `ItemMemory::available` (Q2 respawn times; the arena server runs
//!   without `DF_WEAPONS_STAY`, so a taken weapon pad is empty for 30 s);
//! - **reachable** — finite cost in one `NavGraph::flood_costs_weighted` pass from the bot's
//!   nearest node (one Dijkstra prices every pad; called per ask, never per tick), priced with
//!   the `risk` overlay (Plan 74 B7: the rim surcharge, so a pad reached along lava walkways
//!   costs more — and reads farther to the model — than one reached over safe floor).
//!
//! Ammo pads are never weapon candidates (`MapItem::weapon` is `None` for them — T2).

use glam::Vec3;
use world::NavGraph;

use crate::brains::core::MapItem;
use crate::items::ItemMemory;
use crate::jev::question::{Offered, CARRY_ON, SEEK_ARMOR, SEEK_HEALTH, SEEK_WEAPON};
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
        match self.rule_label(held, hp, armor) {
            SEEK_WEAPON => self.weapon,
            SEEK_HEALTH => self.health,
            SEEK_ARMOR => self.armor,
            _ => None,
        }
    }

    /// The option label [`Self::rule_pick`] stands for (`carry_on` when it picks nothing).
    pub fn rule_label(&self, held: Option<Weapon>, hp: i32, armor: i32) -> &'static str {
        let weak = matches!(
            held,
            Some(Weapon::Blaster | Weapon::Shotgun | Weapon::Machinegun)
        );
        [
            (weak && self.weapon.is_some(), SEEK_WEAPON),
            (hp < 50 && self.health.is_some(), SEEK_HEALTH),
            (armor < 50 && self.armor.is_some(), SEEK_ARMOR),
        ]
        .into_iter()
        .find_map(|(want, label)| want.then_some(label))
        .unwrap_or(CARRY_ON)
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
        EntityClass::ItemWeapon
            if item
                .weapon
                .is_some_and(|w| !is_weak(w) && w.power_tier() > held_tier) =>
        {
            Some(&mut out.weapon)
        }
        EntityClass::ItemHealth if hp < FULL_HEALTH => Some(&mut out.health),
        EntityClass::ItemArmor if armor < FULL_ARMOR => Some(&mut out.armor),
        _ => None,
    }
}

/// Extra pricing cost for a node that starts or ends a ride (mover) edge — Plan 74 B8.
pub const MOVER_NODE_COST: f32 = 512.0;

/// Per-node pricing surcharge for pad routes that ride a mover: [`MOVER_NODE_COST`] on both ends
/// of every ride edge. Plan 74 B3–B6: the pickup arms were crushed ("squish") about twice as
/// often as q3 (jev 32, jvr 37, q3 20), clustered on q2dm3's loop-train/elevator route to the
/// second railgun — trips to that pad ride the train.
pub fn mover_node_costs(graph: &NavGraph) -> Vec<f32> {
    let mut c = vec![0.0; graph.node_count()];
    for (a, cost) in c.iter_mut().enumerate() {
        if graph
            .neighbors(a)
            .iter()
            .any(|&(b, _)| graph.is_ride_edge(a, b) || graph.is_ride_edge(b, a))
        {
            *cost = MOVER_NODE_COST;
        }
    }
    c
}

/// Weak weapons — the decision state's own set. A weak pad is never a weapon objective
/// (Plan 74 B3): walking to a shotgun leaves the bot as under-armed as the blaster did.
fn is_weak(w: Weapon) -> bool {
    matches!(w, Weapon::Blaster | Weapon::Shotgun | Weapon::Machinegun)
}

/// How a candidate is ranked within its class (lower is better): travel cost, and for weapons
/// travel cost scaled by `100 / power_tier`, so a rocket launcher (90) a little farther beats a
/// nearer super shotgun (50) — but not one twice as far.
fn rank(item: &MapItem, cost: f32) -> f32 {
    match item.weapon {
        Some(w) => cost * 100.0 / f32::from(w.power_tier().max(1)),
        None => cost,
    }
}

/// Where to retreat to (Plan 74 B2): the cheapest reachable, not-known-empty health pad — else
/// armor pad — that lies **away** from `enemy` (farther from it than we are, by a margin), so
/// the retreat ends on a pickup instead of q3's blind 256 u step back. `None` ⇒ q3's own.
pub fn retreat_pad(
    items: &[MapItem],
    memory: &ItemMemory,
    graph: &NavGraph,
    risk: &[f32],
    view: &Worldview,
    now: f32,
    enemy: Vec3,
) -> Option<Pad> {
    let s = view.self_state();
    let from = graph.nearest(&s.origin.into())?;
    let costs = graph.flood_costs_weighted(from, risk);
    let mine = (s.origin - enemy).length();
    let mut best: [Option<Pad>; 2] = [None, None];
    for (i, item) in items.iter().enumerate() {
        let slot = match item.class {
            EntityClass::ItemHealth => 0,
            EntityClass::ItemArmor => 1,
            _ => continue,
        };
        if (item.origin - enemy).length() < mine + RETREAT_MARGIN
            || !memory.available(i, item.class, now)
        {
            continue;
        }
        let Some(cost) = item
            .nav_node
            .and_then(|n| costs.get(n).copied())
            .filter(|c| c.is_finite() && *c <= RETREAT_MAX_COST)
        else {
            continue;
        };
        if best[slot].is_none_or(|p| cost < p.cost) {
            best[slot] = Some(Pad {
                item: i,
                origin: item.origin,
                cost,
            });
        }
    }
    best[0].or(best[1])
}

/// A retreat pad must be this much farther from the enemy than we are.
const RETREAT_MARGIN: f32 = 128.0;
/// Retreat pads farther than this (travel cost) are not a retreat, they are a journey.
const RETREAT_MAX_COST: f32 = 1536.0;

/// The best pad per class for the bot in `view` (see the module doc). Empty before the bot can
/// be placed on the graph.
pub fn candidates(
    items: &[MapItem],
    memory: &ItemMemory,
    graph: &NavGraph,
    risk: &[f32],
    view: &Worldview,
    now: f32,
) -> PadCandidates {
    let s = view.self_state();
    let mut out = PadCandidates::default();
    let Some(from) = graph.nearest(&s.origin.into()) else {
        return out;
    };
    let costs = graph.flood_costs_weighted(from, risk);
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
        if best.is_none_or(|p| rank(item, cost) < rank(&items[p.item], p.cost)) {
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
    fn jev_pads_mover_surcharge_marks_both_ends_of_a_ride() {
        let mut g = graph();
        let info = world::RideInfo {
            board: [256.0, 0.0, 0.0],
            far: [512.0, 0.0, 0.0],
            dismount: [512.0, 0.0, 0.0],
            model_index: 7,
            vertical: false,
            board_ent: [256.0, 0.0, 0.0],
            far_ent: [512.0, 0.0, 0.0],
            ladder: false,
            stand_offset: [0.0; 3],
        };
        g.add_ride_edge(1, 2, 300.0, info);
        let c = mover_node_costs(&g);
        assert_eq!(c, vec![0.0, MOVER_NODE_COST, MOVER_NODE_COST, 0.0, 0.0]);
    }

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
            &[],
            &view(BLASTER, 100, 0, vec![]),
            0.0,
        );
        let w = c.weapon.expect("rocket pad offered");
        assert_eq!((w.item, w.cost), (1, 768.0));
        assert_eq!(c.facts().weapon, Some(DistBand::Mid));
        assert_eq!((c.health, c.armor), (None, None));
    }

    #[test]
    fn jev_pads_weapon_pads_skip_weak_guns_and_weigh_power_against_distance() {
        let blaster = view(BLASTER, 100, 100, vec![]);
        let g = graph();
        let pick = |items: &[MapItem]| {
            candidates(items, &ItemMemory::new(), &g, &[], &blaster, 0.0)
                .weapon
                .map(|p| p.item)
        };
        // A shotgun next door is no objective; the rocket launcher three nodes away is.
        let a = [
            pad(EntityClass::ItemWeapon, Some(Weapon::Shotgun), 1),
            pad(EntityClass::ItemWeapon, Some(Weapon::RocketLauncher), 3),
        ];
        assert_eq!(pick(&a), Some(1));
        // A super shotgun at 256 (rank 512) beats a rocket launcher at 768 (rank 853)…
        let b = [
            pad(EntityClass::ItemWeapon, Some(Weapon::SuperShotgun), 1),
            pad(EntityClass::ItemWeapon, Some(Weapon::RocketLauncher), 3),
        ];
        assert_eq!(pick(&b), Some(0));
        // …but at 512 (rank 1024) the rocket launcher wins.
        let c = [
            pad(EntityClass::ItemWeapon, Some(Weapon::SuperShotgun), 2),
            pad(EntityClass::ItemWeapon, Some(Weapon::RocketLauncher), 3),
        ];
        assert_eq!(pick(&c), Some(1));
        assert_eq!(
            pick(&a[..1]),
            None,
            "only a shotgun: no weapon objective at all"
        );
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
            &[],
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
            &[],
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
            candidates(&items, &mem, &g, &[], &v, 10.0)
                .health
                .map(|p| p.item),
            Some(1)
        );
        assert_eq!(
            candidates(&items, &mem, &g, &[], &v, 41.0)
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
            candidates(&items, &mem, &g, &[], &v, 11.0)
                .health
                .map(|p| p.item),
            Some(0)
        );
    }
}
