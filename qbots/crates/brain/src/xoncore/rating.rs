//! Goal-rating math — havocbot's single smooth objective (Plan 59 T2).
//!
//! Xonotic rates every candidate goal (items, enemies, wander waypoints) with ONE formula:
//! `value * rangebias / (rangebias + cost)` where both `rangebias` and `cost` are **travel
//! time in seconds** (`navigation.qc:1418`, `waypoint_getlinearcost` `waypoints.qc:1010`).
//! Item values come from the `*_pickupevalfunc` family (`server/items/items.qc:885-979`),
//! enemy values from `havocbot_goalrating_enemyplayers` (`roles.qc:176-215`), and the wander
//! fallback from `havocbot_goalrating_waypoints` (`roles.qc:16-43`). Highest rating wins.
//!
//! Values are on a 0–10000-ish scale (see [`crate::xonchar::weapon_base_value`]); the role's
//! `ratingscale * 0.0001` normalization (`roles.qc:185`) is pre-folded, so the constants here
//! compare directly: an even-matched enemy rates [`BOT_RATING_ENEMY`] = 2500.

use super::Lcg;
use crate::move_ctrl::MAX_SPEED;

/// The vendor's default `rangebias` for every DM rating call (`roles.qc:37,213`), in world
/// units (converted to travel-time inside [`route_rating`]).
pub const RANGEBIAS_QU: f32 = 2000.0;

/// `BOT_RATING_ENEMY` (`havocbot/roles.qh:3`) — the enemy-player base value.
pub const BOT_RATING_ENEMY: f32 = 2500.0;

/// Enemies closer than this are not *rated* as goals (`roles.qc:189`) — you fight them, you
/// don't route to them.
pub const ENEMY_MIN_RATE_DIST: f32 = 100.0;

/// `waypoint_getlinearcost` (`waypoints.qc:1010`): distance → travel-time seconds at Q2 run
/// speed. (The vendor's bunnyhop 1.25× variant is deliberately not ported — no Q2 bhop.)
pub fn linear_cost(dist_qu: f32) -> f32 {
    dist_qu / MAX_SPEED
}

/// **THE formula** (`navigation.qc:1416-1418`): smooth value/distance tradeoff. `cost_s` is
/// the travel time to the candidate (from a Dijkstra flood + the final leg);
/// `rangebias_qu` ≈ [`RANGEBIAS_QU`]. At `cost = 0` → `value`; at `cost = rangebias` →
/// `value/2`; monotonically decreasing.
pub fn route_rating(value: f32, rangebias_qu: f32, cost_s: f32) -> f32 {
    let rb = linear_cost(rangebias_qu);
    value * rb / (rb + cost_s.max(0.0))
}

/// `weapon_pickupevalfunc` (`items.qc:887-907`), the not-owned branch: base value discounted
/// by how rich our arsenal already is — `base * (1 − 0.5 * bound(0, arsenal/20000, 1))`.
/// `arsenal_value` = Σ [`crate::xonchar::weapon_base_value`] over weapons we own. For an
/// **owned** weapon the vendor re-rates it as ammo — use [`ammo_value`] with the weapon's
/// base as `weapon_bonus`.
pub fn weapon_value(base: f32, arsenal_value: f32) -> f32 {
    base * (1.0 - (arsenal_value / 20_000.0).clamp(0.0, 1.0) * 0.5)
}

/// `ammo_pickupevalfunc` (`items.qc:909-955`): worth more the emptier we are —
/// `ammo_base * min(gives / max(0.5, have), 2) + weapon_bonus * 0.1`. `have` is our current
/// count of that ammo type; `gives` what the pickup grants; `weapon_bonus` is the weapon's
/// base value when the pickup IS a weapon we already own (`items.qc:952-953`), else 0.
/// (`noammorating = 0.5`, `items.qc:946`.)
pub fn ammo_value(ammo_base: f32, gives: f32, have: f32, weapon_bonus: f32) -> f32 {
    let c = gives / have.max(0.5);
    ammo_base * c.min(2.0) + weapon_bonus * 0.1
}

/// `healtharmor_pickupevalfunc` (`items.qc:957-979`): `base * min(2, c)` where `c` compares
/// the pickup amount (× the clustered-group multiplier `min(4, group_count)`,
/// `items.qc:965-969`) to what we have — health: `amount / max(1, health)`; armor:
/// `amount / max(1, armor*2/3 + health*1/3)`. Returns 0 when already at the item's cap
/// (caller checks the cap — Q2 caps differ per item).
pub fn health_armor_value(
    base: f32,
    amount: f32,
    group_count: u32,
    my_health: f32,
    my_armor: f32,
    is_armor: bool,
) -> f32 {
    let amt = amount * (group_count.clamp(1, 4) as f32);
    let denom = if is_armor {
        (my_armor * 2.0 / 3.0 + my_health / 3.0).max(1.0)
    } else {
        my_health.max(1.0)
    };
    base * (amt / denom).min(2.0)
}

/// `havocbot_goalrating_enemyplayers` (`roles.qc:201-213`): the enemy-as-goal value.
/// `t = bound(0, 1 + (my_hp+armor − their_hp+armor)/150, 3)` then
/// `t += max(0, 8 − skill) * 0.05` ("less skilled bots attack more mindlessly"), value =
/// `t * BOT_RATING_ENEMY`. Enemy health isn't on the Q2 wire — callers pass an estimate
/// (default 100). The powerup adjustments (`roles.qc:203-209`) are dropped (not wire-visible).
pub fn enemy_rating(my_hp_plus_armor: f32, their_hp_plus_armor_est: f32, skill: f32) -> f32 {
    let mut t = (1.0 + (my_hp_plus_armor - their_hp_plus_armor_est) / 150.0).clamp(0.0, 3.0);
    t += (8.0 - skill).max(0.0) * 0.05;
    t * BOT_RATING_ENEMY
}

