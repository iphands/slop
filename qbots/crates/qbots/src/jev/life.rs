//! # qbots::jev::life — per-life clock for the `secs_to_strong` metric (Plan 74 T7)
//!
//! Plan 74's headline number is how long a bot spends, after each spawn, before it first holds a
//! **strong** weapon — anything but the blaster, shotgun or machinegun, the decision state's own
//! "weak" set. `bot_task` feeds every alive frame to a [`LifeClock`] and ends the life on a death
//! or a level change; the record goes to the journal through [`super::hub::LifeSink`] for every
//! arm, so q3 is measured the same way as jev and jvr.

use brain::weapons::Weapon;

use super::hub::LifeRecord;

/// Is `held` a strong weapon? An unrecognised view model is not counted as strong.
pub fn is_strong(held: Option<Weapon>) -> bool {
    matches!(held, Some(w) if !matches!(w, Weapon::Blaster | Weapon::Shotgun | Weapon::Machinegun))
}

/// The clock for one bot's current life.
#[derive(Debug, Default)]
pub struct LifeClock {
    /// `(start time, frags at start)` once the first alive frame arrives.
    start: Option<(f32, i32)>,
    strong_at: Option<f32>,
    frags: i32,
}

impl LifeClock {
    /// Feed one alive frame: `now` in seconds on any monotonic base, the held weapon, frags.
    pub fn alive(&mut self, now: f32, held: Option<Weapon>, frags: i32) {
        let (t0, _) = *self.start.get_or_insert((now, frags));
        if self.strong_at.is_none() && is_strong(held) {
            self.strong_at = Some(now - t0);
        }
        self.frags = frags;
    }

    /// End the life (a death, or a level change when `died` is false): its record, or `None`
    /// if no alive frame was seen. The clock is reset for the next life.
    pub fn end(&mut self, now: f32, died: bool) -> Option<LifeRecord> {
        let (t0, f0) = self.start.take()?;
        let rec = LifeRecord {
            secs: now - t0,
            secs_to_strong: self.strong_at.take(),
            died,
            frags: self.frags - f0,
        };
        Some(rec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jev_life_clock_times_the_first_strong_weapon() {
        let mut c = LifeClock::default();
        assert_eq!(c.end(1.0, true), None, "no alive frame, no life");
        c.alive(10.0, Some(Weapon::Blaster), 3);
        c.alive(14.0, Some(Weapon::Shotgun), 3);
        c.alive(17.5, Some(Weapon::RocketLauncher), 4);
        c.alive(19.0, Some(Weapon::Blaster), 5); // switched back: the first time still counts
        let r = c.end(25.0, true).expect("a life");
        assert_eq!(
            r,
            LifeRecord {
                secs: 15.0,
                secs_to_strong: Some(7.5),
                died: true,
                frags: 2,
            }
        );
        // The next life starts fresh; never strong ⇒ None.
        c.alive(30.0, Some(Weapon::Machinegun), 5);
        let r = c.end(40.0, false).expect("a life");
        assert_eq!((r.secs_to_strong, r.died, r.frags), (None, false, 0));
        assert!(!is_strong(None) && is_strong(Some(Weapon::SuperShotgun)));
    }
}
