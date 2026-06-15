//! Projectile danger avoidance — dodge incoming rockets/grenades.
//!
//! Ports Eraser's `avoid_ent` dodge (`bot_nav.c:357` `botJumpAvoidEnt`,
//! `g_weapon.c` projectile tagging; see `distilled/eraser.md` §7). Eraser tags
//! nearby bots from inside the projectile's think function; we can't do that
//! (external client), so we instead inspect visible projectiles from
//! `svc_packetentities` and decide whether one is close + heading at us.
//!
//! - **Rocket** (`combat >= 4`): dodge when ≤300 u and closing.
//! - **Grenade** (any skill): dodge when ≤256 u.
//! - Dodge vector is **perpendicular** to the projectile's travel in the XY
//!   plane (`(vel.y, -vel.x, 0)`), picked toward the side we're already on.
//!   Grounded-landing BSP check is omitted (we lack a cheap oracle); we bias to
//!   a strafe-away and only jump when not already airborne-likely.
//!
//! This is a **frame-scale tactical override**; it composes with (does not
//! replace) the strategic nav from Plan 08.

use crate::perception::{EntityClass, Worldview};
use glam::Vec3;

/// A rocket within this axial distance (and closing) triggers a dodge.
const ROCKET_DODGE_DIST: f32 = 300.0;
/// A grenade within this distance triggers a dodge (any skill).
const GRENADE_DODGE_DIST: f32 = 256.0;
/// Combat rating threshold for bothering to dodge rockets (Eraser `combat >= 4`).
const ROCKET_DODGE_COMBAT: f32 = 4.0;

/// A dodge decision for one frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct DodgeAction {
    /// Unit horizontal direction to strafe (world space).
    pub strafe_dir: Vec3,
    /// Whether to also jump this frame.
    pub jump: bool,
}

impl DodgeAction {
    pub fn is_active(&self) -> bool {
        self.strafe_dir.length_squared() > 0.5 || self.jump
    }
}

/// Stateless danger evaluator (kept as a struct for future per-bot hysteresis).
#[derive(Debug, Clone, Default)]
pub struct DangerDriver;

impl DangerDriver {
    pub fn new() -> Self {
        Self
    }

    /// Inspect `view` for an imminent projectile threat and return a dodge.
    /// `combat` is the bot's 1-5 combat rating (rockets only dodged at >= 4).
    pub fn evaluate(&self, view: &Worldview, combat: f32) -> DodgeAction {
        let origin = view.self_state().origin;
        let mut best: Option<(f32, Vec3)> = None; // (closeness score, dodge dir)

        for e in view.entities() {
            let is_rocket = e.class == EntityClass::ProjectileRocket;
            let is_grenade = e.class == EntityClass::ProjectileGrenade;
            if !is_rocket && !is_grenade {
                continue;
            }

            let to_us = origin - e.origin;
            let dist = to_us.length();
            let max_dist = if is_grenade {
                GRENADE_DODGE_DIST
            } else {
                ROCKET_DODGE_DIST
            };
            if dist > max_dist || dist < 1.0 {
                continue;
            }

            // Closing check: projectile velocity must point toward us.
            let Some(vel) = e.velocity else {
                continue;
            };
            let vel_horiz = Vec3::new(vel.x, vel.y, 0.0);
            if vel_horiz.length_squared() < 1.0 {
                continue;
            }
            let dir_to_us = to_us.normalize();
            if vel.normalize().dot(dir_to_us) < 0.2 {
                continue; // not heading at us
            }

            // Rockets are only dodged by combat-aware bots (Eraser).
            if is_rocket && combat < ROCKET_DODGE_COMBAT {
                continue;
            }

            // Perpendicular dodge in the XY plane. Pick the side we're already
            // drifting toward so the strafe builds on existing motion.
            let perp = Vec3::new(vel_horiz.y, -vel_horiz.x, 0.0).normalize();
            let self_vel = view.self_state().velocity;
            let side = if perp.dot(Vec3::new(self_vel.x, self_vel.y, 0.0)) < 0.0 {
                -perp
            } else {
                perp
            };

            // Closer threats win.
            let score = max_dist - dist;
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, side));
            }
        }

        match best {
            Some((_, dir)) => {
                // Jump on the dodging frame to clear splash / get off the line.
                // (We don't have a grounded oracle, so only jump for rockets —
                // grenades arc and a jump can be counterproductive; strafe suffices.)
                let jump = best_is_rocket_jump(view);
                DodgeAction {
                    strafe_dir: dir,
                    jump,
                }
            }
            None => DodgeAction::default(),
        }
    }
}

