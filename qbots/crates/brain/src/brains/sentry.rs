//! # brain::brains::sentry — a minimal reference brain (Plan 24)
//!
//! `SentryBrain` is the simplest possible `trait Brain` implementation: it stands still and
//! fires at any enemy combat can see, ignoring navigation entirely. It exists to **prove the
//! plugin seam runs with more than one brain** — a single-impl trait is easy to get subtly
//! wrong (e.g. a method that secretly assumes `MainBrain`'s FSM). It shares no state with
//! `MainBrain`; if it satisfies the contract and runs, the seam is real.

use crate::brains::core::{Brain, BrainContext, BrainMap, BrainOutput};
use crate::combat::CombatDriver;
use crate::move_ctrl::MovementIntent;
use crate::skill::BotSkill;

/// A stationary, combat-only brain: aim + fire at any LOS enemy, never move.
pub struct SentryBrain {
    combat: CombatDriver,
    skill: BotSkill,
}

impl SentryBrain {
    /// Construct a sentry with the given skill/personality.
    pub fn new(skill: BotSkill) -> Self {
        Self {
            combat: CombatDriver::new(),
            skill,
        }
    }
}

impl Brain for SentryBrain {
    /// Sentries ignore navigation entirely — no roam nodes, no graph.
    fn set_map(&mut self, _map: BrainMap) {}

    fn tick(&mut self, ctx: BrainContext) -> BrainOutput {
        let jitter = (ctx.ticks as f32) * 0.1;
        let dec = self.combat.evaluate(ctx.view, &self.skill, jitter, ctx.cm);
        let mut mv = MovementIntent::new();
        if dec.should_fire {
            mv.attack();
            mv.look_at(dec.aim_yaw, dec.aim_pitch);
        }
        BrainOutput {
            intent: mv,
            weapon_request: dec.weapon_request.map(|r| r.0),
        }
    }

    fn status(&self) -> &str {
        "sentry"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perception::Worldview;
    use client::parse::ConfigStrings;
    use q2proto::Frame;

    #[test]
    fn sentry_constructs_and_labels() {
        let s = SentryBrain::new(BotSkill::default());
        assert_eq!(s.status(), "sentry");
    }

    #[test]
    fn sentry_with_no_enemy_does_not_move() {
        let mut s = SentryBrain::new(BotSkill::default());
        let view = Worldview::from_frame(&Frame::default(), &ConfigStrings::default(), 0);
        let out = s.tick(BrainContext {
            view: &view,
            nav: None,
            cm: None,
            dt: 0.1,
            ticks: 1,
        });
        assert_eq!(out.intent.forward, 0.0);
        assert_eq!(out.intent.side, 0.0);
        assert_eq!(out.intent.up, 0.0);
    }
}
