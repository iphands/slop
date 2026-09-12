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
use world::{CollisionModel, MASK_SOLID};

/// Bots never aim steeply up/down — Eraser clamps pitch to ±15° (`bot_wpns.c:368`).
/// Notable GL/RL-lob limit. We clamp the *aim* pitch; the bot can still look
/// around freely for navigation.
pub const PITCH_CLAMP_DEG: f32 = 15.0;

/// Eraser's per-weapon lead point (`bot_wpns.c` lead table): the world-space point
/// the shot should be aimed *at*, i.e. the target origin advanced by the projectile's
/// flight time. Exposed separately from [`aim_direction`] because the skill jitter
/// (`aim_hitscan`) is defined as a perturbation of this POINT in world units, not of
/// the resulting angles.
///
/// Lead factors (`distilled/eraser.md` §5 lead table):
/// - Blaster / Hyperblaster: `dist/1000` (speed 1000).
/// - Rocket: `dist/650`, and **ignores upward velocity** so it won't lead a
///   jumper skyward (`bot_wpns.c:863-898`).
/// - Grenade: `dist/550` (deliberately over-leads ~9% to compensate for arc),
///   then pitches up piecewise (`bot_wpns.c:1042-1048`).
/// - BFG: `dist/400` (Eraser's `dist/550` is a bug vs the 400 fired speed).
/// - Hitscan (MG/SG/SSG/CG/Railgun): no lead (Railgun trails `−0.2*vel`).
pub fn lead_point(
    shooter_origin: Vec3,
    target_origin: Vec3,
    target_velocity: Option<Vec3>,
    weapon: Weapon,
) -> Vec3 {
    let dist = (target_origin - shooter_origin).length();
    let vel = target_velocity.unwrap_or(Vec3::ZERO);

    match weapon {
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
        // BFG: dist/400 (Eraser's dist/550 was a bug vs the 400 speed).
        Weapon::Bfg10k => target_origin + vel * (dist / 400.0),
    }
}

/// Yaw/pitch (degrees, pitch clamped) from `shooter_origin` at the world-space point
/// `aim_point`, applying the grenade lob. The single place angles are produced, so
/// leaded aim and jittered aim cannot drift apart.
///
/// `range` is the PHYSICAL shooter→target distance, deliberately NOT
/// `aim_point - shooter_origin`: the GL lob pays for projectile flight time, which is
/// set by how far the grenade actually travels, so it must not wobble when lead or
/// jitter displaces the aim point. Eraser computed `dist` from the real enemy origin
/// and reused it for both `tf` and the lob (`bot_wpns.c:423,1042`).
fn angles_from(shooter_origin: Vec3, aim_point: Vec3, weapon: Weapon, range: f32) -> (f32, f32) {
    let (yaw, pitch) = vec3_to_angles(aim_point - shooter_origin);

    // GL lob (`bot_wpns.c:1042-1048`): pitch up piecewise — +15° at/above 384u,
    // ramping down to −15° at dist=0. Only the GL reaches this branch; `combat.rs`
    // routes every other non-hitscan weapon through its own arc code.
    let pitch = if matches!(weapon, Weapon::GrenadeLauncher) {
        let lob = if range >= 384.0 {
            15.0
        } else {
            15.0 * (2.0 * range / 384.0 - 1.0)
        };
        pitch + lob
    } else {
        pitch
    };

    // Eraser clamps aim pitch to ±15° (`:368`). Apply after the GL lob so a
    // maximum lob still respects the cap.
    (yaw, pitch.clamp(-PITCH_CLAMP_DEG, PITCH_CLAMP_DEG))
}

/// Compute the aim direction toward a target for the given weapon, applying
/// Eraser's exact per-weapon lead factor and, for the grenade launcher, the
/// piecewise pitch-lob. Returns `(yaw_deg, pitch_deg)` with pitch clamped to
/// ±[`PITCH_CLAMP_DEG`] (Eraser `bot_wpns.c:368`).
pub fn aim_direction(
    shooter_origin: Vec3,
    target_origin: Vec3,
    target_velocity: Option<Vec3>,
    weapon: Weapon,
) -> (f32, f32) {
    let dist = (target_origin - shooter_origin).length();
    angles_from(
        shooter_origin,
        lead_point(shooter_origin, target_origin, target_velocity, weapon),
        weapon,
        dist,
    )
}

