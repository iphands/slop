//! # brain::brains — the brain plugin layer (Plan 23)
//!
//! `core` defines the `trait Brain` contract + shared I/O types. The `BrainKind` enum +
//! `build_brain` factory (Plan 23 T4) select an implementation at startup, exactly mirroring
//! the nav layer's `NavMode` / `build_navigator`.

pub mod core;
pub mod main;
pub mod q3;
pub mod runtester;
pub mod sentry;
pub mod xon;
pub mod zb2;

pub use core::{Brain, BrainConfig, BrainContext, BrainMap, BrainOutput};

use crate::brains::main::MainBrain;
use crate::brains::q3::Q3Brain;
use crate::brains::runtester::RunTesterBrain;
use crate::brains::sentry::SentryBrain;
use crate::brains::xon::XonBrain;
use crate::brains::zb2::Zb2Brain;
use crate::q3char::{CharPreset, Q3Character};
use crate::skill::BotSkill;
use crate::xonchar::{XonCharPreset, XonSkill};

/// Which brain implementation a bot runs. Mirrors `NavMode` (the nav-backend selector); a
/// `ValueEnum` derive + CLI flag land in Plan 25, more variants in Plan 24.
/// Which brain implementation a bot runs. A clap `ValueEnum` so `--brain <kind>` selects it,
/// mirroring `NavMode` for the nav backend; the two are independent per-bot axes.
/// Each variant's canonical CLI token is its short code (`mai`, `q3`, …) — the same code the
/// competition scoreboard/bot-names use — with the long name kept as an accepted alias, so
/// both `--brain mai` and `--brain main` work. The `--help` line leads with the short code.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum BrainKind {
    /// main — The full decision brain (combat + FSM + nav + recovery) — the live fleet bot.
    #[value(name = "mai", alias = "main")]
    Main,
    /// sentry — A stationary combat-only reference brain that proves the seam runs with >1 impl.
    #[value(name = "sen", alias = "sentry")]
    Sentry,
    /// runtester — The combat-free movement-scenario brain (the `spawn-to-*` pathfinder).
    #[value(name = "run", alias = "runtester")]
    RunTester,
    /// quake3 — The Quake 3-derived brain (node FSM + aggression-gated retreat/chase + Q3 aim/fire).
    #[value(name = "q3", alias = "quake3")]
    Quake3,
    /// zb2 — The 3ZB2-derived brain (committed routes + shortcut skips + run-and-gun; Plan 44).
    #[value(name = "zb2")]
    Zb2,
    /// xonotic — The Xonotic-havocbot-derived brain (goal-rating strategy + XonAim + keyboard
    /// movement; Plan 60).
    #[value(name = "xon", alias = "xonotic")]
    Xon,
}

/// Short kebab-case tag for `kind` — for logging + competition bot naming (mirrors `mode_tag`).
pub fn brain_tag(kind: BrainKind) -> &'static str {
    match kind {
        BrainKind::Main => "main",
        BrainKind::Sentry => "sentry",
        BrainKind::RunTester => "runtester",
        BrainKind::Quake3 => "q3",
        BrainKind::Zb2 => "zb2",
        BrainKind::Xon => "xon",
    }
}

