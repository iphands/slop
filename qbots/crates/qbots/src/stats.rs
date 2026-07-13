//! Fleet-wide kill/death tally — observability for the supervisor (Plan 09).
//!
//! This is shared mutable *telemetry*, not shared mutable *world* state: a
//! counter never lets one bot perceive another, so it doesn't trip the
//! AGENTS.md §Concurrency rule (which is about world/perception state). Each bot
//! updates only its own entry, so there's no cross-bot lock contention; the
//! supervisor reads totals for the periodic heartbeat and the final shutdown
//! report.

use brain::EnvDeath;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// One bot's running kill/death count.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BotTally {
    pub kills: u64,
    pub deaths: u64,
    /// Environmental suicides (lava/slime/drown/squish/…), indexed by
    /// [`EnvDeath::index`]. These deaths are also counted in `deaths`.
    pub env_suicides: [u64; EnvDeath::ALL.len()],
    /// Health *points* gained from pickups (stimpack/small/large/mega/adrenaline) —
    /// own-playerstate stat increases while alive, so respawn resets don't count
    /// (Plan 67). Amounts, not item counts.
    pub health_picked: u64,
    /// Armor *points* gained from pickups (shard/jacket/combat/body), same
    /// detection rule as `health_picked`.
    pub armor_picked: u64,
}

impl BotTally {
    /// Total environmental suicides across all causes.
    pub fn env_total(&self) -> u64 {
        self.env_suicides.iter().sum()
    }

