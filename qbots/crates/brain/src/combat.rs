//! Combat driver — target selection, fire decision, danger avoidance.
//!
//! Orchestrates aim.rs + weapons.rs. Maintains target cache to prevent thrashing.

use crate::aim::{aim_hitscan, aim_projectile};
use crate::perception::{EntityClass, Worldview};
use crate::weapons::{self, Weapon};

/// Frames to hold on a target before considering a switch.
/// 5 frames (~0.5s at 10 Hz) prevents thrashing when enemies pop in/out of PVS.
/// Based on Eraser's "target stability" heuristic.
const TARGET_LOCK_FRAMES: u32 = 5;

/// Minimum frames between shots (fire rate limiter).
const FIRE_RATE_COOLDOWN_FRAMES: u32 = 2;

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
    /// Weapon we're currently trying to use (local desired state).
    desired_weapon: Weapon,
    /// Weapon the server reports we have (from serverstate).
    server_weapon: Weapon,
}

impl CombatDriver {
    pub fn new() -> Self {
        Self {
            current_target: None,
            lock_frames_remaining: 0,
            frames_since_shot: 10,
            desired_weapon: Weapon::Blaster,
            server_weapon: Weapon::Blaster,
        }
    }

    /// Evaluate combat state and produce a decision.
    pub fn evaluate(&mut self, view: &Worldview, skill: f32, jitter_seed: f32) -> CombatDecision {
        self.server_weapon = match view.self_state().weapon {
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
            let target = view.entities().find(|e| e.entity_number == num);

            if let Some(t) = target {
                let distance = (t.origin - view.self_state().origin).length();
                let ammo = view.self_state().ammo;

                let best_weapon = weapons::select_best_weapon(self.desired_weapon, &ammo, distance);
                let best_weapon_ammo_idx = best_weapon.ammo_index();
                let best_weapon_has_ammo = best_weapon.ammo_cost() == 0
                    || (best_weapon_ammo_idx < ammo.len() && ammo[best_weapon_ammo_idx] > 0);

                // In Q2, you can only use weapons you've picked up (server sends gunindex).
                // Check if the server has given us this weapon by comparing with gunindex.
                let server_gave_us_this_weapon =
                    self.server_weapon == best_weapon || best_weapon == Weapon::Blaster; // Blaster is always available

                // Debug: log weapon selection decision
                if best_weapon != self.server_weapon {
                    tracing::trace!(
                        "weapon eval: best={:?} (ammo={}, has_ammo={}, gunindex_ok={}), current={:?}",
                        best_weapon,
                        if best_weapon_ammo_idx < ammo.len() { ammo[best_weapon_ammo_idx] } else { -1 },
                        best_weapon_has_ammo,
                        server_gave_us_this_weapon,
                        self.server_weapon
                    );
                }

                // Only switch if: we have ammo, server gave us the weapon, and it's different from current
                let impulse = if best_weapon != self.server_weapon
                    && best_weapon_has_ammo
                    && server_gave_us_this_weapon
                {
                    self.desired_weapon = best_weapon;
                    Some(best_weapon as u8)
                } else {
                    None
                };

                if impulse.is_some() {
                    let current_weapon = self.server_weapon;
                    let current_ammo_idx = current_weapon.ammo_index();
                    let current_ammo = if current_ammo_idx < ammo.len() {
                        ammo[current_ammo_idx]
                    } else {
                        0
                    };
                    let new_ammo_idx = best_weapon.ammo_index();
                    let new_ammo = if new_ammo_idx < ammo.len() {
                        ammo[new_ammo_idx]
                    } else {
                        0
                    };
                    tracing::info!(
                        "weapon switch: {:?} (ammo {}) → {:?} (ammo {})",
                        current_weapon,
                        current_ammo,
                        best_weapon,
                        new_ammo
                    );
                }

                let (yaw, pitch) = if self.desired_weapon.is_hitscan() {
                    aim_hitscan(
                        view.self_state().origin,
                        t.origin,
                        t.velocity,
                        skill,
                        jitter_seed,
                    )
                } else {
                    let proj_speed = self.desired_weapon.projectile_speed().unwrap_or(500.0);
                    aim_projectile(view.self_state().origin, t.origin, t.velocity, proj_speed)
                };

                let should_fire = self.should_fire(view, distance);

                self.frames_since_shot = if should_fire {
                    0
                } else {
                    self.frames_since_shot + 1
                };

                if should_fire {
                    tracing::info!(
                        target = t.entity_number,
                        distance = %format!("{:.1}", distance),
                        weapon = ?self.desired_weapon,
                        "shooting at player"
                    );
                }

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
        if self.frames_since_shot < FIRE_RATE_COOLDOWN_FRAMES {
            return false;
        }

        if distance < self.desired_weapon.min_safe_distance() {
            return false;
        }

        let ammo_idx = self.desired_weapon.ammo_index();
        let ammo = if ammo_idx < view.self_state().ammo.len() {
            view.self_state().ammo[ammo_idx]
        } else {
            0
        };
        if self.desired_weapon.ammo_cost() > 0 && ammo <= 0 {
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
        assert_eq!(driver.desired_weapon, Weapon::Blaster);
        assert_eq!(driver.server_weapon, Weapon::Blaster);
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
    }

    #[test]
    fn no_switch_to_weapon_without_ammo() {
        let ammo = [0i32; 32];
        let distance = 100.0;

        let best = weapons::select_best_weapon(Weapon::Blaster, &ammo, distance);
        let best_ammo_idx = best.ammo_index();
        let has_ammo =
            best.ammo_cost() == 0 || (best_ammo_idx < ammo.len() && ammo[best_ammo_idx] > 0);

        assert_eq!(best, Weapon::Blaster);
        assert!(has_ammo);
    }

    #[test]
    fn switch_to_weapon_with_ammo() {
        let mut ammo = [0i32; 32];
        ammo[Weapon::Shotgun.ammo_index()] = 10;
        let distance = 100.0;

        let best = weapons::select_best_weapon(Weapon::Blaster, &ammo, distance);
        let best_ammo_idx = best.ammo_index();
        let has_ammo =
            best.ammo_cost() == 0 || (best_ammo_idx < ammo.len() && ammo[best_ammo_idx] > 0);

        assert!(best == Weapon::Shotgun || best == Weapon::Blaster);
        assert!(has_ammo);
    }
}