/// `havocbot_goalrating_waypoints` (`roles.qc:16-43`), the per-waypoint factor: `0.1` if the
/// candidate is near either of the last two visited wander goals (× `range*1.5` = 750 qu),
/// else `0.5 + random()*0.5`. The vendor runs this at `ratingscale = 1` and ONLY when nothing
/// else rated — a pure tie-breaking fallback, values ≪ any item/enemy.
pub fn wander_value(rng: &mut Lcg, near_prev0: bool, near_prev1: bool) -> f32 {
    if near_prev0 || near_prev1 {
        0.1
    } else {
        0.5 + rng.next() * 0.5
    }
}

/// The vendor's "near a previous wander goal" radius (`range * 1.5`, `roles.qc:31`), qu.
pub const WANDER_PREV_RADIUS: f32 = 750.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_rating_pins() {
        // cost 0 → full value; cost == rangebias-time → exactly half; monotone decreasing.
        assert_eq!(route_rating(8000.0, RANGEBIAS_QU, 0.0), 8000.0);
        let half = route_rating(8000.0, RANGEBIAS_QU, linear_cost(RANGEBIAS_QU));
        assert!((half - 4000.0).abs() < 1e-3, "got {half}");
        let near = route_rating(8000.0, RANGEBIAS_QU, 1.0);
        let far = route_rating(8000.0, RANGEBIAS_QU, 10.0);
        assert!(near > far && far > 0.0);
        // A close mid item can out-rate a far great item — the tradeoff the formula exists for.
        let close_shard = route_rating(3000.0, RANGEBIAS_QU, 0.5);
        let far_rail = route_rating(8000.0, RANGEBIAS_QU, 20.0);
        assert!(close_shard > far_rail);
    }

    #[test]
    fn weapon_value_arsenal_discount() {
        assert_eq!(weapon_value(8000.0, 0.0), 8000.0);
        // Full 20000+ arsenal → exactly half value (items.qc:904).
        assert_eq!(weapon_value(8000.0, 20_000.0), 4000.0);
        assert_eq!(weapon_value(8000.0, 40_000.0), 4000.0); // bound caps the discount
        assert_eq!(weapon_value(8000.0, 10_000.0), 6000.0);
    }

    #[test]
    fn ammo_value_rises_when_empty() {
        // Empty (have=0): c = gives/0.5, capped at 2 → 2× base (items.qc:946-951).
        assert_eq!(ammo_value(1000.0, 10.0, 0.0, 0.0), 2000.0);
        // Rich: 10 rockets when holding 50 → c = 0.2.
        assert_eq!(ammo_value(1000.0, 10.0, 50.0, 0.0), 200.0);
        // Owned-weapon pickup adds base*0.1 (items.qc:952-953).
        assert_eq!(ammo_value(1000.0, 10.0, 50.0, 8000.0), 1000.0);
    }

    #[test]
    fn health_armor_value_pins() {
        // 25-health at 100 hp → 0.25×; at 12 hp → capped 2×.
        assert_eq!(
            health_armor_value(5000.0, 25.0, 1, 100.0, 0.0, false),
            1250.0
        );
        assert_eq!(
            health_armor_value(5000.0, 25.0, 1, 12.0, 0.0, false),
            10_000.0
        );
        // Clustered shards: group of 6 caps at ×4 (items.qc:967).
        let single = health_armor_value(5000.0, 5.0, 1, 100.0, 100.0, true);
        let group = health_armor_value(5000.0, 5.0, 6, 100.0, 100.0, true);
        assert!((group / single - 4.0).abs() < 1e-3);
        // Armor denominator mixes armor 2/3 + health 1/3 (items.qc:972).
        let vs_armored = health_armor_value(5000.0, 50.0, 1, 100.0, 100.0, true);
        let vs_naked = health_armor_value(5000.0, 50.0, 1, 100.0, 0.0, true);
        assert!(vs_naked > vs_armored);
    }

    #[test]
    fn enemy_rating_pins() {
        // Even match at skill ≥ 8 → t = 1 → 2500.
        assert_eq!(enemy_rating(100.0, 100.0, 8.0), 2500.0);
        // +150 advantage → t = 2 → 5000.
        assert_eq!(enemy_rating(250.0, 100.0, 8.0), 5000.0);
        // Hopeless (−150) → t = 0 + mindless term only.
        assert_eq!(enemy_rating(0.0, 150.0, 8.0), 0.0);
        // Skill 0 adds the full mindless bonus: +0.4 → 1.4 × 2500.
        assert!((enemy_rating(100.0, 100.0, 0.0) - 3500.0).abs() < 1e-3);
    }

    #[test]
    fn wander_prefers_unvisited() {
        let mut rng = Lcg::new(1);
        assert_eq!(wander_value(&mut rng, true, false), 0.1);
        assert_eq!(wander_value(&mut rng, false, true), 0.1);
        for _ in 0..100 {
            let v = wander_value(&mut rng, false, false);
            assert!((0.5..1.0).contains(&v));
        }
    }
}