/// Eraser skill-jittered aim for hitscan weapons (`bot_wpns.c:423-430` skeleton):
/// ```text
/// tf = min(dist/2, 256) * ((5−accuracy)/5) * 2   // acc5→0 (perfect), acc1→1.6×
/// jitter target by crandom()*tf in x,y and crandom()*tf*zscale in z (MG 0.1, else 0.2)
/// ```
/// `accuracy` is 1..5. `tf` is a **world-unit offset added to the aim point** — the
/// reference perturbs `target` then re-aims, so `zscale` is a height scale and the
/// error shrinks with range as real inaccuracy does. Returns `(yaw, pitch)` clamped
/// to ±[`PITCH_CLAMP_DEG`].
///
/// The angular error is bounded at ANY range, which is the guarantee the turn
/// controller relies on: worst case (acc1, target straight ahead, the x-draw pulling
/// back against range while the y-draw saturates) the aim point lands 0.8·dist to one
/// side of a 0.2·dist forward component, so `atan(0.8/0.2) ≈ 76°` — the aim may spray,
/// but can never spin or reverse. acc4 ≤ 13.5°, acc3 ≤ 22.6°.
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
    let mut aim = lead_point(shooter_origin, target_origin, target_velocity, weapon);

    if accuracy < 5.0 {
        let tf = (dist / 2.0).min(256.0) * ((5.0 - accuracy) / 5.0) * 2.0;
        let zscale = if matches!(weapon, Weapon::Machinegun) {
            0.1
        } else {
            0.2
        };
        aim += Vec3::new(
            rng.next_signed() * tf,
            rng.next_signed() * tf,
            rng.next_signed() * tf * zscale,
        );
    }

    angles_from(shooter_origin, aim, weapon, dist)
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
    /// Seed the RNG. Callers mix the tick with a per-bot salt before calling
    /// (`CombatDriver::jitter_rng`) so fleet bots on shared server frames diverge.
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

