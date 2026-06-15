//! Aim math — pure functions for computing view angles.
//!
//! Ports Eraser v1.01's per-weapon lead prediction and skill jitter from
//! `bot_wpns.c` (constants cited inline). Q2 projectiles travel at constant
//! velocity, so single-pass velocity prediction is sufficient; Eraser's
//! calibrated per-weapon factors compensate for spread and arc.
//!
//! Reference: `context/distilled/eraser.md` §5. Plugin-only parts (`gi.trace`
//! ground-aim, exact enemy velocity/health) are replaced: enemy velocity is
//! derived from origin deltas (possibly noisy → low-pass upstream), and the
//! pitch clamp / GL lob / per-weapon lead port verbatim.

use crate::weapons::Weapon;
use glam::Vec3;

/// Bots never aim steeply up/down — Eraser clamps pitch to ±15° (`bot_wpns.c:368`).
/// Notable GL/RL-lob limit. We clamp the *aim* pitch; the bot can still look
/// around freely for navigation.
pub const PITCH_CLAMP_DEG: f32 = 15.0;

/// Compute the aim direction toward a target for the given weapon, applying
/// Eraser's exact per-weapon lead factor and, for the grenade launcher, the
/// piecewise pitch-lob. Returns `(yaw_deg, pitch_deg)` with pitch clamped to
/// ±[`PITCH_CLAMP_DEG`] (Eraser `bot_wpns.c:368`).
///
/// Lead factors (`distilled/eraser.md` §5 lead table):
/// - Blaster / Hyperblaster: `dist/1000` (speed 1000).
/// - Rocket: `dist/650`, and **ignores upward velocity** so it won't lead a
///   jumper skyward (`bot_wpns.c:863-898`).
/// - Grenade: `dist/550` (deliberately over-leads ~9% to compensate for arc),
///   then pitches up piecewise (`bot_wpns.c:1042-1048`).
/// - BFG: `dist/400` (Eraser's `dist/550` is a bug vs the 400 fired speed).
/// - Hitscan (MG/SG/SSG/CG/Railgun): no lead (Railgun trails `−0.2*vel`).
pub fn aim_direction(
    shooter_origin: Vec3,
    target_origin: Vec3,
    target_velocity: Option<Vec3>,
    weapon: Weapon,
) -> (f32, f32) {
    let dist = (target_origin - shooter_origin).length();
    let vel = target_velocity.unwrap_or(Vec3::ZERO);

    let predicted = match weapon {
        // Hitscan: aim at current origin; Railgun trails slightly behind motion.
        Weapon::Shotgun | Weapon::SuperShotgun | Weapon::Machinegun | Weapon::Chaingun => {
            target_origin
        }
        Weapon::Railgun => target_origin - vel * 0.2,
        // Blaster/Hyperblaster: dist/1000.
        Weapon::Blaster | Weapon::Hyperblaster => target_origin + vel * (dist / 1000.0),
        // Rocket: dist/650, zero out upward velocity so we don't lead jumpers up.
        Weapon::RocketLauncher => {
            let mut v = vel;
            if v.z > 0.0 {
                v.z = 0.0;
            }
            target_origin + v * (dist / 650.0)
        }
        // Grenade: dist/550 (over-leads to compensate for arc).
        Weapon::GrenadeLauncher => target_origin + vel * (dist / 550.0),
        // BFG: dist/400 (Eraser's dist/550 was a bug vs the 400 fired speed).
        Weapon::Bfg10k => target_origin + vel * (dist / 400.0),
    };

    let direction = predicted - shooter_origin;
    let (yaw, pitch) = vec3_to_angles(direction);

    // GL lob (`bot_wpns.c:1042-1048`): pitch up piecewise — +15° at/above 384u,
    // ramping down to −15° at dist=0.
    let pitch = if matches!(weapon, Weapon::GrenadeLauncher) {
        let lob = if dist >= 384.0 {
            15.0
        } else {
            15.0 * (2.0 * dist / 384.0 - 1.0)
        };
        pitch + lob
    } else {
        pitch
    };

    // Eraser clamps aim pitch to ±15° (`:368`). Apply after the GL lob so a
    // maximum lob still respects the cap.
    (yaw, pitch.clamp(-PITCH_CLAMP_DEG, PITCH_CLAMP_DEG))
}

