//! # brain — bot AI
//!
//! Turns per-frame perception into intent: navigation over the `world` nav graph, combat
//! (aim/lead/weapon-select), and a behavior FSM. Builds on `client` + `world`.
//! Filled in by Plan 06.
//!
//! See `AGENTS.md` and `context/plans/06_brain.md`.

pub mod aim;
pub mod combat;
pub mod danger;
pub mod fsm;
pub mod heatmap;
pub mod items;
pub mod los;
pub mod move_ctrl;
pub mod nav;
pub mod observed;
pub mod perception;
pub mod recorder;
pub mod skill;
pub mod steer;
pub mod weapons;

pub use combat::{CombatDecision, CombatDriver};
pub use danger::{DangerDriver, DodgeAction};
pub use heatmap::Heatmap;
pub use move_ctrl::{MovementController, MovementIntent};
pub use nav::{NavGoal, NavigationDriver, StuckAction};
pub use observed::{parse_obituary, HeatmapObserver, HeatmapSnapshot, Obituary};
pub use perception::{EntityClass, PerceivedEntity, SelfState, Worldview};
pub use recorder::{
    CmWallProbe, FrameRecord, MovementRecorder, RunSummary, Sample, WallBump, WallProbe,
};
pub use skill::{BotSkill, Personality, SkillLevel, SkillRegistry};
pub use weapons::Weapon;

#[cfg(test)]
mod tests {
    #[test]
    fn sanity() {
        assert_eq!(2 + 2, 4);
    }
}