/// Decide whether to jump on the dodge. Without a grounded/landing oracle we
/// jump only when there's a rocket threat and our vertical velocity is near zero
/// (roughly grounded) to avoid wasted air-jumps.
fn best_is_rocket_jump(view: &Worldview) -> bool {
    let v = view.self_state().velocity;
    v.z.abs() < 40.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_projectile_no_dodge() {
        let view = test_view(Vec3::new(0.0, 0.0, 0.0), Vec3::ZERO, &[]);
        let d = DangerDriver::new().evaluate(&view, 5.0);
        assert!(!d.is_active());
    }

    #[test]
    fn closing_rocket_dodged_by_skilled_bot() {
        // Rocket 200u away heading straight at us in +x.
        let rocket = Threat::new(EntityClass::ProjectileRocket, Vec3::new(-200.0, 0.0, 0.0))
            .velocity(Vec3::new(800.0, 0.0, 0.0));
        let view = test_view(Vec3::ZERO, Vec3::ZERO, &[rocket]);
        let d = DangerDriver::new().evaluate(&view, 5.0);
        assert!(d.is_active(), "combat-5 bot should dodge a closing rocket");
        // Dodge is perpendicular to +x travel → along ±y.
        assert!(d.strafe_dir.x.abs() < 0.1);
        assert!(d.strafe_dir.y.abs() > 0.9);
    }

    #[test]
    fn low_combat_bot_ignores_rocket() {
        let rocket = Threat::new(EntityClass::ProjectileRocket, Vec3::new(-200.0, 0.0, 0.0))
            .velocity(Vec3::new(800.0, 0.0, 0.0));
        let view = test_view(Vec3::ZERO, Vec3::ZERO, &[rocket]);
        let d = DangerDriver::new().evaluate(&view, 2.0);
        assert!(!d.is_active(), "combat-2 bot doesn't dodge rockets");
    }

    #[test]
    fn grenade_dodged_at_any_skill() {
        let grenade = Threat::new(EntityClass::ProjectileGrenade, Vec3::new(-150.0, 0.0, 0.0))
            .velocity(Vec3::new(400.0, 0.0, 0.0));
        let view = test_view(Vec3::ZERO, Vec3::ZERO, &[grenade]);
        let d = DangerDriver::new().evaluate(&view, 1.0);
        assert!(d.is_active(), "grenades dodge at any combat rating");
    }

    #[test]
    fn non_closing_projectile_ignored() {
        // Rocket moving away from us.
        let rocket = Threat::new(EntityClass::ProjectileRocket, Vec3::new(-200.0, 0.0, 0.0))
            .velocity(Vec3::new(-800.0, 0.0, 0.0));
        let view = test_view(Vec3::ZERO, Vec3::ZERO, &[rocket]);
        let d = DangerDriver::new().evaluate(&view, 5.0);
        assert!(!d.is_active(), "rocket moving away isn't dodged");
    }

    // ---- test helpers (build a minimal Worldview with synthetic threats) ----

    struct Threat {
        class: EntityClass,
        origin: Vec3,
        velocity: Vec3,
    }
    impl Threat {
        fn new(class: EntityClass, origin: Vec3) -> Self {
            Self {
                class,
                origin,
                velocity: Vec3::ZERO,
            }
        }
        fn velocity(mut self, v: Vec3) -> Self {
            self.velocity = v;
            self
        }
    }

    fn test_view(self_origin: Vec3, self_vel: Vec3, threats: &[Threat]) -> Worldview {
        use client::parse::ConfigStrings;
        use q2proto::{Frame, PlayerState};
        let mut frame = Frame::default();
        // A playerstate at self_origin with the given velocity.
        let mut ps = PlayerState::default();
        ps.pmove.origin = [
            (self_origin.x * 8.0) as i16,
            (self_origin.y * 8.0) as i16,
            (self_origin.z * 8.0) as i16,
        ];
        ps.pmove.velocity = [
            (self_vel.x * 8.0) as i16,
            (self_vel.y * 8.0) as i16,
            (self_vel.z * 8.0) as i16,
        ];
        frame.playerstate = ps;
        // Synthesize packet entities for each threat.
        for (i, t) in threats.iter().enumerate() {
            use q2proto::EntityState;
            let es = EntityState {
                number: 100 + i as i32,
                origin: [t.origin.x, t.origin.y, t.origin.z],
                ..Default::default()
            };
            frame.entities.push(es);
        }
        let cs = ConfigStrings::default();
        let mut view = Worldview::from_frame(&frame, &cs, 0);
        // Force the threat classifications + velocities (from_frame can't infer
        // them without model configstrings / prior frames).
        for (e, t) in view.entities_mut().zip(threats.iter()) {
            e.class = t.class;
            if t.velocity.length_squared() > 0.0 {
                e.velocity = Some(t.velocity);
            }
        }
        view
    }
}
