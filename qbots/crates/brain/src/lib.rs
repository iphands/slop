//! # brain — bot AI
//!
//! Turns per-frame perception into intent: navigation over the `world` nav graph, combat
//! (aim/lead/weapon-select), and a behavior FSM. Builds on `client` + `world`.
//! Filled in by Plan 06.
//!
//! See `AGENTS.md` and `context/plans/06_brain.md`.

pub mod aim;
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
pub mod persona;
pub mod pursuit;
pub mod q3char;
pub mod recorder;
pub mod recover;
pub mod ride;
pub mod skill;
pub mod steer;
pub mod traverse;
pub mod water;
pub mod weapons;

// The brain plugin contract + bundled I/O (Plan 23). The root `Brain` is the **trait** (the
// binary drives a `Box<dyn Brain>` via `build_brain`); the concrete `main` impl is `MainBrain`
// (`brains::main`), reached through the factory.
pub use brains::core::{Brain, BrainConfig, BrainContext, BrainMap, BrainOutput};
pub use brains::main::MainBrain;
pub use brains::runtester::RunTesterBrain;
pub use brains::sentry::SentryBrain;
pub use brains::{brain_tag, build_brain, BrainKind};
pub use combat::{CombatDecision, CombatDriver};
pub use danger::{DangerDriver, DodgeAction};
pub use heatmap::Heatmap;
pub use move_ctrl::{MovementController, MovementIntent};
pub use nav::{NavGoal, NavigationDriver};
pub use nav_mode::Navigator;
pub use navmesh_driver::NavmeshDriver;
pub use observed::{parse_obituary, HeatmapObserver, HeatmapSnapshot, Obituary};
pub use perception::{EntityClass, PerceivedEntity, SelfState, Worldview};
pub use q3char::{CharPreset, Q3Character};
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
