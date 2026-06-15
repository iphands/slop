//! Combat driver — target selection, fire decision, danger avoidance.
//!
//! Orchestrates aim.rs + weapons.rs. Maintains target cache to prevent thrashing.

use crate::aim::{aim_hitscan, aim_projectile};
use crate::perception::{EntityClass, Worldview};
use crate::weapons::{self, Weapon};

/// Frames to hold on a target before considering a switch.
const TARGET_LOCK_FRAMES: u32 = 5;

/// Combat decision for one frame.
#[derive(Debug, Clone, Copy)]
pub struct CombatDecision {
    pub should_fire: bool,
    pub aim_yaw: f32,
    pub aim_pitch: f32,
    pub target_entity: Option<i32>,
    pub impulse: Option<u8>,
}

/// Combat driver state.
pub struct CombatDriver {
    current_target: Option<i32>,
    lock_frames_remaining: u32,
    frames_since_shot: u32,
    current_weapon: Weapon,
}

impl CombatDriver {
    pub fn new() -> Self {
        Self {
            current_target: None,
            lock_frames_remaining: 0,
            frames_since_shot: 10,
            current_weapon: Weapon::Blaster,
        }
    }

    /// Evaluate combat state and produce a decision.
    pub fn evaluate(&mut self, view: &Worldview, skill: f32, jitter_seed: f32) -> CombatDecision {
        self.current_weapon = match view.self_state().weapon {
            1 => Weapon::Blaster,
            2 => Weapon::Shotgun,
            3 => Weapon::Nailgun,
            4 => Weapon::GrenadeLauncher,
            5 => Weapon::HandGrenade,
            6 => Weapon::Railgun,
            7 => Weapon::BFG10k,
            8 => Weapon::RocketLauncher,
            9 => Weapon::Hyperblaster,
            10 => Weapon::Chaingun,
            _ => Weapon::Blaster,
        };

        let target_num = self.select_target_entity(view);

        if let Some(num) = target_num {
            let target = view
                .entities()
                .find(|e| e.entity_number == num)
                .or_else(|| view.entities().find(|e| e.entity_number == num));

            if let Some(t) = target {
                let distance = (t.origin - view.self_state().origin).length();
                let ammo = view.self_state().ammo;

                let best_weapon = weapons::select_best_weapon(self.current_weapon, &ammo, distance);
                let impulse = if best_weapon != self.current_weapon {
                    Some(best_weapon as u8)
                } else {
                    None
                };

                let (yaw, pitch) = if self.current_weapon.is_hitscan() {
                    aim_hitscan(
                        view.self_state().origin,
                        t.origin,
                        t.velocity,
                        skill,
                        jitter_seed,
                    )
                } else {
                    let proj_speed = self.current_weapon.projectile_speed().unwrap_or(500.0);
                    aim_projectile(view.self_state().origin, t.origin, t.velocity, proj_speed)
                };

                let should_fire = self.should_fire(view, distance);

                self.frames_since_shot = if should_fire {
                    0
                } else {
                    self.frames_since_shot + 1
                };

                return CombatDecision {
                    should_fire,
                    aim_yaw: yaw,
                    aim_pitch: pitch,
                    target_entity: Some(t.entity_number),
                    impulse,
                };
            }
        }

        self.frames_since_shot += 1;
        CombatDecision {
            should_fire: false,
            aim_yaw: 0.0,
            aim_pitch: 0.0,
            target_entity: None,
            impulse: None,
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

    fn should_fire(&self, view: &Worldview, distance: f32) -> bool {
        if self.frames_since_shot < 2 {
            return false;
        }

        if distance < self.current_weapon.min_safe_distance() {
            return false;
        }

        let ammo_idx = self.current_weapon.ammo_index();
        let ammo = if ammo_idx < view.self_state().ammo.len() {
            view.self_state().ammo[ammo_idx]
        } else {
            0
        };
        if self.current_weapon.ammo_cost() > 0 && ammo <= 0 {
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
    fn combat_driver_starts_ready() {
        let driver = CombatDriver::new();
        assert_eq!(driver.current_weapon, Weapon::Blaster);
        assert!(driver.current_target.is_none());
    }

    #[test]
    fn no_target_means_no_fire() {
        let mut driver = CombatDriver::new();
        let frame = Frame::default();
        let config = ConfigStrings::default();
        let view = crate::perception::Worldview::from_frame(&frame, &config);
        let decision = driver.evaluate(&view, 0.5, 0.0);
        assert!(!decision.should_fire);
        assert!(decision.target_entity.is_none());
    }
}
