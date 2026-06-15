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

use crate::aim::{aim_direction, aim_hitscan, JitterRng};
use crate::perception::{EntityClass, Worldview};
use crate::weapons::{self, Weapon};

/// Frames to hold on a target before considering a switch (~0.5s at 10 Hz),
/// preventing thrashing when enemies pop in/out of PVS. (Eraser "target stability".)
const TARGET_LOCK_FRAMES: u32 = 5;

/// Brain tick rate (Hz). Eraser's calibrated timings are in seconds; we convert
/// to frames at this cadence.
const TICK_HZ: f32 = 10.0;

/// `BOT_CHANGEWEAPON_DELAY` — withhold `BUTTON_ATTACK` for 0.9 s after
/// requesting a weapon switch (Eraser `bot_procs.h`). Firing mid-switch is wasted.
const SWITCH_LOCKOUT_SECS: f32 = 0.9;

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
    /// Frames since we acquired the *current* target — drives Eraser's reaction
    /// delay (`SIGHT_FIRE_DELAY`). Reset when the target changes.
    sight_frames: u32,
    /// Frames since we requested a weapon switch — drives the 0.9 s attack lockout.
    frames_since_switch: u32,
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
            sight_frames: 0,
            frames_since_switch: u32::MAX,
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
        let prev_target = self.current_target;
        let target_num = self.select_target_entity(view);

        let Some(num) = target_num else {
            self.frames_since_shot += 1;
            self.frames_since_switch = self.frames_since_switch.saturating_add(1);
            return CombatDecision::default();
        };

        let Some(t) = view.entities().find(|e| e.entity_number == num) else {
            self.frames_since_shot += 1;
            self.frames_since_switch = self.frames_since_switch.saturating_add(1);
            return CombatDecision::default();
        };

        // New target acquired → reset the reaction-delay timer (Eraser reacquire).
        if Some(num) != prev_target {
            self.sight_frames = 0;
        }
        self.sight_frames = self.sight_frames.saturating_add(1);

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
            self.frames_since_switch = 0;
        } else {
            self.frames_since_switch = self.frames_since_switch.saturating_add(1);
        }

        let weapon = self.held_weapon;

        // `skill` here is a 0-1 jitter factor (1=max miss, 0=perfect). Map to
        // Eraser's 1-5 accuracy (5=perfect) and a 1-5 combat rating. Seed a
        // deterministic jitter RNG.
        let accuracy = (5.0 - skill * 4.0).clamp(1.0, 5.0);
        let combat = (5.0 - skill * 4.0).clamp(1.0, 5.0);
        let mut rng = JitterRng::new(jitter_seed.to_bits());

        let (yaw, pitch) = if weapon.is_hitscan() {
            aim_hitscan(
                view.self_state().origin,
                t.origin,
                t.velocity,
                weapon,
                accuracy,
                &mut rng,
            )
        } else {
            aim_direction(view.self_state().origin, t.origin, t.velocity, weapon)
        };

        let should_fire = self.should_fire(weapon, distance, combat);

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

    /// Eraser fire gate (`bot_Attack`, `bot_wpns.c:134-324`): fire iff cooldown
    /// elapsed AND reaction delay satisfied AND not in weapon-switch lockout AND
    /// a sane range. All timings are per-frame at [`TICK_HZ`].
    fn should_fire(&self, weapon: Weapon, distance: f32, combat: f32) -> bool {
        // 0.9 s switch lockout — firing mid-weapon-change is wasted.
        let switch_lockout_frames = (SWITCH_LOCKOUT_SECS * TICK_HZ).round() as u32;
        if self.frames_since_switch < switch_lockout_frames {
            return false;
        }

        // Reaction delay on target acquisition: `0.8 * (5 - combat*0.5)/5` s.
        // combat1 → 0.72 s, combat3 → 0.56 s, combat5 → 0.40 s.
        let reaction_secs = 0.8 * (5.0 - combat * 0.5) / 5.0;
        let reaction_frames = (reaction_secs * TICK_HZ).round() as u32;
        if self.sight_frames < reaction_frames {
            return false;
        }

        // Per-weapon fire interval (0 = every frame for CG/MG/HB).
        let interval_frames = (weapon.fire_interval_secs() * TICK_HZ).round() as u32;
        if self.frames_since_shot < interval_frames {
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

    /// A driver with all timing gates satisfied, so `should_fire` depends only
    /// on weapon + distance (range/safety). Each test then perturbs one gate.
    fn ready_driver() -> CombatDriver {
        let mut d = CombatDriver::new();
        d.sight_frames = 100; // past any reaction delay
        d.frames_since_switch = 100; // past switch lockout
        d.frames_since_shot = 100; // past any fire interval
        d
    }

    #[test]
    fn should_not_fire_blaster_across_map() {
        let driver = ready_driver();
        // Blaster effective range ~600; at 2000 it shouldn't fire.
        assert!(!driver.should_fire(Weapon::Blaster, 2000.0, 3.0));
        assert!(driver.should_fire(Weapon::Blaster, 200.0, 3.0));
    }

    #[test]
    fn should_not_fire_rocket_point_blank() {
        let driver = ready_driver();
        assert!(!driver.should_fire(Weapon::RocketLauncher, 50.0, 3.0));
        assert!(driver.should_fire(Weapon::RocketLauncher, 300.0, 3.0));
    }

    #[test]
    fn switch_lockout_suppresses_fire() {
        let mut driver = ready_driver();
        driver.frames_since_switch = 0; // just switched
                                        // Even with everything else satisfied, the 0.9 s lockout blocks fire.
        assert!(!driver.should_fire(Weapon::Railgun, 400.0, 5.0));
        driver.frames_since_switch = 9; // 0.9 s elapsed
        assert!(driver.should_fire(Weapon::Railgun, 400.0, 5.0));
    }

    #[test]
    fn per_weapon_fire_interval_enforced() {
        let mut driver = ready_driver();
        // Railgun interval = 1.5 s = 15 frames. Right after a shot it must wait.
        driver.frames_since_shot = 0;
        assert!(!driver.should_fire(Weapon::Railgun, 400.0, 5.0));
        driver.frames_since_shot = 15;
        assert!(driver.should_fire(Weapon::Railgun, 400.0, 5.0));
    }

    #[test]
    fn reaction_delay_blocks_immediate_fire() {
        let mut driver = ready_driver();
        // Fresh acquisition (combat 1 → ~0.72 s reaction ≈ 7 frames).
        driver.sight_frames = 1;
        assert!(!driver.should_fire(Weapon::Shotgun, 150.0, 1.0));
        driver.sight_frames = 8;
        assert!(driver.should_fire(Weapon::Shotgun, 150.0, 1.0));
    }
}