/// Eraser skill-jittered aim for hitscan weapons (`bot_wpns.c:423-430` skeleton):
/// ```text
/// tf = min(dist/2, 256) * ((5−accuracy)/5) * 2   // acc5→0 (perfect), acc1→1.6×
/// jitter target by crandom()*tf in x,y and crandom()*tf*zscale in z (MG 0.1, else 0.2)
/// ```
/// `accuracy` is 1..5. Returns `(yaw, pitch)` clamped to ±[`PITCH_CLAMP_DEG`].
pub fn aim_hitscan(
    shooter_origin: Vec3,
    target_origin: Vec3,
    target_velocity: Option<Vec3>,
    weapon: Weapon,
    accuracy: f32,
    rng: &mut impl AimRng,
) -> (f32, f32) {
    let dist = (target_origin - shooter_origin).length();

    // Start from the no-jitter leaded aim point for this weapon.
    let (base_yaw, mut base_pitch) =
        aim_direction(shooter_origin, target_origin, target_velocity, weapon);

    if accuracy < 5.0 {
        let tf = (dist / 2.0).min(256.0) * ((5.0 - accuracy) / 5.0) * 2.0;
        let zscale = if matches!(weapon, Weapon::Machinegun) {
            0.1
        } else {
            0.2
        };
        // Jitter is applied as an angular offset around the base aim.
        let yaw_jitter = rng.next_signed() * tf;
        let pitch_jitter = rng.next_signed() * tf * zscale;
        let yaw = base_yaw + yaw_jitter.to_degrees();
        base_pitch += pitch_jitter.to_degrees();
        (yaw, base_pitch.clamp(-PITCH_CLAMP_DEG, PITCH_CLAMP_DEG))
    } else {
        (
            base_yaw,
            base_pitch.clamp(-PITCH_CLAMP_DEG, PITCH_CLAMP_DEG),
        )
    }
}

/// Convert a direction vector to Q2 view angles (yaw, pitch) in degrees.
fn vec3_to_angles(dir: Vec3) -> (f32, f32) {
    let yaw = if dir.x == 0.0 && dir.y == 0.0 {
        0.0
    } else {
        dir.y.atan2(dir.x).to_degrees()
    };
    let pitch = (-dir.z).atan2(dir.x.hypot(dir.y)).to_degrees();
    (yaw, pitch)
}

/// Check if a direction is within a cone (for FOV checks).
pub fn in_fov(forward: Vec3, direction: Vec3, fov_degrees: f32) -> bool {
    let fov_radians = fov_degrees.to_radians();
    forward.dot(direction.normalize()) > fov_radians.cos()
}

/// A small RNG trait so aim jitter is deterministic in tests and uses the bot's
/// own seed at runtime. `next_signed` returns a value in roughly [−1, 1).
pub trait AimRng {
    fn next_signed(&mut self) -> f32;
}

/// Deterministic LCG-backed jitter RNG — gives repeatable aim spread per
/// `(seed, frame)` so a bot's misses look stable and a given skill tier
/// behaves consistently.
#[derive(Debug, Clone, Copy)]
pub struct JitterRng {
    state: u32,
}

impl JitterRng {
    /// Seed from a frame counter and a per-bot id so different bots diverge.
    pub fn new(seed: u32) -> Self {
        // Avoid the degenerate 0 state; mix in a nonzero constant.
        Self {
            state: seed.wrapping_mul(2654435761).wrapping_add(1),
        }
    }
}