/// Build the brain implementation for `kind`. Single match — the kind→impl mapping lives here,
/// exactly mirroring `build_navigator` for nav backends. `Send` so a bot task can own the box.
///
/// `char` selects a named Q3 personality for the `Quake3` brain (Plan 38 roster); `None` →
/// `Q3Character::from_skill(skill)` (the Plan 37 default). `xonchar` is the same idea for the
/// `Xon` brain (Plan 60/62 roster); `None` → a neutral `XonSkill` at the master skill level.
/// Every other arm ignores both.
pub fn build_brain(
    kind: BrainKind,
    skill: BotSkill,
    cfg: BrainConfig,
    char: Option<CharPreset>,
    persona: Option<crate::persona::Persona>,
    xonchar: Option<XonCharPreset>,
) -> Box<dyn Brain + Send> {
    match kind {
        BrainKind::Main => Box::new(MainBrain::new(skill, cfg).with_persona(persona)),
        // Sentry ignores `cfg` (no nav, no goal override) — it's a proof-of-pluggability.
        BrainKind::Sentry => Box::new(SentryBrain::new(skill)),
        // RunTester is combat-free and goal-driven per tick; it needs neither skill nor cfg.
        BrainKind::RunTester => Box::new(RunTesterBrain::new()),
        // Quake3: a named roster preset if given, else derive the character from the master skill
        // level. `cfg` is unused: in a movement scenario there are no enemies, so the Q3 combat
        // path never fires anyway.
        BrainKind::Quake3 => {
            let ch = char
                .map(CharPreset::character)
                .unwrap_or_else(|| Q3Character::from_skill(skill.skill));
            Box::new(Q3Brain::new(ch))
        }
        // Zb2 reuses the shared combat driver; `cfg.combat_enabled` gates it for scenarios.
        // It ignores `char`/`persona` (its personality IS the committed-route texture).
        BrainKind::Zb2 => Box::new(Zb2Brain::new(skill, cfg.combat_enabled)),
        // Xon: a named 12-axis roster preset if given, else neutral at the master skill.
        BrainKind::Xon => {
            let sk = xonchar
                .map(XonCharPreset::skill)
                .unwrap_or_else(|| XonSkill::new(skill.skill.min(10) as f32));
            Box::new(XonBrain::new(sk, cfg))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_main_brain_starts_roaming() {
        let brain = build_brain(
            BrainKind::Main,
            BotSkill::default(),
            BrainConfig::default(),
            None,
            None,
            None,
        );
        assert_eq!(brain.status(), "roam");
    }

    #[test]
    fn build_sentry_brain_labels_sentry() {
        let brain = build_brain(
            BrainKind::Sentry,
            BotSkill::default(),
            BrainConfig::default(),
            None,
            None,
            None,
        );
        assert_eq!(brain.status(), "sentry");
    }

    #[test]
    fn brain_kind_value_enum_round_trip() {
        use clap::ValueEnum;
        // Long names still parse (kept as aliases)…
        assert_eq!(BrainKind::from_str("main", true), Ok(BrainKind::Main));
        assert_eq!(BrainKind::from_str("sentry", true), Ok(BrainKind::Sentry));
        assert_eq!(
            BrainKind::from_str("runtester", true),
            Ok(BrainKind::RunTester)
        );
        assert_eq!(BrainKind::from_str("quake3", true), Ok(BrainKind::Quake3));
        // …and so do the short codes (now the canonical CLI tokens).
        assert_eq!(BrainKind::from_str("mai", true), Ok(BrainKind::Main));
        assert_eq!(BrainKind::from_str("sen", true), Ok(BrainKind::Sentry));
        assert_eq!(BrainKind::from_str("run", true), Ok(BrainKind::RunTester));
        assert_eq!(BrainKind::from_str("q3", true), Ok(BrainKind::Quake3));
        assert_eq!(BrainKind::from_str("zb2", true), Ok(BrainKind::Zb2));
        assert_eq!(BrainKind::from_str("xon", true), Ok(BrainKind::Xon));
        assert_eq!(BrainKind::from_str("xonotic", true), Ok(BrainKind::Xon));
        assert!(BrainKind::from_str("nope", true).is_err());
        assert_eq!(brain_tag(BrainKind::Main), "main");
        assert_eq!(brain_tag(BrainKind::Sentry), "sentry");
        assert_eq!(brain_tag(BrainKind::RunTester), "runtester");
        assert_eq!(brain_tag(BrainKind::Quake3), "q3");
        assert_eq!(brain_tag(BrainKind::Xon), "xon");
    }

    #[test]
    fn build_q3_brain_starts_in_seek_ltg() {
        let brain = build_brain(
            BrainKind::Quake3,
            BotSkill::default(),
            BrainConfig::default(),
            None,
            None,
            None,
        );
        assert_eq!(brain.status(), "seek-ltg");
    }
}
