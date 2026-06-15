//! # brain — bot AI
//!
//! Turns per-frame perception into intent: navigation over the `world` nav graph, combat
//! (aim/lead/weapon-select), and a behavior FSM. Builds on `client` + `world`.
//! Filled in by Plan 06.
//!
//! See `AGENTS.md` and `context/plans/06_brain.md`.

pub mod aim;
pub mod combat;
pub mod move_ctrl;
pub mod nav;
pub mod perception;
pub mod weapons;

pub use combat::{CombatDecision, CombatDriver};
pub use move_ctrl::{MovementController, MovementIntent};
pub use nav::{NavGoal, NavigationDriver, StuckAction};
pub use perception::{EntityClass, PerceivedEntity, SelfState, Worldview};
pub use weapons::Weapon;

#[cfg(test)]
mod tests {
    #[test]
    fn sanity() {
        assert_eq!(2 + 2, 4);
    }
}
