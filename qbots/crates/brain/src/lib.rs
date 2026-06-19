//! # brain — bot AI
//!
//! Turns per-frame perception into intent: navigation over the `world` nav graph, combat
//! (aim/lead/weapon-select), and a behavior FSM. Builds on `client` + `world`.
//! Filled in by Plan 06.
//!
//! See `AGENTS.md` and `context/plans/06_brain.md`.

pub mod aim;
pub mod brain;
pub mod brains;
pub mod combat;
pub mod danger;
pub mod fsm;
pub mod heatmap;
pub mod hybrid;
pub mod items;
pub mod los;
pub mod move_ctrl;
pub mod nav;
pub mod nav_mode;
pub mod navmesh_driver;
pub mod observed;
pub mod perception;
pub mod pursuit;
pub mod recorder;
pub mod recover;
pub mod skill;
pub mod steer;
pub mod weapons;

pub use brain::{Brain, BrainConfig, BrainOutput};
// The brain plugin contract + bundled I/O (Plan 23). The `Brain` *trait* lives at
// `brains::core::Brain`; the root `Brain` export flips from the concrete struct to the trait in
// Plan 23 T5 once the binary drives a `Box<dyn Brain>`. `BrainContext`/`BrainMap` are new here.
pub use brains::core::{BrainContext, BrainMap};
pub use brains::{build_brain, BrainKind};
pub use combat::{CombatDecision, CombatDriver};
pub use danger::{DangerDriver, DodgeAction};
pub use heatmap::Heatmap;
pub use move_ctrl::{MovementController, MovementIntent};
pub use nav::{NavGoal, NavigationDriver};
pub use nav_mode::Navigator;
pub use navmesh_driver::NavmeshDriver;
pub use observed::{parse_obituary, HeatmapObserver, HeatmapSnapshot, Obituary};
pub use perception::{EntityClass, PerceivedEntity, SelfState, Worldview};
pub use recorder::{
    CmWallProbe, FrameRecord, MovementRecorder, RunSummary, Sample, WallBump, WallProbe,
};
pub use recover::{Recovery, RecoveryAction, StuckDetector, StuckLevel};
pub use skill::{BotSkill, Personality, SkillLevel, SkillRegistry};
pub use weapons::Weapon;

#[cfg(test)]
mod tests {
    #[test]
    fn sanity() {
        assert_eq!(2 + 2, 4);
    }
}