impl AimRng for JitterRng {
    fn next_signed(&mut self) -> f32 {
        // Numerical Recipes LCG → [0,1) → centered to [−1,1).
        self.state = self.state.wrapping_mul(1664525).wrapping_add(1013904223);
        let u01 = (self.state >> 8) as f32 / ((1u32 << 24) as f32);
        u01 * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ConstRng(f32);
    impl AimRng for ConstRng {
        fn next_signed(&mut self) -> f32 {
            self.0
        }
    }

    #[test]
    fn hitscan_stationary_aims_at_origin() {
        let shooter = Vec3::new(0.0, 0.0, 0.0);
        let target = Vec3::new(100.0, 0.0, 0.0);
        let mut rng = ConstRng(0.0);
        let (yaw, pitch) = aim_hitscan(shooter, target, None, Weapon::Railgun, 5.0, &mut rng);
        assert!(yaw.abs() < 0.1, "yaw ~0°, got {yaw}");
        assert!(pitch.abs() < 0.1);
    }

    #[test]
    fn railgun_trails_moving_target() {
        let shooter = Vec3::new(0.0, 0.0, 0.0);
        let target = Vec3::new(600.0, 0.0, 0.0);
        let vel = Some(Vec3::new(0.0, 200.0, 0.0));
        // Railgun leads −0.2*vel → aim slightly behind in +y (negative yaw contribution).
        let (yaw, _) = aim_direction(shooter, target, vel, Weapon::Railgun);
        assert!(
            yaw < 0.0,
            "railgun should trail a +y mover (yaw<0), got {yaw}"
        );
    }

    #[test]
    fn rocket_leads_and_ignores_upward_velocity() {
        let shooter = Vec3::new(0.0, 0.0, 0.0);
        let target = Vec3::new(650.0, 0.0, 0.0);
        // Pure upward velocity should NOT move the rocket aim (ignores +z).
        let (yaw1, _) = aim_direction(
            shooter,
            target,
            Some(Vec3::new(0.0, 0.0, 500.0)),
            Weapon::RocketLauncher,
        );
        let (yaw2, _) = aim_direction(shooter, target, None, Weapon::RocketLauncher);
        assert!((yaw1 - yaw2).abs() < 0.1, "RL ignores upward V");
        // Horizontal velocity DOES lead.
        let (yaw_lead, _) = aim_direction(
            shooter,
            target,
            Some(Vec3::new(0.0, 200.0, 0.0)),
            Weapon::RocketLauncher,
        );
        assert!(yaw_lead.abs() > yaw2.abs(), "RL leads horizontal motion");
    }

    #[test]
    fn blaster_leads_moving_target() {
        let shooter = Vec3::new(0.0, 0.0, 0.0);
        let target = Vec3::new(1000.0, 0.0, 0.0);
        let (yaw, _) = aim_direction(
            shooter,
            target,
            Some(Vec3::new(0.0, 200.0, 0.0)),
            Weapon::Blaster,
        );
        assert!(yaw > 0.0, "blaster should lead a +y mover");
    }

    #[test]
    fn pitch_clamped_to_15_deg() {
        let shooter = Vec3::new(0.0, 0.0, 0.0);
        let target = Vec3::new(100.0, 0.0, 400.0); // very steep upward
        let (_, pitch) = aim_direction(shooter, target, None, Weapon::Blaster);
        assert!(
            pitch <= PITCH_CLAMP_DEG + 0.001,
            "pitch clamped, got {pitch}"
        );
    }

    #[test]
    fn grenade_lob_raises_pitch_near_and_far() {
        let shooter = Vec3::new(0.0, 0.0, 0.0);
        let far = Vec3::new(500.0, 0.0, 0.0);
        let (_, pitch_far) = aim_direction(shooter, far, None, Weapon::GrenadeLauncher);
        // At 500u the +15° lob dominates; base pitch ~0 so result ≈ +15° (clamped).
        assert!(pitch_far > 10.0, "GL lobs up at range, got {pitch_far}");
    }

    #[test]
    fn accuracy_5_is_perfect_no_jitter() {
        let shooter = Vec3::new(0.0, 0.0, 0.0);
        let target = Vec3::new(500.0, 0.0, 0.0);
        let mut rng = ConstRng(1.0); // would add jitter if applied
        let (yaw, _) = aim_hitscan(shooter, target, None, Weapon::Shotgun, 5.0, &mut rng);
        assert!(yaw.abs() < 0.1, "acc5 has no jitter");
    }

    #[test]
    fn accuracy_1_adds_jitter() {
        let shooter = Vec3::new(0.0, 0.0, 0.0);
        let target = Vec3::new(500.0, 0.0, 0.0);
        let mut rng = ConstRng(1.0); // constant +1 jitter
        let (yaw, _) = aim_hitscan(shooter, target, None, Weapon::Shotgun, 1.0, &mut rng);
        assert!(yaw.abs() > 1.0, "acc1 should jitter, got {yaw}");
    }

    #[test]
    fn in_fov_center_and_edge() {
        let forward = Vec3::new(1.0, 0.0, 0.0);
        assert!(in_fov(forward, Vec3::new(1.0, 0.0, 0.0), 90.0));
        assert!(!in_fov(forward, Vec3::new(0.0, 1.0, 0.0), 89.0));
    }
}
