//! Keyboard-emulation movement quantizer (Plan 59 T4) — a faithful port of
//! `havocbot_keyboard_movement` (`havocbot.qc:272-341`).
//!
//! Xonotic humanizes bot movement by pretending the bot is on a keyboard: the analog
//! movement vector is quantized to `{-1, 0, +1}` per axis at a skill-scaled re-key cadence,
//! with low skills gated to smaller key vocabularies (forward only → no diagonals → forward
//! diagonals → everything). Close to the goal the quantized keys are blended back toward the
//! analog vector (`bound(0, dist/250, 1)`) — full 360° analog steering when near, clunky keys
//! when far. This maps 1:1 onto Q2's `usercmd` forward/side axes.
//!
//! The vendor applies this only when `skill < 10` (call site `havocbot.qc:170`-ish);
//! [`KeyboardEmu::quantize`] mirrors that gate internally.

use super::Lcg;
use crate::xonchar::XonSkill;

/// `bot_ai_keyboard_threshold` (cvar default 0.57): the analog magnitude a "key" needs.
const THRESHOLD: f32 = 0.57;
/// `bot_ai_keyboard_distance` (cvar default 250): the analog-blend radius (qu).
const BLEND_DIST: f32 = 250.0;

/// Per-bot keyboard state: the currently "held" keys + the re-key clock.
#[derive(Debug, Clone, Default)]
pub struct KeyboardEmu {
    /// Internal clock (seconds, accumulated from `dt`).
    time: f32,
    /// Re-key deadline (`havocbot_keyboardtime`).
    next_key_time: f32,
    /// The held keys as (forward, side) ∈ {-1, 0, +1} (`havocbot_keyboard`).
    keys: (f32, f32),
}

impl KeyboardEmu {
    /// Fresh state (no keys held).
    pub fn new() -> Self {
        Self::default()
    }

    /// Quantize an analog `(forward, side)` intent (each in `[-1, 1]`) through the keyboard
    /// model. `dist_to_goal` drives the analog blend. Skill ≥ 10 is a pure passthrough.
    pub fn quantize(
        &mut self,
        rng: &mut Lcg,
        sk: &XonSkill,
        analog: (f32, f32),
        dist_to_goal: f32,
        dt: f32,
    ) -> (f32, f32) {
        if sk.skill >= 10.0 {
            return analog;
        }
        self.time += dt;

        if self.time > self.next_key_time {
            // Re-key cadence (`havocbot.qc:278-283`): note the vendor sums differently in
            // the two terms — `sk + keyboardskill` (sk = skill + moveskill) vs
            // `skill + keyboardskill`.
            let sk_move = sk.movement();
            let t1 = 0.05 / (sk_move + sk.axes.keyboard).max(1.0);
            let t2 = rng.next() * 0.025 / sk.keyboard().max(0.000_25);
            self.next_key_time = (self.next_key_time + t1 + t2).max(self.time);

            // Quantize with the skill-gated key vocabulary (`havocbot.qc:289-320`):
            // sk < 1.5 → forward only; sk < 2.5 → no diagonals; sk < 4.5 → forward
            // diagonals only (no backward diagonals); ≥ 4.5 → all combos.
            let (ax, ay) = analog;
            let kx;
            let mut ky = ay;
            if ax > THRESHOLD {
                kx = 1.0;
                if sk_move < 2.5 {
                    ky = 0.0;
                }
            } else if ax < -THRESHOLD && sk_move > 1.5 {
                kx = -1.0;
                if sk_move < 4.5 {
                    ky = 0.0;
                }
            } else {
                kx = 0.0;
                if sk_move < 1.5 {
                    ky = 0.0;
                }
            }
            ky = if ky > THRESHOLD {
                1.0
            } else if ky < -THRESHOLD {
                -1.0
            } else {
                0.0
            };

            // No keys at all → don't stay frozen behind a long re-key timer
            // (`havocbot.qc:329-331`).
            if kx == 0.0 && ky == 0.0 {
                self.next_key_time = self.next_key_time.min(self.time + 0.2);
            }
            self.keys = (kx, ky);
        }

        // Analog blend by distance (`havocbot.qc:338-340`): near = analog, far = keys.
        let blend = (dist_to_goal / BLEND_DIST).clamp(0.0, 1.0);
        (
            analog.0 + (self.keys.0 - analog.0) * blend,
            analog.1 + (self.keys.1 - analog.1) * blend,
        )
    }

