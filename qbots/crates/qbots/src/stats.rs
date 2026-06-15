//! Fleet-wide kill/death tally — observability for the supervisor (Plan 09).
//!
//! This is shared mutable *telemetry*, not shared mutable *world* state: a
//! counter never lets one bot perceive another, so it doesn't trip the
//! AGENTS.md §Concurrency rule (which is about world/perception state). Each bot
//! updates only its own entry, so there's no cross-bot lock contention; the
//! supervisor reads totals for the periodic heartbeat and the final shutdown
//! report.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// One bot's running kill/death count.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BotTally {
    pub kills: u64,
    pub deaths: u64,
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

    /// Fleet totals across all registered bots.
    pub fn totals(&self) -> BotTally {
        self.bots
            .lock()
            .unwrap()
            .values()
            .fold(BotTally::default(), |acc, t| BotTally {
                kills: acc.kills + t.kills,
                deaths: acc.deaths + t.deaths,
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
                deaths: 1
            }
        );
        assert_eq!(
            snap["b"],
            BotTally {
                kills: 1,
                deaths: 0
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
                deaths: 3
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
    fn register_is_idempotent_and_preserves_tally() {
        let s = FleetStats::new();
        s.record_kill("a");
        s.register("a"); // must not reset
        s.register("a");
        let snap: HashMap<String, BotTally> = s.snapshot().into_iter().collect();
        assert_eq!(snap["a"].kills, 1);
    }
}
