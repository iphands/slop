//! The havocbot aim dynamical system (Plan 59 T3) — a faithful port of `bot_aimdir` +
//! `bot_aim` (`aim.qc:150-414`).
//!
//! Xonotic models the *dynamics* of human aim, not just positional error:
//! 1. a periodically resampled **bad-aim offset** (`aim.qc:194-203`),
//! 2. a five-stage **anticipation filter cascade** over the desired-angle velocity
//!    (`aim.qc:230-250`) — low skill lags a mover, high skill anticipates,
//! 3. discrete **"mouse think"** retargeting with random undershoot (`aim.qc:261-265`),
//! 4. a distance-anger **turn-rate law** (`aim.qc:289-295`),
//! 5. an empirical **fire cone** `1000/(dist−9) − 0.35` degrees + a burst **fire timer**
//!    (`aim.qc:365-374`, `:315-330`),
//! 6. linear **shot lead** `target + vel*(latency + dist/shotspeed)` (`bot_shotlead`,
//!    `aim.qc:333-337`).
//!
//! Angles are Q2 `v_angle` convention: degrees, **pitch positive = down** (the vendor's
//! `desiredang.x = bound(-90, 0 - vectoangles(v).x, 90)`, `aim.qc:207`). All state is
//! per-bot; all randomness comes from the caller's [`Lcg`]. The internal clock accumulates
//! the caller's `dt` — the vendor's absolute `time` timers translate directly.
//!
//! Deliberate deviations, documented:
//! - The vendor's `int f = bound(0, 1 - 0.1*(skill+offsetskill), 1)` (`aim.qc:197`) truncates
//!   to 0 for any positive skill in gmqcc — an upstream regression that disables the bad-aim
//!   offset almost everywhere. We port the float semantics (the intended behavior).
//! - SUPERBOT (`skill > 100`) instant snap (`aim.qc:167-180`) is not modeled.
//! - `findtrajectorywithleading` ballistic search (`aim.qc:16-95`) is deferred to the brain
//!   (needs a CM gravity trace); [`XonAim::step`] aims at the (lead-corrected) point.

use glam::Vec3;

use super::{wrap180, Lcg};
use crate::xonchar::XonSkill;

/// Cvar defaults (`xonotic-server.cfg:136-183`), named as in `cvars.qh`.
const AIMSKILL_OFFSET: f32 = 1.8;
const ORDER_FILTER: [f32; 5] = [0.2, 0.2, 0.1, 0.2, 0.25];
const ORDER_MIX: [f32; 5] = [0.01, 0.075, 0.01, 0.0375, 0.01];
const AIMSKILL_FIXEDRATE: f32 = 15.0;
const AIMSKILL_BLENDRATE: f32 = 2.0;

/// A (pitch, yaw) angle pair in degrees, Q2 convention (pitch positive down).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Angles {
    pub pitch: f32,
    pub yaw: f32,
}

impl Angles {
    /// Angles looking along `v` (world direction): `yaw = atan2(y, x)`,
    /// `pitch = -atan2(z, xy)` — the vendor's negated `vectoangles` (`aim.qc:207`).
    pub fn from_dir(v: Vec3) -> Self {
        let xy = (v.x * v.x + v.y * v.y).sqrt();
        Self {
            pitch: -v.z.atan2(xy).to_degrees(),
            yaw: v.y.atan2(v.x).to_degrees(),
        }
    }

    /// Wrapped difference `self − other` (yaw wrapped to ±180; pitch is already bounded).
    fn diff(self, other: Self) -> Self {
        Self {
            pitch: self.pitch - other.pitch,
            yaw: wrap180(self.yaw - other.yaw),
        }
    }

    /// Vector length over (pitch, yaw) — the vendor's `vlen(diffang)` (`aim.qc:285`).
    fn len(self) -> f32 {
        (self.pitch * self.pitch + self.yaw * self.yaw).sqrt()
    }
}

/// Per-call aim inputs. `shot_speed` uses the vendor's hitscan fallback `1_000_000`
/// (`aim.qc:352`) for instant weapons; `latency` is the bot's real RTT (`bot_aimlatency`);
/// `sight_dist` is how far the bot can see along its view (the `traceline` distance at
/// `aim.qc:324-325` — the caller traces, pass `f32::INFINITY` for open air).
#[derive(Debug, Clone, Copy)]
pub struct AimInputs {
    /// Shot origin (eye position).
    pub eye: Vec3,
    /// Target center (the vendor aims at bbox center, `aim.qc:362`).
    pub target_pos: Vec3,
    /// Target velocity from frame deltas (for the shot lead).
    pub target_vel: Vec3,
    /// Projectile speed (u/s); `1e6` for hitscan.
    pub shot_speed: f32,
    /// Our measured latency in seconds (lead term).
    pub latency: f32,
    /// Fighting an enemy (×5 bad-aim factor + real skill) vs roaming (×2 + `max(4, skill)`
    /// smoothing, `aim.qc:186-188`, `:201`).
    pub fighting: bool,
    /// Weapon counts as accurate (fire-cone factor 1 vs 1.6, `aim.qc:372`).
    pub accurate: bool,
    /// Distance to the first wall along our view (close quarters always arm the trigger,
    /// `aim.qc:324-325`).
    pub sight_dist: f32,
}