    /// Drop the held keys and arm an immediate re-key. The caller's hazard veto (Plan 63
    /// B4): keys are held across ticks at the re-key cadence AFTER every upstream hazard
    /// gate ran, so a stale held key can point into lava on a later tick — releasing lets
    /// the (already-gated) analog legs take over this tick and re-keys fresh on the next.
    pub fn release(&mut self) {
        self.keys = (0.0, 0.0);
        self.next_key_time = self.time;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xonchar::XonAxes;

    fn skill(n: f32) -> XonSkill {
        XonSkill::new(n)
    }

    /// Run one quantize far from the goal (pure keys) with a fresh emu.
    fn far_keys(sk: &XonSkill, analog: (f32, f32)) -> (f32, f32) {
        let mut kb = KeyboardEmu::new();
        let mut rng = Lcg::new(1);
        kb.quantize(&mut rng, sk, analog, 10_000.0, 0.1)
    }

    #[test]
    fn skill_ten_is_passthrough() {
        let mut kb = KeyboardEmu::new();
        let mut rng = Lcg::new(1);
        let out = kb.quantize(&mut rng, &skill(10.0), (0.33, -0.71), 10_000.0, 0.1);
        assert_eq!(out, (0.33, -0.71));
    }

    #[test]
    fn threshold_pins() {
        // 0.58 forward → the forward key; 0.56 → no key (threshold 0.57).
        assert_eq!(far_keys(&skill(5.0), (0.58, 0.0)), (1.0, 0.0));
        assert_eq!(far_keys(&skill(5.0), (0.56, 0.0)), (0.0, 0.0));
        assert_eq!(far_keys(&skill(5.0), (0.0, -0.9)), (0.0, -1.0));
    }

    #[test]
    fn low_skill_vocabulary_gates() {
        // sk 1 (< 1.5): forward only — no side keys ever, no backpedal key.
        let noob = XonSkill {
            skill: 1.0,
            axes: XonAxes::default(),
        };
        assert_eq!(far_keys(&noob, (0.9, 0.9)), (1.0, 0.0)); // fwd diagonal → fwd only... sk<2.5 kills y
        assert_eq!(far_keys(&noob, (-0.9, 0.0)), (0.0, 0.0)); // backpedal needs sk > 1.5
        assert_eq!(far_keys(&noob, (0.0, 0.9)), (0.0, 0.0)); // pure strafe needs sk ≥ 1.5

        // sk 2 (< 2.5): individual directions only — a forward diagonal drops the strafe,
        // but a pure strafe works.
        let low = XonSkill {
            skill: 2.0,
            axes: XonAxes::default(),
        };
        assert_eq!(far_keys(&low, (0.9, 0.9)), (1.0, 0.0));
        assert_eq!(far_keys(&low, (0.0, 0.9)), (0.0, 1.0));
        // Backward diagonal at sk < 4.5 drops the strafe too (`havocbot.qc:300-305`).
        assert_eq!(far_keys(&low, (-0.9, 0.9)), (-1.0, 0.0));

        // sk 5 (≥ 4.5): everything, diagonals included.
        assert_eq!(far_keys(&skill(5.0), (0.9, 0.9)), (1.0, 1.0));
        assert_eq!(far_keys(&skill(5.0), (-0.9, -0.9)), (-1.0, -1.0));
    }

    #[test]
    fn blend_by_distance() {
        let mut kb = KeyboardEmu::new();
        let mut rng = Lcg::new(1);
        let analog = (0.9, 0.4); // quantizes to keys (1, 0) at sk 5 far away... y below thr
        let far = kb.quantize(&mut rng, &skill(5.0), analog, 10_000.0, 0.1);
        assert_eq!(far, (1.0, 0.0), "far = pure keys");
        // Same held keys, at the goal: pure analog.
        let near = kb.quantize(&mut rng, &skill(5.0), analog, 0.0, 0.1);
        assert_eq!(near, analog, "dist 0 = pure analog");
        // Halfway: linear mix.
        let mid = kb.quantize(&mut rng, &skill(5.0), analog, BLEND_DIST / 2.0, 0.1);
        assert!((mid.0 - 0.95).abs() < 1e-3 && (mid.1 - 0.2).abs() < 1e-3);
    }

    #[test]
    fn rekey_cadence_holds_keys_between_updates() {
        // High-skill re-key period is tiny; low skill holds stale keys longer. Feed a
        // direction flip and count ticks until the keys follow.
        let flip_lag = |sk: f32| {
            let s = skill(sk);
            let mut kb = KeyboardEmu::new();
            let mut rng = Lcg::new(2);
            let _ = kb.quantize(&mut rng, &s, (0.9, 0.0), 10_000.0, 0.01);
            for tick in 1..200 {
                let out = kb.quantize(&mut rng, &s, (0.0, 0.9), 10_000.0, 0.01);
                if out.1 > 0.5 {
                    return tick;
                }
            }
            200
        };
        // Low skill can be slower to re-key (never faster) than high skill w/ same seed.
        assert!(flip_lag(1.9) >= flip_lag(9.0));
    }

    #[test]
    fn release_drops_held_keys_and_rekeys_immediately() {
        // Plan 63 B4: the hazard veto releases held keys; the very next quantize must
        // re-key from the fresh analog legs instead of replaying the stale hold.
        let s = skill(1.9); // slow cadence → keys would otherwise be held for many ticks
        let mut kb = KeyboardEmu::new();
        let mut rng = Lcg::new(4);
        let _ = kb.quantize(&mut rng, &s, (0.9, 0.0), 10_000.0, 0.01);
        assert_eq!(kb.keys, (1.0, 0.0), "forward key held");
        kb.release();
        assert_eq!(kb.keys, (0.0, 0.0), "release drops the held keys");
        // Next tick re-keys immediately from the new (reversed) analog intent.
        let out = kb.quantize(&mut rng, &s, (-0.9, 0.0), 10_000.0, 0.01);
        assert_eq!(out.0, -1.0, "re-keyed on the tick after release");
    }

    #[test]
    fn zero_keys_shortens_the_rekey_wait() {
        // A no-key frame must clamp the next re-key to ≤ 0.2 s so the bot can't freeze
        // (`havocbot.qc:329-331`).
        let s = skill(0.0); // slowest cadence
        let mut kb = KeyboardEmu::new();
        let mut rng = Lcg::new(3);
        let _ = kb.quantize(&mut rng, &s, (0.0, 0.0), 10_000.0, 0.01);
        // 0.25 s later a full-forward analog must be able to re-key.
        for _ in 0..25 {
            let _ = kb.quantize(&mut rng, &s, (0.9, 0.0), 10_000.0, 0.01);
        }
        let out = kb.quantize(&mut rng, &s, (0.9, 0.0), 10_000.0, 0.01);
        assert_eq!(out.0, 1.0, "must have re-keyed within ~0.2 s");
    }
}
