//! # brain::engage — the winning/losing read for engagement decisions (Plan 29)
//!
//! To "chase for the kill" or "break off a losing fight" a bot needs to know whether it is
//! **winning**. Q2 gives us almost nothing to judge that with: the enemy's health and weapon are
//! **not on the wire** (`pitfalls.md`). So we infer it from what we *can* see — our own state:
//!
//! - **pressure** `[0,1]`: are WE landing fire? A proxy — accumulates while we fire with LOS on the
//!   target (enemy pain sounds would sharpen this but aren't reliably transmitted), decays when we
//!   aren't. High pressure ≈ we're dictating the fight.
//! - **losing**: are we taking sustained damage without answering it? Our health drop IS visible.
//!
//! [`EngageTracker`] holds the per-engagement state; [`EngageRead`] is the per-tick output the
//! chase/disengage gates consume. Reset it when the target changes.

/// The per-tick engagement read consumed by `main`'s chase/disengage gates (Plan 29).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngageRead {
    /// How much pressure WE are applying, `[0,1]` (fire-on-target proxy — enemy health not visible).
    pub pressure: f32,
    /// The fight is going against us: sustained incoming damage with little pressure of our own.
    pub losing: bool,
}

/// How fast `pressure` rises per second of on-target fire, and falls otherwise.
const PRESSURE_RISE: f32 = 1.5;
const PRESSURE_FALL: f32 = 0.8;
/// Seconds of recent damage that, with low pressure, marks the fight "losing".
const HURT_LOSING_SECS: f32 = 1.2;
/// How fast the hurt streak decays when we stop taking damage.
const HURT_DECAY: f32 = 1.0;
/// Below this pressure, sustained damage means we're losing (we're eating shots, not trading).
const LOSING_PRESSURE_CEIL: f32 = 0.35;

/// Per-engagement winning/losing estimator (Plan 29 T1). Owned by the brain; updated once per
/// combat tick and reset when the target changes.
#[derive(Debug, Clone, Copy, Default)]
pub struct EngageTracker {
    /// Accumulated fire-on-target pressure, `[0,1]`.
    pressure: f32,
    /// Seconds of recent damage (rises on hits, decays otherwise).
    hurt_streak: f32,
}

impl EngageTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget the current engagement (target changed / fight ended). Next `update` starts fresh.
    pub fn reset(&mut self) {
        self.pressure = 0.0;
        self.hurt_streak = 0.0;
    }

    /// Advance one combat tick. `landed` = we fired at the target with LOS this tick (our pressure
    /// proxy); `took_damage` = our health dropped this tick; `dt` = seconds. Returns the read.
    pub fn update(&mut self, landed: bool, took_damage: bool, dt: f32) -> EngageRead {
        let dt = dt.clamp(0.0, 0.5); // guard against a huge first-frame dt
        self.pressure += if landed {
            PRESSURE_RISE * dt
        } else {
            -PRESSURE_FALL * dt
        };
        self.pressure = self.pressure.clamp(0.0, 1.0);

        self.hurt_streak += if took_damage { dt } else { -HURT_DECAY * dt };
        self.hurt_streak = self.hurt_streak.max(0.0);

        EngageRead {
            pressure: self.pressure,
            losing: self.hurt_streak >= HURT_LOSING_SECS && self.pressure < LOSING_PRESSURE_CEIL,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Firing on-target with no damage taken → pressure climbs, not losing.
    #[test]
    fn sustained_on_target_fire_builds_pressure() {
        let mut t = EngageTracker::new();
        let mut read = EngageRead {
            pressure: 0.0,
            losing: true,
        };
        for _ in 0..20 {
            read = t.update(true, false, 0.1);
        }
        assert!(read.pressure > 0.8, "pressure builds: {}", read.pressure);
        assert!(!read.losing, "we're dictating — not losing");
    }

    /// Taking sustained damage while not landing our own fire → losing.
    #[test]
    fn eating_shots_without_answering_is_losing() {
        let mut t = EngageTracker::new();
        let mut read = t.update(false, true, 0.1);
        for _ in 0..20 {
            read = t.update(false, true, 0.1);
        }
        assert!(read.losing, "sustained damage + no pressure → losing");
        assert!(read.pressure < 0.1);
    }

    /// A brief hit while we're pressuring hard does NOT flip us to losing.
    #[test]
    fn winning_hard_survives_a_stray_hit() {
        let mut t = EngageTracker::new();
        for _ in 0..20 {
            t.update(true, false, 0.1);
        }
        let read = t.update(true, true, 0.1); // one hit while dominating
        assert!(!read.losing, "one hit while winning isn't 'losing'");
    }

    /// Reset clears the read.
    #[test]
    fn reset_clears_state() {
        let mut t = EngageTracker::new();
        for _ in 0..20 {
            t.update(false, true, 0.1);
        }
        t.reset();
        let read = t.update(false, false, 0.1);
        assert!(!read.losing);
        assert!(read.pressure < 0.05);
    }
}
