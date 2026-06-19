//! # brain::brains::q3::aim — the Quake 3 aim + fire model (Plan 37 T4/T5)
//!
//! Ports `BotAimAtEnemy` (`ai_dmq3.c:3261`, distilled §5) and `BotCheckAttack`
//! (`ai_dmq3.c:3555`, distilled §6). Distinct from the Eraser aim in [`crate::aim`]:
//! **per-weapon accuracy/skill**, a **reaction-time sight gate**, a **direction-change accuracy
//! penalty**, **hitscan distance falloff**, **radial ground-aim** for splash weapons, and the
//! **fire-throttle duty cycle** + **radial self-preservation abort**.
//!
//! AAS's exact movement prediction (`trap_AAS_PredictClientMovement`) has no qbots equivalent;
//! we substitute the shared **constant-velocity lead** ([`crate::aim::aim_direction`]) — the
//! exact-predict path was only for `aim_skill > 0.8`, so a high-skill bot just gets a better
//! linear lead. The aim-error model is applied on top.

use glam::Vec3;
use world::{CollisionModel, MASK_SOLID};

use crate::aim::{aim_direction, AimRng, PITCH_CLAMP_DEG};
use crate::q3char::Q3Character;
use crate::weapons::Weapon;

/// Enemy velocity memory for the direction-change accuracy penalty (`enemyposition_time`,
/// sampled every 0.5 s). When the enemy reverses direction between samples, a sub-0.9-skill bot
/// gets faked out (`accuracy *= 0.7`).
#[derive(Debug, Clone, Copy)]
pub struct AimState {
    sample_time: f32,
    last_vel: Vec3,
    dir_changed: bool,
}

impl AimState {
    pub fn new() -> Self {
        Self {
            sample_time: f32::NEG_INFINITY,
            last_vel: Vec3::ZERO,
            dir_changed: false,
        }
    }

    /// Re-sample the enemy velocity every 0.5 s; flag a direction reversal for sub-0.9 skill.
    fn update(&mut self, time: f32, enemy_vel: Vec3, aim_skill: f32) {
        if time - self.sample_time >= 0.5 {
            let old = self.last_vel;
            self.dir_changed = aim_skill < 0.9
                && old.length_squared() > 1.0
                && enemy_vel.length_squared() > 1.0
                && old.dot(enemy_vel) < 0.0;
            self.last_vel = enemy_vel;
            self.sample_time = time;
        }
    }
}

impl Default for AimState {
    fn default() -> Self {
        Self::new()
    }
}

/// All the per-tick aim inputs (keeps the function signature legible).
pub struct AimInput<'a> {
    pub ch: &'a Q3Character,
    pub weapon: Weapon,
    /// Our eye origin (trace/aim start).
    pub shooter_eye: Vec3,
    pub enemy_origin: Vec3,
    pub enemy_vel: Option<Vec3>,
    /// Seconds the enemy has been continuously sighted (reaction-time gate input).
    pub sighted_secs: f32,
    /// Do we currently have LOS to the enemy?
    pub visible: bool,
    /// Brain wall-clock seconds (for the 0.5 s velocity-memory sampling).
    pub time: f32,
    pub cm: Option<&'a CollisionModel>,
}

/// The Q3 aim result.
#[derive(Debug, Clone, Copy)]
pub struct AimResult {
    pub yaw: f32,
    pub pitch: f32,
    /// `false` while a high-skill bot is still inside its reaction-time delay (don't fire yet).
    pub ready: bool,
}