/// Would firing a **splash** weapon at `aim_target` splash *us*? Traces eye→aim_target; if the
/// world is hit short of the target and the impact is within the weapon's blast radius of our
/// own feet, a self-preservation-minded bot holds fire (Q3 `BotCheckAttack` radial check; promoted from `brains/q3/aim.rs` for the xon brain, Plan 60 T4).
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

    /// The invariant the radians-vs-world-units bug violated: jitter is a WORLD
    /// offset (max 409.6 u), so the angular error must stay bounded by whatever
    /// that offset subtends — never thousands of degrees. `aim_hitscan` must land
    /// within 90° of true aim at every distance/skill, i.e. it may miss, but it
    /// may never spin.
    #[test]
    fn jitter_stays_within_half_a_circle() {
        for dist in [64.0, 128.0, 300.0, 500.0, 1024.0, 4096.0] {
            for acc in [1.0f32, 2.0, 3.0, 4.0, 4.9] {
                for sign in [1.0f32, -1.0] {
                    let shooter = Vec3::ZERO;
                    let target = Vec3::new(dist, 0.0, 0.0);
                    let mut rng = ConstRng(sign);
                    let (yaw, pitch) =
                        aim_hitscan(shooter, target, None, Weapon::Shotgun, acc, &mut rng);
                    let err = yaw - 0.0;
                    assert!(
                        err.abs() <= 90.0 && pitch.abs() <= PITCH_CLAMP_DEG + 0.001,
                        "dist={dist} acc={acc} sign={sign}: jitter must stay bounded, got yaw={yaw} pitch={pitch}"
                    );
                }
            }
        }
    }

    /// At close range a low-skill bot's shots must still land near the target:
    /// 64 u away at acc 4 is tf=12.8 u of offset = under 12° of aim error. The
    /// old code produced 12.8 radians (≈733°) here.
    #[test]
    fn near_range_low_skill_error_is_small() {
        let shooter = Vec3::ZERO;
        let target = Vec3::new(64.0, 0.0, 0.0);
        let mut rng = ConstRng(1.0);
        let (yaw, _) = aim_hitscan(shooter, target, None, Weapon::Shotgun, 4.0, &mut rng);
        assert!(
            yaw.abs() < 12.0,
            "12.8u of jitter at 64u range is <12°, got {yaw}"
        );
    }

    /// The refactor's load-bearing invariant: at `accuracy == 5` (perfect) jitter is
    /// not applied, so the jittered path must agree with `aim_direction` to the bit —
    /// for EVERY weapon, GL included. `accuracy_5_is_perfect_no_jitter` tests a target
    /// on the +x axis where lead and jitter in y barely move yaw, so it passes for
    /// almost any wrong implementation; this pins the two paths directly at an oblique
    /// target with motion. It is also the guard for the GL-lob-range regression.
    #[test]
    fn acc5_hitscan_matches_aim_direction() {
        let sh = Vec3::new(10.0, -20.0, 32.0);
        let tgt = Vec3::new(410.0, 130.0, 96.0);
        let vel = Some(Vec3::new(-140.0, 60.0, 0.0));
        for w in [
            Weapon::Blaster,
            Weapon::Railgun,
            Weapon::Shotgun,
            Weapon::Machinegun,
            Weapon::Chaingun,
            Weapon::RocketLauncher,
            Weapon::GrenadeLauncher,
            Weapon::Bfg10k,
        ] {
            let a = aim_direction(sh, tgt, vel, w);
            let b = aim_hitscan(sh, tgt, vel, w, 5.0, &mut ConstRng(1.0));
            assert!(
                (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4,
                "{w:?}: acc5 must be bit-identical to aim_direction, {a:?} vs {b:?}"
            );
        }
    }

    /// Pins the one place the split could silently change `aim_direction`'s behaviour:
    /// the GL lob must key on the PHYSICAL range, never the leaded aim-point distance.
    /// Target 200u out, strafing at 400 u/s -> lead displaces the aim point to 247u.
    /// Lob on true range 200 = +0.625 deg; on leaded range 247 it would be +4.3 deg.
    #[test]
    fn grenade_lob_uses_true_range_not_leaded_distance() {
        let pitch = aim_direction(
            Vec3::ZERO,
            Vec3::new(200.0, 0.0, 0.0),
            Some(Vec3::new(0.0, 400.0, 0.0)),
            Weapon::GrenadeLauncher,
        )
        .1;
        assert!(
            (pitch - 0.625).abs() < 0.05,
            "GL lob must key on true range 200 (+0.625 deg); leaded range 247 would give \
             +4.3 deg, got {pitch}"
        );
    }

    /// `lead_point` is the load-bearing half of the split (and now public), so pin the
    /// lead TABLE numerically — a future factor edit must fail on the origin, not on
    /// some downstream yaw delta.
    #[test]
    fn lead_point_matches_eraser_lead_table() {
        let sh = Vec3::ZERO;
        // Rocket: dist/650, and upward velocity is zeroed so jumpers aren't led skyward.
        let rl = lead_point(
            sh,
            Vec3::new(650.0, 0.0, 0.0),
            Some(Vec3::new(0.0, 650.0, 900.0)),
            Weapon::RocketLauncher,
        );
        assert_eq!(
            rl,
            Vec3::new(650.0, 650.0, 0.0),
            "RL: dist/650, +z velocity ignored"
        );
        // Grenade: dist/550 — deliberately over-leads vs the 700 fired speed.
        let gl = lead_point(
            sh,
            Vec3::new(550.0, 0.0, 0.0),
            Some(Vec3::new(0.0, -550.0, 0.0)),
            Weapon::GrenadeLauncher,
        );
        assert_eq!(gl, Vec3::new(550.0, -550.0, 0.0), "GL: dist/550");
        // BFG: dist/400 (Eraser's dist/550 is a bug vs the 400 fired speed).
        let bfg = lead_point(
            sh,
            Vec3::new(400.0, 0.0, 0.0),
            Some(Vec3::new(0.0, 400.0, 0.0)),
            Weapon::Bfg10k,
        );
        assert_eq!(bfg, Vec3::new(400.0, 400.0, 0.0), "BFG: dist/400");
        // Hitscan: no lead at all; blaster is dist/1000.
        let mg = lead_point(
            sh,
            Vec3::new(500.0, 0.0, 0.0),
            Some(Vec3::new(0.0, 300.0, 0.0)),
            Weapon::Machinegun,
        );
        assert_eq!(mg, Vec3::new(500.0, 0.0, 0.0), "hitscan: never leads");
        let bl = lead_point(
            sh,
            Vec3::new(1000.0, 0.0, 0.0),
            Some(Vec3::new(0.0, 100.0, 0.0)),
            Weapon::Blaster,
        );
        assert_eq!(bl, Vec3::new(1000.0, 100.0, 0.0), "blaster: dist/1000");
    }

    /// The physical property Eraser's `tf` formula has and the exact inversion of the
    /// old unit bug: because `tf` is proportional to `dist` below its 256 cap, the
    /// angular error is flat with range and then SHRINKS once the cap binds — it never
    /// grows. The old code (tf read as radians) was flat-but-multiplying-by-57.3, so
    /// the strict decrease above 512u is what discriminates.
    #[test]
    fn jitter_error_never_grows_with_range() {
        // Target straight along +x, so the NO-JITTER yaw is 0 at every range and the
        // measured error is purely the tf/dist geometry (an oblique target would make
        // the baseline yaw itself move with range and mask the property).
        let err_at = |dist: f32| {
            let mut rng = ConstRng(1.0);
            aim_hitscan(
                Vec3::ZERO,
                Vec3::new(dist, 0.0, 0.0),
                None,
                Weapon::Shotgun,
                1.0,
                &mut rng,
            )
            .0
            .abs()
        };
        let ladder = [128.0, 256.0, 512.0, 1024.0, 2048.0, 4096.0];
        let errs: Vec<f32> = ladder.iter().map(|d| err_at(*d)).collect();
        for w in errs.windows(2) {
            assert!(
                w[1] <= w[0] + 1e-3,
                "jitter error must not grow with range: {ladder:?} -> {errs:?}"
            );
        }
        // `tf` saturates at 256 * 1.6 = 409.6u once dist > 512, so beyond that the same
        // world offset subtends a strictly smaller angle. The old bug (tf read as
        // radians) was flat-in-radians here, i.e. ~23000 degrees of wrapped nonsense.
        assert!(
            errs[3] < errs[2] * 0.9,
            "past the tf cap the error must shrink with range: {ladder:?} -> {errs:?}"
        );
    }

    #[test]
    fn in_fov_center_and_edge() {
        let forward = Vec3::new(1.0, 0.0, 0.0);
        assert!(in_fov(forward, Vec3::new(1.0, 0.0, 0.0), 90.0));
        assert!(!in_fov(forward, Vec3::new(0.0, 1.0, 0.0), 89.0));
    }
}
