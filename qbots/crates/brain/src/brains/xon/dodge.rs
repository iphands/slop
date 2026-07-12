//! Flight-path projectile dodge (Plan 60 T5) — `havocbot_dodge` (`havocbot.qc:1773-1829`).
//!
//! For each dangerous projectile: project our offset from its **flight path**
//! (`v -= n*(v·n)` where `n` is the projectile's travel direction), danger =
//! `dodgerating − |v|`; if we're inside the rating, dodge ALONG the perpendicular offset
//! (away from the line). A resting projectile (grenade) pushes radially away. The summed
//! dodge vector is scaled `bound(0, 0.5 + dodge_skill*0.1, 1)` (`havocbot.qc:1798`).
//!
//! Upstream gates this behind SUPERBOT; we enable it for every bot scaled by the dodge
//! axis (documented improvement — it is the adopt-list's "flight-path dodge", and the
//! Plan 48 hazard gates at the call site keep it from dodging into lava).

use glam::Vec3;

/// Effective danger radius of a rocket/grenade flight line (Q2 RL splash 120 + hull margin).
const DODGE_RATING: f32 = 140.0;

/// Summed world-space dodge vector for the PVS-visible projectiles, or `None` when nothing
/// threatens. `projectiles` = (origin, velocity) pairs from frame deltas; `dodge_skill` is
/// `XonSkill::dodge()`.
pub fn flight_path_dodge(
    my_pos: Vec3,
    projectiles: &[(Vec3, Vec3)],
    dodge_skill: f32,
) -> Option<Vec3> {
    let mut dodge = Vec3::ZERO;
    for &(org, vel) in projectiles {
        let to_me = my_pos - org;
        let danger_dir = if vel.length_squared() > 100.0 {
            // Moving: distance from the flight path (`v -= n*(v*n)`, havocbot.qc:1782-1789).
            let n = vel.normalize();
            let ahead = to_me.dot(n);
            if ahead < 0.0 {
                continue; // already past us — no threat
            }
            let perp = to_me - n * ahead;
            if perp.length() >= DODGE_RATING {
                continue;
            }
            // Dodge along the perpendicular offset, away from the line. Dead-center
            // (perp ≈ 0) picks an arbitrary horizontal normal.
            perp.try_normalize()
                .unwrap_or_else(|| Vec3::new(-n.y, n.x, 0.0))
        } else {
            // Resting (armed grenade): radially away (havocbot.qc:1794-1796).
            if to_me.length() >= DODGE_RATING {
                continue;
            }
            to_me.try_normalize().unwrap_or(Vec3::X)
        };
        dodge += Vec3::new(danger_dir.x, danger_dir.y, 0.0);
    }
    let scale = (0.5 + dodge_skill * 0.1).clamp(0.0, 1.0);
    let d = dodge.normalize_or_zero() * scale;
    (d.length_squared() > 1e-6).then_some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dodges_perpendicular_to_an_incoming_rocket() {
        // Rocket flying +x toward us, we're offset +40y from its line → dodge is +y-ish.
        let d = flight_path_dodge(
            Vec3::new(500.0, 40.0, 0.0),
            &[(Vec3::ZERO, Vec3::new(650.0, 0.0, 0.0))],
            5.0,
        )
        .expect("threatened");
        assert!(d.y > 0.5, "dodge away from the flight line, got {d:?}");
    }

    #[test]
    fn ignores_a_rocket_flying_away() {
        // Same line, but the rocket has already passed (we're behind its origin).
        assert!(flight_path_dodge(
            Vec3::new(-100.0, 10.0, 0.0),
            &[(Vec3::ZERO, Vec3::new(650.0, 0.0, 0.0))],
            5.0,
        )
        .is_none());
    }

    #[test]
    fn ignores_distant_lines_and_scales_with_skill() {
        // 200 u off the line: outside the rating.
        assert!(flight_path_dodge(
            Vec3::new(500.0, 200.0, 0.0),
            &[(Vec3::ZERO, Vec3::new(650.0, 0.0, 0.0))],
            5.0,
        )
        .is_none());
        // Skill scaling: same threat, higher dodge skill → larger vector.
        let lo = flight_path_dodge(
            Vec3::new(500.0, 40.0, 0.0),
            &[(Vec3::ZERO, Vec3::new(650.0, 0.0, 0.0))],
            0.0,
        )
        .unwrap();
        let hi = flight_path_dodge(
            Vec3::new(500.0, 40.0, 0.0),
            &[(Vec3::ZERO, Vec3::new(650.0, 0.0, 0.0))],
            5.0,
        )
        .unwrap();
        assert!(hi.length() > lo.length());
    }

    #[test]
    fn resting_grenade_pushes_radially_away() {
        let d = flight_path_dodge(Vec3::new(60.0, 0.0, 0.0), &[(Vec3::ZERO, Vec3::ZERO)], 5.0)
            .expect("threatened");
        assert!(d.x > 0.5, "radially away from the grenade, got {d:?}");
    }
}