/// `BotAimAtEnemy` (`ai_dmq3.c:3261`). Returns the ideal view `(yaw, pitch)` plus a `ready`
/// flag (the reaction-time gate). `rng` supplies the deterministic aim-error jitter.
pub fn aim_at_enemy(state: &mut AimState, input: &AimInput, rng: &mut impl AimRng) -> AimResult {
    let AimInput {
        ch,
        weapon,
        shooter_eye,
        enemy_origin,
        enemy_vel,
        sighted_secs,
        visible,
        time,
        cm,
    } = *input;

    let mut accuracy = ch.weapon_accuracy(weapon).clamp(0.0, 1.0);
    let skill = ch.aim_skill;
    let evel = enemy_vel.unwrap_or(Vec3::ZERO);

    // Reaction gate: very precise bots refuse to aim until the enemy's been seen long enough
    // (`aim_skill > 0.95`); lower-skill bots aim immediately (but inaccurately).
    let ready = if skill > 0.95 {
        sighted_secs > 0.5 * ch.reaction_time
    } else {
        true
    };

    // Velocity memory + direction-change penalty.
    state.update(time, evel, skill);
    if !visible {
        accuracy *= 0.4; // shooting at a guessed position
    }
    if state.dir_changed {
        accuracy *= 0.7; // got faked out by a direction change
    }

    let dist = (enemy_origin - shooter_eye).length();
    // Hitscan distance falloff: more accurate up close.
    if weapon.is_hitscan() {
        accuracy *= 0.6 + (dist.min(150.0) / 150.0) * 0.4;
    }

    // Radial ground-aim for splash weapons (`aim_skill > 0.6`, enemy not far above us): aim at
    // the floor under the enemy so the splash still hits.
    let mut target = enemy_origin;
    let radial = skill > 0.6 && weapon.self_dangerous() && (enemy_origin.z - shooter_eye.z) < 16.0;
    if radial {
        if let Some(cm) = cm {
            if let Some(floor) = trace_floor(cm, enemy_origin) {
                target = floor;
            }
        }
    }

    // Worldspace aim jitter (Q3 `bestorigin += 20·crandom·(1−accuracy)` on x,y; 10· on z).
    let inacc = 1.0 - accuracy;
    target += Vec3::new(
        rng.next_signed() * 20.0 * inacc,
        rng.next_signed() * 20.0 * inacc,
        rng.next_signed() * 10.0 * inacc,
    );

    // Lead via the shared per-weapon constant-velocity model (hitscan ignores lead internally).
    let lead_vel = if weapon.projectile_speed().is_some() {
        enemy_vel
    } else {
        None
    };
    let (mut yaw, mut pitch) = aim_direction(shooter_eye, target, lead_vel, weapon);

    // Direction perturbation when inaccurate (Q3 `0.3·crandom·(1−accuracy)` per axis, radians).
    if accuracy < 0.8 {
        yaw += (rng.next_signed() * 0.3 * inacc).to_degrees();
        pitch += (rng.next_signed() * 0.15 * inacc).to_degrees();
    }

    AimResult {
        yaw,
        pitch: pitch.clamp(-PITCH_CLAMP_DEG, PITCH_CLAMP_DEG),
        ready,
    }
}

/// Trace straight down from just above an origin to find the floor point under it (for radial
/// ground-aim / self-preservation splash checks). Returns `None` if no floor within 128 u.
fn trace_floor(cm: &CollisionModel, origin: Vec3) -> Option<Vec3> {
    let start = [origin.x, origin.y, origin.z + 8.0];
    let end = [origin.x, origin.y, origin.z - 128.0];
    let t = cm.trace(&start, &end, &[0.0; 3], &[0.0; 3], MASK_SOLID);
    if t.fraction < 1.0 && !t.startsolid {
        Some(Vec3::from(t.endpos))
    } else {
        None
    }
}

