//! Xonotic havocbot core primitives (Plan 59) — pure, deterministic ports of the vendor's
//! decision math, consumed by the `xon` brain (Plan 60) and the `xg` navmode (Plan 61).
//!
//! Research: `context/distilled/xonotic.md`; vendor ground truth:
//! `vendor/xonotic/data/xonotic-data.pk3dir/qcsrc/server/bot/default/`. Every port cites its
//! vendor file:line. All randomness comes from the caller-owned [`Lcg`] so tests are seeded.

pub mod aim;
pub mod rating;

/// A tiny deterministic per-bot LCG (same constants as `Q3Brain::roll`) standing in for
/// QuakeC's `random()`. Callers own one per bot; tests seed it for reproducibility.
#[derive(Debug, Clone)]
pub struct Lcg(u32);

impl Lcg {
    /// Seeded generator (any seed; identical seeds replay identical rolls).
    pub fn new(seed: u32) -> Self {
        Self(seed ^ 0x9e37_79b9)
    }

    /// Next roll in `[0, 1)` — the vendor's `random()`.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / ((1u32 << 24) as f32)
    }
}

/// Wrap an angle difference to `[-180, 180)` degrees (the vendor's
/// `diffang.y -= floor(diffang.y / 360) * 360; if (>=180) -= 360` idiom, `aim.qc:221-223`).
pub fn wrap180(a: f32) -> f32 {
    let w = a - (a / 360.0).floor() * 360.0;
    if w >= 180.0 {
        w - 360.0
    } else {
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcg_is_deterministic_and_unit_range() {
        let mut a = Lcg::new(7);
        let mut b = Lcg::new(7);
        for _ in 0..1000 {
            let x = a.next();
            assert_eq!(x, b.next());
            assert!((0.0..1.0).contains(&x));
        }
    }

    #[test]
    fn wrap180_pins() {
        assert_eq!(wrap180(0.0), 0.0);
        assert_eq!(wrap180(179.0), 179.0);
        assert_eq!(wrap180(180.0), -180.0);
        assert_eq!(wrap180(360.0), 0.0);
        assert_eq!(wrap180(-190.0), 170.0);
        assert_eq!(wrap180(540.0), -180.0);
    }
}
