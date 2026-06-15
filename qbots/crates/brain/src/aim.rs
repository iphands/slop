//! Aim math — pure functions for computing view angles.
//!
//! Ports the lead-aim and jitter algorithms from 3ZB2 `fire.c:11`.
//! Q2 projectiles travel at constant velocity (no gravity), so single-pass
//! velocity prediction is sufficient.

use glam::Vec3;

/// Q2 weapon projectile speeds (from `g_weapon.c` / `shared.h`).
pub const RL_SPEED: f32 = 1200.0;
pub const GL_SPEED: f32 = 750.0;
pub const BLASTER_SPEED: f32 = 500.0;
pub const NAIL_SPEED: f32 = 2000.0;
pub const ROGUE_SPEED: f32 = 1000.0;

/// Aim at a target for a hitscan weapon (instant hit).
/// Returns (yaw, pitch) in degrees.
/// `skill` ranges 0.0 (terrible) to 1.0 (perfect).
/// Lower skill = more jitter.
pub fn aim_hitscan(
    shooter_origin: Vec3,
    target_origin: Vec3,
    _target_velocity: Option<Vec3>,
    skill: f32,
    jitter_seed: f32,
) -> (f32, f32) {
    let mut aim_point = target_origin;

    let jitter_amount = (1.0 - skill) * 8.0;
    if jitter_amount > 0.0 {
        let jitter_x = (jitter_seed * 12.9898 + 78.233).fract() * std::f32::consts::TAU;
        let jitter_y = ((jitter_seed + 1.0) * 12.9898 + 78.233).fract() * std::f32::consts::TAU;
        aim_point += Vec3::new(
            jitter_x.sin() * jitter_amount,
            jitter_y.sin() * jitter_amount,
            ((jitter_seed + 2.0) * 12.9898 + 78.233).fract() * jitter_amount * 0.5,
        );
    }

    let direction = aim_point - shooter_origin;
    vec3_to_angles(direction)
}

/// Aim at a moving target for a projectile weapon.
/// Predicts where the target will be when the projectile arrives.
/// Single-pass prediction (sufficient for Q2's constant-velocity projectiles).
pub fn aim_projectile(
    shooter_origin: Vec3,
    target_origin: Vec3,
    target_velocity: Option<Vec3>,
    projectile_speed: f32,
) -> (f32, f32) {
    let to_target = target_origin - shooter_origin;
    let dist = to_target.length();
    let time_of_flight = dist / projectile_speed;

    // Predict target position at impact time
    let predicted = target_origin + target_velocity.unwrap_or(Vec3::ZERO) * time_of_flight;

    let direction = predicted - shooter_origin;
    vec3_to_angles(direction)
}

/// Convert a direction vector to Q2 view angles (yaw, pitch) in degrees.
fn vec3_to_angles(dir: Vec3) -> (f32, f32) {
    let yaw = if dir.x == 0.0 && dir.y == 0.0 {
        0.0
    } else {
        dir.y.atan2(dir.x).to_degrees()
    };
    let pitch = (-dir.z).atan2(dir.x.hypot(dir.y)).to_degrees();
    (yaw, pitch.clamp(-89.0, 89.0))
}

/// Check if a direction is within a cone (for FOV checks).
pub fn in_fov(forward: Vec3, direction: Vec3, fov_degrees: f32) -> bool {
    let fov_radians = fov_degrees.to_radians();
    forward.dot(direction.normalize()) > fov_radians.cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aim_hitscan_stationary_target() {
        let shooter = Vec3::new(0.0, 0.0, 0.0);
        let target = Vec3::new(100.0, 0.0, 0.0);
        let (yaw, pitch) = aim_hitscan(shooter, target, None, 1.0, 0.0);
        assert!((yaw - 0.0).abs() < 0.1, "yaw should be ~0°");
        assert!(pitch.abs() < 0.1, "pitch should be ~0°");
    }

    #[test]
    fn aim_hitscan_target_above() {
        let shooter = Vec3::new(0.0, 0.0, 0.0);
        let target = Vec3::new(100.0, 0.0, 100.0);
        let (yaw, pitch) = aim_hitscan(shooter, target, None, 1.0, 0.0);
        assert!((yaw - 0.0).abs() < 0.1);
        // Positive Z = above; Q2 pitch: positive looks down, so target above = negative pitch
        assert!((pitch - (-45.0)).abs() < 1.0, "pitch should be ~-45°");
    }

    #[test]
    fn aim_projectile_leads_moving_target() {
        let shooter = Vec3::new(0.0, 0.0, 0.0);
        let target = Vec3::new(600.0, 0.0, 0.0);
        let vel = Some(Vec3::new(0.0, 200.0, 0.0)); // Moving sideways
        let (yaw, pitch) = aim_projectile(shooter, target, vel, RL_SPEED);
        // Should lead slightly in +y direction
        assert!(pitch.abs() < 1.0, "pitch should be ~0°");
        // Yaw should still be close to 0 for small lead
        assert!(yaw.abs() < 10.0, "yaw should be small positive");
    }

    #[test]
    fn aim_projectile_no_lead_for_stationary() {
        let shooter = Vec3::new(0.0, 0.0, 0.0);
        let target = Vec3::new(600.0, 0.0, 0.0);
        let (yaw, pitch) = aim_projectile(shooter, target, None, RL_SPEED);
        assert!((yaw - 0.0).abs() < 0.1);
        assert!(pitch.abs() < 0.1);
    }

    #[test]
    fn in_fov_center() {
        let forward = Vec3::new(1.0, 0.0, 0.0);
        let direction = Vec3::new(1.0, 0.0, 0.0);
        assert!(in_fov(forward, direction, 90.0));
    }

    #[test]
    fn in_fov_edge() {
        let forward = Vec3::new(1.0, 0.0, 0.0);
        let direction = Vec3::new(0.0, 1.0, 0.0); // 90° to the right
        assert!(!in_fov(forward, direction, 89.0));
        assert!(in_fov(forward, direction, 90.0));
    }
}