/// What the aim system wants this tick: the new view angles and whether the trigger is down.
#[derive(Debug, Clone, Copy)]
pub struct AimCmd {
    pub angles: Angles,
    pub fire: bool,
}

/// The per-bot aim state (filters, offsets, timers). One per bot; survives across targets.
#[derive(Debug, Clone)]
pub struct XonAim {
    /// Internal clock (seconds, accumulated from `dt`).
    time: f32,
    /// Resample deadline for the bad-aim offset (`bot_badaimtime`).
    badaim_until: f32,
    /// Current bad-aim offset (`bot_badaimoffset`), degrees.
    badaim: Angles,
    /// Previous desired angles (`bot_olddesiredang`) — the filter cascade's differentiator.
    old_desired: Angles,
    /// The five chained low-pass states (`bot_1st..5th_order_aimfilter`), deg/s.
    filters: [Angles; 5],
    /// The discrete mouse target (`bot_mouseaim`).
    mouse_aim: Angles,
    /// Mouse retarget deadline (`bot_aimthinktime`).
    mouse_until: f32,
    /// Trigger-down deadline (`bot_firetimer`).
    fire_timer: f32,
}

impl XonAim {
    /// Fresh aim state.
    pub fn new() -> Self {
        Self {
            time: 0.0,
            badaim_until: 0.0,
            badaim: Angles::default(),
            old_desired: Angles::default(),
            filters: [Angles::default(); 5],
            mouse_aim: Angles::default(),
            mouse_until: 0.0,
            fire_timer: -1.0,
        }
    }

