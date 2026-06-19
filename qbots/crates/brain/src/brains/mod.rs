//! # brain::brains — the brain plugin layer (Plan 23)
//!
//! `core` defines the `trait Brain` contract + shared I/O types. The `BrainKind` enum +
//! `build_brain` factory (Plan 23 T4) select an implementation at startup, exactly mirroring
//! the nav layer's `NavMode` / `build_navigator`.

pub mod core;

pub use core::{Brain, BrainConfig, BrainContext, BrainMap, BrainOutput};

use crate::brain::Brain as MainBrain;
use crate::skill::BotSkill;

/// Which brain implementation a bot runs. Mirrors `NavMode` (the nav-backend selector); a
/// `ValueEnum` derive + CLI flag land in Plan 25, more variants in Plan 24.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BrainKind {
    /// The full decision brain (combat + FSM + nav + recovery) — the live fleet bot.
    Main,
}

/// Build the brain implementation for `kind`. Single match — the kind→impl mapping lives here,
/// exactly mirroring `build_navigator` for nav backends. `Send` so a bot task can own the box.
pub fn build_brain(kind: BrainKind, skill: BotSkill, cfg: BrainConfig) -> Box<dyn Brain + Send> {
    match kind {
        // The concrete brain relocates into `brains/main.rs` (renamed `MainBrain`) in Plan 24;
        // for now the factory references it where it lives.
        BrainKind::Main => Box::new(MainBrain::new(skill, cfg)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_main_brain_starts_roaming() {
        let brain = build_brain(BrainKind::Main, BotSkill::default(), BrainConfig::default());
        assert_eq!(brain.status(), "roam");
    }
}
