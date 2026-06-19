//! # brain::brains — the brain plugin layer (Plan 23)
//!
//! `core` defines the `trait Brain` contract + shared I/O types. The `BrainKind` enum +
//! `build_brain` factory (Plan 23 T4) select an implementation at startup, exactly mirroring
//! the nav layer's `NavMode` / `build_navigator`.

pub mod core;
pub mod main;
pub mod runtester;
pub mod sentry;

pub use core::{Brain, BrainConfig, BrainContext, BrainMap, BrainOutput};

use crate::brains::main::MainBrain;
use crate::brains::runtester::RuntesterBrain;
use crate::brains::sentry::SentryBrain;
use crate::skill::BotSkill;

/// Which brain implementation a bot runs. Mirrors `NavMode` (the nav-backend selector); a
/// `ValueEnum` derive + CLI flag land in Plan 25, more variants in Plan 24.
/// Which brain implementation a bot runs. A clap `ValueEnum` so `--brain <kind>` selects it,
/// mirroring `NavMode` for the nav backend; the two are independent per-bot axes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum BrainKind {
    /// The full decision brain (combat + FSM + nav + recovery) — the live fleet bot.
    Main,
    /// A stationary combat-only reference brain that proves the seam runs with >1 impl.
    Sentry,
    /// The combat-free movement-scenario brain (the `spawn-to-*` pathfinder).
    Runtester,
}

/// Short kebab-case tag for `kind` — for logging + competition bot naming (mirrors `mode_tag`).
pub fn brain_tag(kind: BrainKind) -> &'static str {
    match kind {
        BrainKind::Main => "main",
        BrainKind::Sentry => "sentry",
        BrainKind::Runtester => "runtester",
    }
}

/// Build the brain implementation for `kind`. Single match — the kind→impl mapping lives here,
/// exactly mirroring `build_navigator` for nav backends. `Send` so a bot task can own the box.
pub fn build_brain(kind: BrainKind, skill: BotSkill, cfg: BrainConfig) -> Box<dyn Brain + Send> {
    match kind {
        BrainKind::Main => Box::new(MainBrain::new(skill, cfg)),
        // Sentry ignores `cfg` (no nav, no goal override) — it's a proof-of-pluggability.
        BrainKind::Sentry => Box::new(SentryBrain::new(skill)),
        // Runtester is combat-free and goal-driven per tick; it needs neither skill nor cfg.
        BrainKind::Runtester => Box::new(RuntesterBrain::new()),
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

    #[test]
    fn build_sentry_brain_labels_sentry() {
        let brain = build_brain(
            BrainKind::Sentry,
            BotSkill::default(),
            BrainConfig::default(),
        );
        assert_eq!(brain.status(), "sentry");
    }

    #[test]
    fn brain_kind_value_enum_round_trip() {
        use clap::ValueEnum;
        assert_eq!(BrainKind::from_str("main", true), Ok(BrainKind::Main));
        assert_eq!(BrainKind::from_str("sentry", true), Ok(BrainKind::Sentry));
        assert_eq!(
            BrainKind::from_str("runtester", true),
            Ok(BrainKind::Runtester)
        );
        assert!(BrainKind::from_str("nope", true).is_err());
        assert_eq!(brain_tag(BrainKind::Main), "main");
        assert_eq!(brain_tag(BrainKind::Sentry), "sentry");
        assert_eq!(brain_tag(BrainKind::Runtester), "runtester");
    }
}