    /// Compact per-cause breakdown for reports, e.g. `lava:3 drown:1`.
    /// Empty string when no environmental suicides occurred.
    pub fn env_breakdown(&self) -> String {
        EnvDeath::ALL
            .into_iter()
            .filter(|k| self.env_suicides[k.index()] > 0)
            .map(|k| format!("{}:{}", k.name(), self.env_suicides[k.index()]))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Shared, clone-cheap fleet tally keyed by bot name. Cheap to clone (one `Arc`);
/// pass the same clone to every bot task + the supervisor.
#[derive(Clone, Default)]
pub struct FleetStats {
    bots: Arc<Mutex<HashMap<String, BotTally>>>,
}

impl FleetStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a bot with a zero tally so bots that never frag/die still appear
    /// in the final report. Idempotent.
    pub fn register(&self, name: &str) {
        self.bots
            .lock()
            .unwrap()
            .entry(name.to_string())
            .or_default();
    }

    /// Record one kill attributed to `name`.
    pub fn record_kill(&self, name: &str) {
        self.bots
            .lock()
            .unwrap()
            .entry(name.to_string())
            .or_default()
            .kills += 1;
    }

    /// Record one death attributed to `name`.
    pub fn record_death(&self, name: &str) {
        self.bots
            .lock()
            .unwrap()
            .entry(name.to_string())
            .or_default()
            .deaths += 1;
    }

    /// Record one environmental suicide (lava/slime/…) attributed to `name`.
    /// Does **not** bump `deaths` — the health-based death detector already
    /// counts every death; this only classifies the cause.
    pub fn record_env_suicide(&self, name: &str, kind: EnvDeath) {
        self.bots
            .lock()
            .unwrap()
            .entry(name.to_string())
            .or_default()
            .env_suicides[kind.index()] += 1;
    }

    /// Record `amount` health points gained from a pickup by `name` (Plan 67).
    pub fn record_health_pickup(&self, name: &str, amount: u64) {
        self.bots
            .lock()
            .unwrap()
            .entry(name.to_string())
            .or_default()
            .health_picked += amount;
    }

    /// Record `amount` armor points gained from a pickup by `name` (Plan 67).
    pub fn record_armor_pickup(&self, name: &str, amount: u64) {
        self.bots
            .lock()
            .unwrap()
            .entry(name.to_string())
            .or_default()
            .armor_picked += amount;
    }

    /// Fleet totals across all registered bots.
    pub fn totals(&self) -> BotTally {
        self.bots
            .lock()
            .unwrap()
            .values()
            .fold(BotTally::default(), |acc, t| {
                let mut env = acc.env_suicides;
                for (a, b) in env.iter_mut().zip(t.env_suicides) {
                    *a += b;
                }
                BotTally {
                    kills: acc.kills + t.kills,
                    deaths: acc.deaths + t.deaths,
                    env_suicides: env,
                    health_picked: acc.health_picked + t.health_picked,
                    armor_picked: acc.armor_picked + t.armor_picked,
                }
            })
    }

    /// Per-bot tallies, sorted by kills descending then name (frag leaders first).
    pub fn snapshot(&self) -> Vec<(String, BotTally)> {
        let mut v: Vec<(String, BotTally)> = self
            .bots
            .lock()
            .unwrap()
            .iter()
            .map(|(n, t)| (n.clone(), *t))
            .collect();
        v.sort_by(|a, b| b.1.kills.cmp(&a.1.kills).then_with(|| a.0.cmp(&b.0)));
        v
    }

    /// Number of registered bots.
    pub fn bot_count(&self) -> usize {
        self.bots.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_accumulates_per_bot() {
        let s = FleetStats::new();
        s.record_kill("a");
        s.record_kill("a");
        s.record_death("a");
        s.record_kill("b");
        let snap: HashMap<String, BotTally> = s.snapshot().into_iter().collect();
        assert_eq!(
            snap["a"],
            BotTally {
                kills: 2,
                deaths: 1,
                ..Default::default()
            }
        );
        assert_eq!(
            snap["b"],
            BotTally {
                kills: 1,
                deaths: 0,
                ..Default::default()
            }
        );
    }

    #[test]
    fn totals_sum_across_bots() {
        let s = FleetStats::new();
        s.record_kill("a");
        s.record_kill("b");
        s.record_death("a");
        s.record_death("b");
        s.record_death("b");
        let t = s.totals();
        assert_eq!(
            t,
            BotTally {
                kills: 2,
                deaths: 3,
                ..Default::default()
            }
        );
    }

    #[test]
    fn snapshot_sorted_by_kills_desc() {
        let s = FleetStats::new();
        s.record_kill("lo");
        s.record_kill("hi");
        s.record_kill("hi");
        let snap = s.snapshot();
        assert_eq!(snap[0].0, "hi");
        assert_eq!(snap[0].1.kills, 2);
        assert_eq!(snap[1].0, "lo");
    }

    #[test]
    fn register_makes_zero_entry() {
        let s = FleetStats::new();
        s.register("idle");
        assert_eq!(s.bot_count(), 1);
        assert_eq!(s.totals(), BotTally::default());
        let snap = s.snapshot();
        assert_eq!(snap[0], ("idle".to_string(), BotTally::default()));
    }

    #[test]
    fn env_suicides_tally_per_cause_and_sum() {
        let s = FleetStats::new();
        s.record_env_suicide("a", EnvDeath::Lava);
        s.record_env_suicide("a", EnvDeath::Lava);
        s.record_env_suicide("a", EnvDeath::Drown);
        s.record_env_suicide("b", EnvDeath::Squish);
        let snap: HashMap<String, BotTally> = s.snapshot().into_iter().collect();
        assert_eq!(snap["a"].env_suicides[EnvDeath::Lava.index()], 2);
        assert_eq!(snap["a"].env_total(), 3);
        assert_eq!(snap["a"].env_breakdown(), "lava:2 drown:1");
        assert_eq!(snap["b"].env_breakdown(), "squish:1");
        assert_eq!(s.totals().env_total(), 4);
        // Cause classification never bumps the death counter itself.
        assert_eq!(s.totals().deaths, 0);
    }

    #[test]
    fn pickups_accumulate_and_sum_in_totals() {
        let s = FleetStats::new();
        s.record_health_pickup("a", 25);
        s.record_health_pickup("a", 100); // megahealth
        s.record_armor_pickup("a", 2); // shard
        s.record_health_pickup("b", 10);
        s.record_armor_pickup("b", 50);
        let snap: HashMap<String, BotTally> = s.snapshot().into_iter().collect();
        assert_eq!(snap["a"].health_picked, 125);
        assert_eq!(snap["a"].armor_picked, 2);
        assert_eq!(snap["b"].health_picked, 10);
        assert_eq!(snap["b"].armor_picked, 50);
        let t = s.totals();
        assert_eq!((t.health_picked, t.armor_picked), (135, 52));
        // Pickups never touch the frag counters.
        assert_eq!((t.kills, t.deaths), (0, 0));
    }

    #[test]
    fn register_is_idempotent_and_preserves_tally() {
        let s = FleetStats::new();
        s.record_kill("a");
        s.register("a"); // must not reset
        s.register("a");
        let snap: HashMap<String, BotTally> = s.snapshot().into_iter().collect();
        assert_eq!(snap["a"].kills, 1);
    }
}
