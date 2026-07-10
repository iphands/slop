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
use crate::skill::BotSkill;
use crate::weapons::{self, Weapon};
use world::CollisionModel;

/// Frames to hold on a target before considering a switch (~0.5s at 10 Hz),
/// preventing thrashing when enemies pop in/out of PVS. (Eraser "target stability".)
const TARGET_LOCK_FRAMES: u32 = 5;

/// Frames to keep firing at a target after LOS drops (Eraser `last_enemy_sight` gate,
/// Plan 11 T3). 2 frames ≈ 0.2 s at 10 Hz — enough to not flicker on thin pillars.
const SIGHT_GRACE_FRAMES: u32 = 2;

/// Brain tick rate (Hz). Eraser's calibrated timings are in seconds; we convert
/// to frames at this cadence.
const TICK_HZ: f32 = 10.0;

/// Withhold `BUTTON_ATTACK` briefly after requesting a weapon switch so we don't fire the
/// old weapon mid-change. Eraser used a full `0.9 s` (`BOT_CHANGEWEAPON_DELAY`), but that —
/// stacked on the reaction delay — left `main` idle >1 s at the start of every engagement
/// while `q3` (0.1 s lockout) shot first and won the duel (Plan 45). Trimmed to a realistic
/// switch time; the server still ignores fire during the actual change.
const SWITCH_LOCKOUT_SECS: f32 = 0.2;

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
    /// Frames of LOS grace remaining on the current target (Plan 11 T3).
    /// Set to `SIGHT_GRACE_FRAMES` on fresh selection or while LOS holds;
    /// decremented each tick after LOS is lost; target is dropped when it reaches 0.
    sight_grace_remaining: u32,
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
            sight_grace_remaining: 0,
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

    /// Evaluate combat state and produce a decision. `skill` drives aim jitter
    /// (accuracy rating), reaction delay, and combat gating. `los` is the collision
    /// model for line-of-sight gating (Plan 11); when `None` (geometry not loaded —
    /// e.g. before the nav graph builds, or in unit tests) targeting degrades to
    /// FOV-only, which the recorder's `phantom_target` flag will surface.
    pub fn evaluate(
        &mut self,
        view: &Worldview,
        skill: &BotSkill,
        jitter_seed: f32,
        los: Option<&CollisionModel>,
    ) -> CombatDecision {
        let prev_target = self.current_target;
        let (target_num, fire_allowed) = self.select_target_entity(view, los);

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
        // differs from what we're holding. A dry held weapon (0 ammo) forces a fallback (Plan 30 T4).
        let desired =
            weapons::select_best_weapon(self.held_weapon, distance, view.self_state().held_ammo());
        let weapon_request = (desired != self.held_weapon).then(|| {
            self.held_weapon = desired;
            // EVT counter (Plan 47 T1): weapon switch + the engagement range that drove it —
            // greppable proof of "switches weapons for close/far combat".
            tracing::info!(
                weapon = desired.name(),
                dist = distance as i32,
                "EVT switch"
            );
            WeaponRequest(desired)
        });

        if let Some(WeaponRequest(w)) = weapon_request {
            tracing::info!(weapon = %w.name(), distance = %format!("{:.0}", distance), "requesting weapon");
            self.frames_since_switch = 0;
        } else {
            self.frames_since_switch = self.frames_since_switch.saturating_add(1);
        }

        let weapon = self.held_weapon;

        // `skill` provides Eraser's accuracy/combat ratings (1-5, adjusted to the
        // bot's level). Seed a deterministic jitter RNG.
        let accuracy = skill.accuracy();
        let combat = skill.combat();
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

        // Gate `should_fire` on both timing gates AND current LOS (or grace period).
        let should_fire = fire_allowed && self.should_fire(weapon, distance, combat);

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

    /// Returns `(target_entity_num, fire_allowed)`.
    /// `fire_allowed=false` on stale-only targets so the caller never fires at
    /// an entity with no confirmed LOS (Plan 11 T3).
    fn select_target_entity(
        &mut self,
        view: &Worldview,
        los: Option<&CollisionModel>,
    ) -> (Option<i32>, bool) {
        if let Some(target_num) = self.current_target {
            if self.lock_frames_remaining > 0
                && view.enemies().any(|e| e.entity_number == target_num)
            {
                // LOS + grace check on the locked target (Plan 11 T3).
                if let Some(cm) = los {
                    if let Some(target) = view.entities().find(|e| e.entity_number == target_num) {
                        let eye = crate::los::eye_origin(view.self_state().origin.into());
                        if crate::los::has_los_player(cm, eye, target.origin.into()) {
                            // LOS holds: refresh grace.
                            self.sight_grace_remaining = SIGHT_GRACE_FRAMES;
                        } else if self.sight_grace_remaining == 0 {
                            // Grace expired: force-drop the target.
                            self.current_target = None;
                            self.lock_frames_remaining = 0;
                            // Fall through to fresh selection below.
                        } else {
                            // Grace period: keep target one more frame.
                            self.sight_grace_remaining -= 1;
                            self.lock_frames_remaining -= 1;
                            return (Some(target_num), true);
                        }
                    }
                }
                // Still locked (LOS holds or no cm for geometry).
                if self.current_target.is_some() {
                    self.lock_frames_remaining -= 1;
                    return (Some(target_num), true);
                }
            }
        }

        // Fresh selection: trace-gated when geometry is available; else FOV-only.
        let nearest = match los {
            Some(cm) => view.nearest_visible_enemy(cm, 90.0),
            None => view.nearest_enemy(90.0),
        };
        if let Some(t) = nearest {
            self.current_target = Some(t.entity_number);
            self.lock_frames_remaining = TARGET_LOCK_FRAMES;
            self.sight_grace_remaining = SIGHT_GRACE_FRAMES;
            return (Some(t.entity_number), true);
        }

        // Stale fallback: navigate to last-known pos but do not fire (no LOS).
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
            return (Some(t.entity_number), false);
        }

        (None, false)
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

        // Reaction delay on target acquisition. Eraser's `0.8 * (5 - combat*0.5)/5` floored a
        // skilled bot at 0.40 s — slower than `q3` major's 0.30 s, so `main` always shot
        // second and lost the duel (Plan 45). Halved the base so a high-combat `main` reacts
        // in ~0.20 s (still a human-ish delay, now faster than the opponent).
        // combat1 → 0.36 s, combat3 → 0.28 s, combat5 → 0.20 s.
        let reaction_secs = 0.4 * (5.0 - combat * 0.5) / 5.0;
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

    /// Drop the current target (e.g. after the give-up watchdog abandons a stale
    /// chase). Next tick re-selects a fresh target if one is visible.
    pub fn clear_target(&mut self) {
        self.current_target = None;
        self.lock_frames_remaining = 0;
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
        let decision = driver.evaluate(&view, &BotSkill::default(), 0.0, None);
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

    /// Plan 11 T3: when LOS drops the target is kept for exactly `SIGHT_GRACE_FRAMES`
    /// frames (fire allowed during grace), then dropped — bot doesn't fire through walls.
    #[test]
    fn sight_grace_allows_fire_then_drops_after_n_frames() {
        use client::parse::ConfigStrings;
        use q2proto::{EntityState, Frame};

        // Wall at x=0 (x<0 solid). Bot at (100,0,0), enemy behind wall at (-50,0,0).
        let cm = world::CollisionModel::half_space([1.0, 0.0, 0.0], 0.0);
        let mut frame = Frame::default();
        frame.playerstate.pmove.origin = [(100.0 * 8.0) as i16, 0, 0];
        frame.entities = vec![EntityState {
            number: 5,
            origin: [-50.0, 0.0, 0.0], // behind wall — no LOS
            modelindex: 255,
            ..Default::default()
        }];
        let cs = ConfigStrings::default();
        let view = crate::perception::Worldview::from_frame(&frame, &cs, 0);

        let mut driver = ready_driver();
        // Prime: pretend the target was freshly acquired last tick with LOS.
        driver.current_target = Some(5);
        driver.lock_frames_remaining = 10;
        driver.sight_grace_remaining = SIGHT_GRACE_FRAMES; // 2

        // Grace frame 1: LOS absent, grace 2→1 — target kept.
        let dec1 = driver.evaluate(&view, &BotSkill::default(), 0.0, Some(&cm));
        assert_eq!(
            dec1.target_entity,
            Some(5),
            "target kept during grace frame 1"
        );

        // Grace frame 2: LOS absent, grace 1→0 — target kept (last grace tick).
        let dec2 = driver.evaluate(&view, &BotSkill::default(), 0.0, Some(&cm));
        assert_eq!(
            dec2.target_entity,
            Some(5),
            "target kept during grace frame 2"
        );

        // Frame 3: grace=0, LOS still absent — target dropped.
        let dec3 = driver.evaluate(&view, &BotSkill::default(), 0.0, Some(&cm));
        assert!(
            dec3.target_entity.is_none(),
            "target dropped after grace expires"
        );
        assert!(!dec3.should_fire, "no fire after target dropped");
    }
}
