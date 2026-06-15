//! Combat driver — target selection, weapon selection, fire decision.
//!
//! Orchestrates `aim.rs` + `weapons.rs`. Maintains a target cache to prevent
//! thrashing and a held-weapon model so we only request a switch on change.
//!
//! Weapon switching is done by emitting a [`WeaponRequest`] (`use <name>`
//! stringcmd); Q2 ignores `usercmd.impulse`, so the connection layer sends the
//! `use` command, not an impulse. Ownership isn't visible on the wire (Q2's HUD
//! is server-driven), so we request optimistically and the server grants it
//! only if we own the weapon.

use crate::aim::{aim_hitscan, aim_projectile};
use crate::perception::{EntityClass, Worldview};
use crate::weapons::{self, Weapon};

/// Frames to hold on a target before considering a switch (~0.5s at 10 Hz),
/// preventing thrashing when enemies pop in/out of PVS. (Eraser "target stability".)
const TARGET_LOCK_FRAMES: u32 = 5;

/// Minimum frames between shots (fire-rate limiter).
const FIRE_RATE_COOLDOWN_FRAMES: u32 = 2;

/// A requested weapon switch (`use <name>`). The connection layer converts this
/// to a reliable stringcmd.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeaponRequest(pub Weapon);

/// Combat decision for one frame.
#[derive(Debug, Clone, Copy)]
pub struct CombatDecision {
    pub should_fire: bool,
    pub aim_yaw: f32,
    pub aim_pitch: f32,
    pub target_entity: Option<i32>,
    /// A `use <name>` switch to emit this frame, if the desired weapon changed.
    pub weapon_request: Option<WeaponRequest>,
}

impl Default for CombatDecision {
    fn default() -> Self {
        Self {
            should_fire: false,
            aim_yaw: 0.0,
            aim_pitch: 0.0,
            target_entity: None,
            weapon_request: None,
        }
    }
}

/// Combat driver state.
pub struct CombatDriver {
    current_target: Option<i32>,
    lock_frames_remaining: u32,
    frames_since_shot: u32,
    /// Weapon we believe we're holding. Optimistic: set when we request a switch
    /// (the server grants it only if owned). Reset to Blaster on respawn.
    held_weapon: Weapon,
}

impl CombatDriver {
    pub fn new() -> Self {
        Self {
            current_target: None,
            lock_frames_remaining: 0,
            frames_since_shot: 10,
            held_weapon: Weapon::Blaster,
        }
    }

    /// The weapon we currently believe we're holding.
    pub fn held_weapon(&self) -> Weapon {
        self.held_weapon
    }

    /// Reset held-weapon tracking to the Blaster (e.g. after a respawn, where the
    /// server forces us back to the spawn loadout).
    pub fn on_respawn(&mut self) {
        self.held_weapon = Weapon::Blaster;
    }

    /// Evaluate combat state and produce a decision.
    pub fn evaluate(&mut self, view: &Worldview, skill: f32, jitter_seed: f32) -> CombatDecision {
        let target_num = self.select_target_entity(view);

        let Some(num) = target_num else {
            self.frames_since_shot += 1;
            return CombatDecision::default();
        };

        let Some(t) = view.entities().find(|e| e.entity_number == num) else {
            self.frames_since_shot += 1;
            return CombatDecision::default();
        };

        let distance = (t.origin - view.self_state().origin).length();

        // Pick the best weapon for this distance; only request a switch if it
        // differs from what we're holding.
        let desired = weapons::select_best_weapon(self.held_weapon, distance);
        let weapon_request = (desired != self.held_weapon).then(|| {
            self.held_weapon = desired;
            WeaponRequest(desired)
        });

        if let Some(WeaponRequest(w)) = weapon_request {
            tracing::info!(weapon = %w.name(), distance = %format!("{:.0}", distance), "requesting weapon");
        }

        let weapon = self.held_weapon;

        let (yaw, pitch) = if weapon.is_hitscan() {
            aim_hitscan(
                view.self_state().origin,
                t.origin,
                t.velocity,
                skill,
                jitter_seed,
            )
        } else {
            let proj_speed = weapon.projectile_speed().unwrap_or(500.0);
            aim_projectile(view.self_state().origin, t.origin, t.velocity, proj_speed)
        };

        let should_fire = self.should_fire(weapon, distance);

        self.frames_since_shot = if should_fire {
            0
        } else {
            self.frames_since_shot + 1
        };

        if should_fire {
            tracing::info!(
                target = t.entity_number,
                distance = %format!("{:.0}", distance),
                weapon = %weapon.name(),
                "shooting at player"
            );
        }

        CombatDecision {
            should_fire,
            aim_yaw: yaw,
            aim_pitch: pitch,
            target_entity: Some(t.entity_number),
            weapon_request,
        }
    }