/// Would firing a **splash** weapon at `aim_target` splash *us*? Traces eye→aim_target; if the
/// world is hit short of the target and the impact is within the weapon's blast radius of our
/// own feet, a self-preservation-minded bot holds fire (`BotCheckAttack` radial check, §6).
/// Non-splash weapons never self-abort.
pub fn would_self_splash(
    cm: &CollisionModel,
    shooter_eye: Vec3,
    self_origin: Vec3,
    aim_target: Vec3,
    weapon: Weapon,
) -> bool {
    if !weapon.self_dangerous() {
        return false;
    }
    let start = [shooter_eye.x, shooter_eye.y, shooter_eye.z];
    let end = [aim_target.x, aim_target.y, aim_target.z];
    let t = cm.trace(&start, &end, &[0.0; 3], &[0.0; 3], MASK_SOLID);
    if t.fraction >= 1.0 {
        return false; // nothing in the way — shot reaches the target
    }
    let impact = Vec3::from(t.endpos);
    (impact - self_origin).length() < weapon.min_safe_distance()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aim::JitterRng;

    fn precise_input(weapon: Weapon) -> (AimState, AimInput<'static>) {
        // Leak a character so the input can borrow it for 'static in these simple tests.
        let ch: &'static Q3Character = Box::leak(Box::new(Q3Character::major()));
        (
            AimState::new(),
            AimInput {
                ch,
                weapon,
                shooter_eye: Vec3::new(0.0, 0.0, 22.0),
                enemy_origin: Vec3::new(500.0, 0.0, 0.0),
                enemy_vel: None,
                sighted_secs: 5.0,
                visible: true,
                time: 1.0,
                cm: None,
            },
        )
    }

    #[test]
    fn precise_aim_points_at_target() {
        let (mut st, input) = precise_input(Weapon::Railgun);
        let mut rng = JitterRng::new(1);
        let r = aim_at_enemy(&mut st, &input, &mut rng);
        // Major has high per-weapon accuracy → near-zero jitter → aim ~straight ahead (+x, yaw 0).
        assert!(r.yaw.abs() < 5.0, "precise aim near 0° yaw, got {}", r.yaw);
        assert!(r.pitch.abs() <= PITCH_CLAMP_DEG + 0.01);
        assert!(r.ready, "long-sighted precise bot is ready to fire");
    }

    #[test]
    fn reaction_gate_blocks_early_fire_for_precise_bot() {
        let (mut st, mut input) = precise_input(Weapon::Railgun);
        input.sighted_secs = 0.0; // just sighted
        let mut rng = JitterRng::new(2);
        let r = aim_at_enemy(&mut st, &input, &mut rng);
        // Major aim_skill 0.9 → not >0.95, so ready stays true. Bump skill to force the gate.
        let crack: &'static Q3Character = Box::leak(Box::new(Q3Character {
            aim_skill: 0.99,
            reaction_time: 0.4,
            ..Q3Character::major()
        }));
        input.ch = crack;
        let r2 = aim_at_enemy(&mut st, &input, &mut rng);
        assert!(r.ready); // 0.9 skill aims immediately
        assert!(!r2.ready, "0.99-skill bot waits out its reaction time");
    }

    #[test]
    fn low_accuracy_adds_spread() {
        let sprayer: &'static Q3Character = Box::leak(Box::new(Q3Character {
            aim_accuracy: 0.2,
            aim_skill: 0.3,
            ..Q3Character::grunt()
        }));
        let mut st = AimState::new();
        let input = AimInput {
            ch: sprayer,
            weapon: Weapon::Machinegun,
            shooter_eye: Vec3::new(0.0, 0.0, 22.0),
            enemy_origin: Vec3::new(500.0, 0.0, 0.0),
            enemy_vel: None,
            sighted_secs: 5.0,
            visible: true,
            time: 1.0,
            cm: None,
        };
        let mut rng = JitterRng::new(7);
        // Average several rolls: a low-accuracy bot should miss dead-center most of the time.
        let mut max_dev: f32 = 0.0;
        for _ in 0..16 {
            let r = aim_at_enemy(&mut st, &input, &mut rng);
            max_dev = max_dev.max(r.yaw.abs());
        }
        assert!(
            max_dev > 1.0,
            "spray bot deviates from center, got {max_dev}"
        );
    }

    #[test]
    fn non_splash_never_self_aborts() {
        let cm = CollisionModel::half_space([1.0, 0.0, 0.0], 0.0);
        assert!(!would_self_splash(
            &cm,
            Vec3::new(50.0, 0.0, 22.0),
            Vec3::new(50.0, 0.0, 0.0),
            Vec3::new(100.0, 0.0, 0.0),
            Weapon::Railgun
        ));
    }

    #[test]
    fn rocket_into_near_wall_self_aborts() {
        // Wall at x=0 (x<0 solid). Shooter just in front, aiming at a point across the wall →
        // the trace stops at the wall ~right at our feet → within rocket blast radius → abort.
        let cm = CollisionModel::half_space([1.0, 0.0, 0.0], 0.0);
        let eye = Vec3::new(10.0, 0.0, 22.0);
        let self_origin = Vec3::new(10.0, 0.0, 0.0);
        let aim = Vec3::new(-100.0, 0.0, 0.0); // behind the wall
        assert!(would_self_splash(
            &cm,
            eye,
            self_origin,
            aim,
            Weapon::RocketLauncher
        ));
    }
}
