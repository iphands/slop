//! # brain — bot AI
//!
//! Turns per-frame perception into intent: navigation over the `world` nav graph, combat
//! (aim/lead/weapon-select), and a behavior FSM. Builds on `client` + `world`.
//! Filled in by Plan 06.
//!
//! See `AGENTS.md` and `context/plans/06_brain.md`.

pub mod perception;

pub use perception::{EntityClass, PerceivedEntity, SelfState, Worldview};

#[cfg(test)]
mod tests {
    #[test]
    fn sanity() {
        assert_eq!(2 + 2, 4);
    }
}