    fn select_target_entity(&mut self, view: &Worldview) -> Option<i32> {
        if let Some(target_num) = self.current_target {
            if self.lock_frames_remaining > 0
                && view.enemies().any(|e| e.entity_number == target_num)
            {
                self.lock_frames_remaining -= 1;
                return Some(target_num);
            }
        }

        if let Some(t) = view.nearest_enemy(90.0) {
            self.current_target = Some(t.entity_number);
            self.lock_frames_remaining = TARGET_LOCK_FRAMES;
            return Some(t.entity_number);
        }

        // Fall back to the nearest stale (last-known) enemy.
        let stale = view
            .entities()
            .filter(|e| e.class == EntityClass::EnemyPlayer && e.is_stale)
            .min_by(|a, b| {
                let da = (a.origin - view.self_state().origin).length_squared();
                let db = (b.origin - view.self_state().origin).length_squared();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            });

        if let Some(t) = stale {
            self.current_target = Some(t.entity_number);
            self.lock_frames_remaining = TARGET_LOCK_FRAMES;
            return Some(t.entity_number);
        }

        None
    }

    fn should_fire(&self, weapon: Weapon, distance: f32) -> bool {
        if self.frames_since_shot < FIRE_RATE_COOLDOWN_FRAMES {
            return false;
        }
        if distance < weapon.min_safe_distance() {
            return false;
        }
        // Don't waste a blaster bolt across the whole map — it'll never land.
        if weapon == Weapon::Blaster && distance > weapon.effective_range() {
            return false;
        }
        true
    }

    pub fn target_entity(&self) -> Option<i32> {
        self.current_target
    }
}

impl Default for CombatDriver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use client::parse::ConfigStrings;
    use q2proto::Frame;

    #[test]
    fn combat_driver_starts_with_blaster() {
        let driver = CombatDriver::new();
        assert_eq!(driver.held_weapon(), Weapon::Blaster);
        assert!(driver.current_target.is_none());
    }

    #[test]
    fn no_target_means_no_fire() {
        let mut driver = CombatDriver::new();
        let frame = Frame::default();
        let config = ConfigStrings::default();
        let view = crate::perception::Worldview::from_frame(&frame, &config, 0);
        let decision = driver.evaluate(&view, 0.5, 0.0);
        assert!(!decision.should_fire);
        assert!(decision.target_entity.is_none());
        assert!(decision.weapon_request.is_none());
    }

    #[test]
    fn respawn_resets_to_blaster() {
        let mut driver = CombatDriver::new();
        driver.held_weapon = Weapon::RocketLauncher;
        driver.on_respawn();
        assert_eq!(driver.held_weapon(), Weapon::Blaster);
    }

    #[test]
    fn should_not_fire_blaster_across_map() {
        let driver = CombatDriver::new();
        // Blaster effective range ~600; at 2000 it shouldn't fire.
        assert!(!driver.should_fire(Weapon::Blaster, 2000.0));
        assert!(driver.should_fire(Weapon::Blaster, 200.0));
    }

    #[test]
    fn should_not_fire_rocket_point_blank() {
        let driver = CombatDriver::new();
        assert!(!driver.should_fire(Weapon::RocketLauncher, 50.0));
        assert!(driver.should_fire(Weapon::RocketLauncher, 300.0));
    }
}
