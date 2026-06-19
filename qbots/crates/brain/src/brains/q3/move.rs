//! # brain::brains::q3::move — Q3 combat movement (Plan 37 T5)
//!
//! Ports `BotAttackMove` (`ai_dmq3.c:2631`, distilled §6): circle-strafe perpendicular to the
//! enemy with a random strafe-direction flip cadence, an ideal-distance band the bot drifts
//! forward/back to hold, occasional random back-up, and jump/crouch dodges. The strafe flip and
//! dodge *rolls* come from the brain (`Q3Brain::roll`) so behavior stays deterministic in tests;
//! this module owns the geometry.

use glam::Vec3;

use crate::q3char::Q3Character;

/// Ideal stand-off distance and the ± band around it the bot tolerates before
/// closing/opening the gap (Q3 `IDEAL_ATTACKDIST` ± range).
const IDEAL_DIST: f32 = 300.0;
const DIST_RANGE: f32 = 100.0;

/// Strafe-flip + dodge state. Held by the brain across ticks.
#[derive(Debug, Clone, Copy)]
pub struct StrafeState {
    /// Current strafe direction (+1 = one way, −1 = the other).
    pub dir: f32,
    /// Seconds in the current strafe direction.
    pub elapsed: f32,
}

impl StrafeState {
    pub fn new() -> Self {
        Self {
            dir: 1.0,
            elapsed: 0.0,
        }
    }
}

impl Default for StrafeState {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute the **world-space** (x,y) move direction for combat against an enemy.
///
/// - `enemy_dir`: unit direction from us to the enemy (xy plane).
/// - `dist`: distance to the enemy.
/// - `flip_roll`: a `[0,1)` roll used to decide a strafe-direction flip when the cadence elapses.
/// - `backup_roll`: a `[0,1)` roll; `>0.9` injects a random back-up this tick.
///
/// `ATTACK_SKILL < 0.2` → no movement (sitting duck). `≤ 0.4` → only close/open the gap. Higher
/// → circle-strafe perpendicular with the forward/back blend.
pub fn attack_move(
    ch: &Q3Character,
    dist: f32,
    enemy_dir: Vec3,
    strafe: &mut StrafeState,
    dt: f32,
    flip_roll: f32,
    backup_roll: f32,
) -> Vec3 {
    let enemy_dir = Vec3::new(enemy_dir.x, enemy_dir.y, 0.0).normalize_or_zero();
    if ch.attack_skill < 0.2 || enemy_dir == Vec3::ZERO {
        return Vec3::ZERO; // stand still
    }

    // Low-skill bots only close/open the gap (no strafing).
    if ch.attack_skill <= 0.4 {
        if dist > IDEAL_DIST + DIST_RANGE {
            return enemy_dir; // approach
        } else if dist < IDEAL_DIST - DIST_RANGE {
            return -enemy_dir; // back off
        }
        return Vec3::ZERO;
    }

    // Circle-strafe. Flip the strafe direction on a skill-tuned cadence (skilled bots change
    // crisply): `strafechange = 0.4 + (1 − attack_skill)·0.2` s, then only flip on a high roll.
    strafe.elapsed += dt;
    let strafe_change = 0.4 + (1.0 - ch.attack_skill) * 0.2;
    if strafe.elapsed > strafe_change && flip_roll > 0.935 {
        strafe.dir = -strafe.dir;
        strafe.elapsed = 0.0;
    }

    // Sideward = enemy_dir rotated 90° (×strafe.dir). In Q2 xy, rotating (x,y) by +90° → (−y, x).
    let sideward = Vec3::new(-enemy_dir.y, enemy_dir.x, 0.0) * strafe.dir;

    // Blend in forward/back: random back-up, else hold the ideal-distance band.
    let radial = if backup_roll > 0.9 {
        -enemy_dir // random back-up
    } else if dist > IDEAL_DIST + DIST_RANGE {
        enemy_dir // too far — close in
    } else if dist < IDEAL_DIST - DIST_RANGE {
        -enemy_dir // too close — back off
    } else {
        Vec3::ZERO
    };

    (sideward + radial).normalize_or_zero()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sitting_duck_does_not_move() {
        let ch = Q3Character {
            attack_skill: 0.1,
            ..Q3Character::default()
        };
        let mut st = StrafeState::new();
        let mv = attack_move(&ch, 300.0, Vec3::X, &mut st, 0.1, 0.0, 0.0);
        assert_eq!(mv, Vec3::ZERO);
    }

    #[test]
    fn low_skill_closes_the_gap() {
        let ch = Q3Character {
            attack_skill: 0.3,
            ..Q3Character::default()
        };
        let mut st = StrafeState::new();
        // Enemy far in +x → approach (+x).
        let mv = attack_move(&ch, 600.0, Vec3::X, &mut st, 0.1, 0.0, 0.0);
        assert!(mv.x > 0.5, "approaches a far enemy, got {mv:?}");
        // Enemy point-blank → back off (−x).
        let mv = attack_move(&ch, 50.0, Vec3::X, &mut st, 0.1, 0.0, 0.0);
        assert!(mv.x < -0.5, "backs off a near enemy, got {mv:?}");
    }

    #[test]
    fn circle_strafe_moves_perpendicular() {
        let ch = Q3Character {
            attack_skill: 0.8,
            ..Q3Character::default()
        };
        let mut st = StrafeState::new();
        // At ideal distance, no back-up roll → pure sideways (perpendicular to +x enemy → ±y).
        let mv = attack_move(&ch, IDEAL_DIST, Vec3::X, &mut st, 0.1, 0.0, 0.0);
        assert!(mv.y.abs() > 0.5, "strafes perpendicular, got {mv:?}");
        assert!(mv.x.abs() < 0.3, "little radial at ideal dist, got {mv:?}");
    }

    #[test]
    fn strafe_flips_direction_on_high_roll() {
        let ch = Q3Character {
            attack_skill: 0.8,
            ..Q3Character::default()
        };
        let mut st = StrafeState::new();
        let first = attack_move(&ch, IDEAL_DIST, Vec3::X, &mut st, 1.0, 0.99, 0.0);
        let second = attack_move(&ch, IDEAL_DIST, Vec3::X, &mut st, 1.0, 0.99, 0.0);
        // A high flip_roll after the cadence elapses flips the perpendicular sign.
        assert!(
            first.y * second.y < 0.0,
            "strafe direction flipped: {first:?} vs {second:?}"
        );
    }
}