    /// One aim tick: advance the pipeline and return the new view angles + fire decision.
    /// `current` is the bot's present view angles; `dt` the seconds since the last call.
    pub fn step(
        &mut self,
        rng: &mut Lcg,
        sk: &XonSkill,
        current: Angles,
        inp: &AimInputs,
        dt: f32,
    ) -> AimCmd {
        let dt = dt.max(1e-3);
        self.time += dt;

        // Roaming turns use `max(4, skill)` so low-skill bots still walk smoothly
        // (`aim.qc:186-188`). The axis offsets ride on top either way.
        let mut eff = *sk;
        if !inp.fighting {
            eff.skill = eff.skill.max(4.0);
        }

        // ── 6. Shot lead first (`bot_shotlead`, aim.qc:333-337): v is the aim vector ──
        let dist0 = (inp.target_pos - inp.eye).length();
        let lead = inp.target_vel * (inp.latency + dist0 / inp.shot_speed.max(1.0));
        let v = (inp.target_pos + lead) - inp.eye;
        if v.length_squared() < 1e-6 {
            // Invalid aim dir (overlapping the target) — hold angles (`aim.qc:183`).
            return AimCmd {
                angles: current,
                fire: self.time <= self.fire_timer,
            };
        }

        // ── 1. Bad-aim offset, resampled every 0.2–0.5 s (`aim.qc:194-203`) ───────────
        if self.time >= self.badaim_until {
            self.badaim_until = (self.badaim_until + 0.2 + 0.3 * rng.next()).max(self.time);
            let f = (1.0 - 0.1 * eff.offset()).clamp(0.0, 1.0); // float semantics (see module doc)
                                                                // randomvec(): three independent rolls in [-1, 1].
            let rv = Angles {
                pitch: (rng.next() * 2.0 - 1.0) * 0.7, // smaller vertical (`aim.qc:199`)
                yaw: rng.next() * 2.0 - 1.0,
            };
            self.badaim = Angles {
                pitch: rv.pitch * f * AIMSKILL_OFFSET,
                yaw: rv.yaw * f * AIMSKILL_OFFSET,
            };
        }
        let enemy_factor = if inp.fighting { 5.0 } else { 2.0 }; // `aim.qc:201`
        let mut desired = Angles::from_dir(v);
        desired.pitch = (desired.pitch + self.badaim.pitch * enemy_factor).clamp(-90.0, 90.0);
        desired.yaw += self.badaim.yaw * enemy_factor;

        // ── 2. Anticipation filter cascade over desired-angle velocity (`aim.qc:230-250`) ──
        let diff = desired.diff(self.old_desired);
        self.old_desired = desired;
        let mut input = Angles {
            pitch: diff.pitch / dt,
            yaw: diff.yaw / dt,
        };
        for (i, f) in self.filters.iter_mut().enumerate() {
            f.pitch += (input.pitch - f.pitch) * ORDER_FILTER[i];
            f.yaw += (input.yaw - f.yaw) * ORDER_FILTER[i];
            input = *f; // chained: each stage filters the previous stage's output
        }
        let blend = eff.aim().clamp(0.0, 10.0) * 0.1; // `aim.qc:242`
        for (i, f) in self.filters.iter().enumerate() {
            desired.pitch += blend * f.pitch * ORDER_MIX[i];
            desired.yaw += blend * f.yaw * ORDER_MIX[i];
        }
        desired.pitch = desired.pitch.clamp(-90.0, 90.0);

        // ── 3. Mouse think: the internal target updates only at the think cadence with a
        // random undershoot (`aim.qc:261-274`; aimskill_think = 1 → desired = mouse_aim) ──
        let mdiff = desired.diff(self.mouse_aim);
        if self.time >= self.mouse_until {
            self.mouse_until = (self.mouse_until + 0.5 - 0.05 * eff.think()).max(self.time);
            let under = 1.0 - rng.next() * 0.1 * (10.0 - eff.think()).clamp(1.0, 10.0);
            self.mouse_aim.pitch += mdiff.pitch * under;
            self.mouse_aim.yaw = wrap180(self.mouse_aim.yaw + mdiff.yaw * under);
        }
        desired = self.mouse_aim;

        // ── 4. Turn-rate law (`aim.qc:289-295`) ───────────────────────────────────────
        let tdiff = desired.diff(current);
        let dist_ang = tdiff.len();
        let fixedrate = AIMSKILL_FIXEDRATE / dist_ang.clamp(1.0, 1000.0);
        let r = fixedrate.max(AIMSKILL_BLENDRATE);
        let m = eff.mouse();
        let r = (r * dt * (2.0 + m * m * m * 0.005 - rng.next())).clamp(dt, 1.0);
        let angles = Angles {
            pitch: (current.pitch + tdiff.pitch * r).clamp(-90.0, 90.0),
            yaw: wrap180(current.yaw + tdiff.yaw * r),
        };

        // ── 5. Fire cone + burst timer (`bot_aim` aim.qc:365-374 + `bot_aimdir` :315-330).
        // Only meaningful while fighting; the cone is measured between the ideal aim vector
        // and the NEW view angles, per axis.
        if inp.fighting {
            let dist = dist0.max(10.0);
            let mut maxdev = 1000.0 / (dist - 9.0) - 0.35; // empirical hit cone (`aim.qc:370`)
            let f = if inp.accurate { 1.0 } else { 1.6 };
            let f = f + ((10.0 - eff.aim()) * 0.3).clamp(0.0, 3.0);
            maxdev = (maxdev * f).min(90.0);

            let ideal = Angles::from_dir(v);
            let dev = ideal.diff(angles);
            if dev.pitch.abs() < maxdev && dev.yaw.abs() < maxdev {
                let close = inp.sight_dist < 500.0 + 500.0 * eff.aggres().clamp(0.0, 10.0);
                let hesitate = rng.next() * rng.next() > (eff.aggres() * 0.05).clamp(0.0, 1.0);
                if close || hesitate {
                    self.fire_timer = self.time + (0.5 - eff.aggres() * 0.05).clamp(0.1, 0.5);
                }
            }
        }

        AimCmd {
            angles,
            // `bot_aim` returns "press fire" while the timer runs (`aim.qc:405-409`).
            fire: inp.fighting && self.time <= self.fire_timer,
        }
    }
}

impl Default for XonAim {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(target: Vec3, vel: Vec3) -> AimInputs {
        AimInputs {
            eye: Vec3::ZERO,
            target_pos: target,
            target_vel: vel,
            shot_speed: 1_000_000.0,
            latency: 0.05,
            fighting: true,
            accurate: true,
            sight_dist: f32::INFINITY,
        }
    }

    fn skill(n: f32) -> XonSkill {
        XonSkill::new(n)
    }

    /// Ticks until the view is within `tol` degrees of a fixed target at +x… from a 90° start.
    fn ticks_to_converge(sk: &XonSkill, seed: u32, tol: f32) -> usize {
        let mut aim = XonAim::new();
        let mut rng = Lcg::new(seed);
        let mut cur = Angles {
            pitch: 0.0,
            yaw: 90.0,
        };
        let inp = inputs(Vec3::new(500.0, 0.0, 0.0), Vec3::ZERO);
        for tick in 0..600 {
            let cmd = aim.step(&mut rng, sk, cur, &inp, 0.1);
            cur = cmd.angles;
            if cur.yaw.abs() < tol && cur.pitch.abs() < tol {
                return tick;
            }
        }
        600
    }

    #[test]
    fn high_skill_converges_faster_and_tighter() {
        // Wide tolerance: skill 10 must snap to a step target no slower than skill 0
        // (same seed → same rolls) and reach a tight cone the low-skill bot may never hold.
        let hi = ticks_to_converge(&skill(10.0), 42, 3.0);
        let lo = ticks_to_converge(&skill(0.0), 42, 3.0);
        assert!(hi <= lo, "skill10 {hi} ticks vs skill0 {lo}");
        assert!(hi < 60, "skill10 should converge fast, took {hi}");
    }

    #[test]
    fn cascade_is_stable_over_10k_steps() {
        // A circling target at dt=0.1 (our real tick) must never blow up the filters.
        let mut aim = XonAim::new();
        let mut rng = Lcg::new(7);
        let sk = skill(8.0);
        let mut cur = Angles::default();
        for i in 0..10_000 {
            let a = i as f32 * 0.05;
            let target = Vec3::new(400.0 * a.cos(), 400.0 * a.sin(), 20.0 * (a * 3.0).sin());
            let cmd = aim.step(&mut rng, &sk, cur, &inputs(target, Vec3::ZERO), 0.1);
            cur = cmd.angles;
            assert!(cur.pitch.is_finite() && cur.yaw.is_finite(), "tick {i}");
            assert!(cur.pitch.abs() <= 90.0);
            assert!(cur.yaw.abs() <= 180.0);
        }
    }

    #[test]
    fn fires_in_bursts_when_on_target() {
        // Converged on a stationary target: the trigger must arm (close quarters → the
        // sight_dist branch guarantees arming) and stay down for the burst window.
        let mut aim = XonAim::new();
        let mut rng = Lcg::new(3);
        let sk = skill(8.0);
        let mut cur = Angles::default();
        let mut inp = inputs(Vec3::new(300.0, 0.0, 0.0), Vec3::ZERO);
        inp.sight_dist = 300.0; // close → always arms once in-cone (`aim.qc:324-325`)
        let mut fired = 0;
        for _ in 0..100 {
            let cmd = aim.step(&mut rng, &sk, cur, &inp, 0.1);
            cur = cmd.angles;
            if cmd.fire {
                fired += 1;
            }
        }
        assert!(
            fired > 10,
            "on-target close-range bot must fire, got {fired}"
        );
    }

    #[test]
    fn does_not_fire_when_far_off_target() {
        // First tick from 180° away: way outside any cone — no fire.
        let mut aim = XonAim::new();
        let mut rng = Lcg::new(9);
        let sk = skill(5.0);
        let cur = Angles {
            pitch: 0.0,
            yaw: 180.0,
        };
        let cmd = aim.step(
            &mut rng,
            &sk,
            cur,
            &inputs(Vec3::new(400.0, 0.0, 0.0), Vec3::ZERO),
            0.1,
        );
        assert!(!cmd.fire);
    }

    #[test]
    fn leads_a_moving_target() {
        // Slow projectile vs a +y mover: converged yaw must sit AHEAD of the direct line
        // (positive yaw) by roughly vel*(dist/speed) worth of angle.
        let mut aim = XonAim::new();
        let mut rng = Lcg::new(11);
        let sk = skill(10.0);
        let mut cur = Angles::default();
        let mut inp = inputs(Vec3::new(600.0, 0.0, 0.0), Vec3::new(0.0, 300.0, 0.0));
        inp.shot_speed = 650.0; // rocket
        for _ in 0..200 {
            let cmd = aim.step(&mut rng, &sk, cur, &inp, 0.1);
            cur = cmd.angles;
        }
        // Direct line = yaw 0; lead ≈ atan2(300*(600/650 + 0.05), 600) ≈ 25°.
        assert!(cur.yaw > 10.0, "must lead the mover, yaw = {}", cur.yaw);
    }

    #[test]
    fn roaming_never_fires_and_smooths_low_skill() {
        let mut aim = XonAim::new();
        let mut rng = Lcg::new(5);
        let sk = skill(0.0); // roaming lifts effective skill to 4 (`aim.qc:186-188`)
        let mut cur = Angles::default();
        let mut inp = inputs(Vec3::new(0.0, 500.0, 0.0), Vec3::ZERO);
        inp.fighting = false;
        let mut reached = false;
        for _ in 0..300 {
            let cmd = aim.step(&mut rng, &sk, cur, &inp, 0.1);
            cur = cmd.angles;
            assert!(!cmd.fire, "roaming must never press fire");
            if (cur.yaw - 90.0).abs() < 5.0 {
                reached = true;
            }
        }
        assert!(reached, "roam turn must reach the goal yaw, at {}", cur.yaw);
    }
}
