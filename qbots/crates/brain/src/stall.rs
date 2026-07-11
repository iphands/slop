//! Wall-press / stall episode detector (Plan 51).
//!
//! Brain-agnostic instrumentation for the LIVE fleet tick: detects sustained
//! "the brain is pushing but the body isn't moving" sequences and summarizes each
//! one as a single episode, which the caller logs as an `EVT wall_press` line.
//! Purely observational — never influences movement.
//!
//! A tick is *hindered* when the movement intent magnitude exceeds
//! [`INTENT_MIN`] while horizontal speed is below [`SPEED_STALL`] (walking speed
//! is 320 u/s; the stuck detector's deadband is 16 u/s — 40 catches hard
//! grinding without flagging normal acceleration/turn frames). An episode opens
//! after [`OPEN_AFTER_TICKS`] consecutive hindered ticks (~0.5 s at 10 Hz) and
//! closes after [`CLOSE_AFTER_TICKS`] consecutive free ticks — or immediately on
//! death, marking the episode `died` (the "sitting duck" signature).

use glam::Vec3;
use world::{CollisionModel, HULL_MAXS, HULL_MINS, MASK_SOLID};

use crate::steer::{view_forward, view_right};

/// Minimum intent magnitude (`sqrt(forward² + side²)`, each in [-1, 1]) for a
/// tick to count as "the brain is pushing".
pub const INTENT_MIN: f32 = 0.5;
/// Horizontal speed (u/s) below which a pushing bot counts as stalled.
pub const SPEED_STALL: f32 = 40.0;
/// Consecutive hindered ticks before an episode opens (~0.5 s at 10 Hz).
pub const OPEN_AFTER_TICKS: u32 = 5;
/// Consecutive free ticks before an open episode closes (~0.3 s at 10 Hz).
pub const CLOSE_AFTER_TICKS: u32 = 3;

/// Trace-origin lift so the wall probe clears steps pmove auto-climbs
/// (`pmove.c` STEPSIZE = 18) — a hit above this is a real wall, not a stair.
const PROBE_LIFT: f32 = 20.0;
/// Wall-probe length ahead of the hull (u).
const PROBE_DIST: f32 = 28.0;

/// True when the intent's world-space wish direction (yaw + forward/side, Q2
/// view basis) is blocked by a wall — a near-vertical face within
/// [`PROBE_DIST`] u — mirroring [`crate::recorder::CmWallProbe`]'s wall test.
/// Feed the result into [`StallSample::wall_blocked`] on hindered ticks.
pub fn wish_blocked(cm: &CollisionModel, pos: Vec3, yaw: f32, forward: f32, side: f32) -> bool {
    let dir = view_forward(yaw) * forward + view_right(yaw) * side;
    let Some(d) = dir.try_normalize() else {
        return false;
    };
    let start = [pos.x, pos.y, pos.z + PROBE_LIFT];
    let end = [
        start[0] + d.x * PROBE_DIST,
        start[1] + d.y * PROBE_DIST,
        start[2],
    ];
    let t = cm.trace(&start, &end, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
    t.startsolid || (t.fraction < 1.0 && t.plane.normal[2].abs() < 0.3)
}

/// One tick's observation, fed by the bot task after `brain.tick()`.
#[derive(Debug, Clone, Copy)]
pub struct StallSample {
    /// Bot origin this frame.
    pub pos: Vec3,
    /// Horizontal (XY) speed in u/s from the playerstate velocity.
    pub speed_h: f32,
    /// `sqrt(forward² + side²)` of the emitted [`crate::MovementIntent`].
    pub intent_mag: f32,
    /// The intent had `attack` set (combat proxy — firing this tick).
    pub attacking: bool,
    /// A short hull trace along the world-space wish direction hit a wall.
    /// Callers may only compute this on hindered ticks; pass `false` otherwise.
    pub wall_blocked: bool,
    /// Damage absorbed this tick (health drop, 0 if none).
    pub damage: i32,
    /// Health > 0 this frame. A dead frame force-closes any open episode.
    pub alive: bool,
    /// Measured frame delta in seconds.
    pub dt: f32,
}

/// Summary of one closed stall episode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StallEpisode {
    /// Where the stall began.
    pub start_pos: Vec3,
    /// Episode length in seconds (sum of sample `dt`, trailing free ticks included).
    pub secs: f32,
    /// Ticks accumulated (hindered + intra-episode free ticks).
    pub ticks: u32,
    /// Mean horizontal speed across the episode (u/s).
    pub mean_speed: f32,
    /// Ticks with `attacking` set.
    pub attack_ticks: u32,
    /// Ticks with `wall_blocked` set.
    pub wall_ticks: u32,
    /// Total damage absorbed during the episode.
    pub damage: i32,
    /// The episode ended because the bot died in it.
    pub died: bool,
}

/// Per-bot episode state machine. Feed [`tick`](Self::tick) once per frame.
#[derive(Debug, Clone, Default)]
pub struct StallMonitor {
    /// Consecutive hindered ticks while no episode is open (provisional run).
    pending: u32,
    /// Accumulators start at the FIRST hindered tick of the provisional run so
    /// an opened episode includes its lead-in.
    acc: Option<Acc>,
    /// Whether the episode has crossed [`OPEN_AFTER_TICKS`] (i.e. is real).
    open: bool,
    /// Consecutive free ticks while open (close countdown).
    free_run: u32,
}

#[derive(Debug, Clone, Copy)]
struct Acc {
    start_pos: Vec3,
    secs: f32,
    ticks: u32,
    speed_sum: f32,
    attack_ticks: u32,
    wall_ticks: u32,
    damage: i32,
}

impl Acc {
    fn new(pos: Vec3) -> Self {
        Self {
            start_pos: pos,
            secs: 0.0,
            ticks: 0,
            speed_sum: 0.0,
            attack_ticks: 0,
            wall_ticks: 0,
            damage: 0,
        }
    }

    fn add(&mut self, s: &StallSample) {
        self.secs += s.dt;
        self.ticks += 1;
        self.speed_sum += s.speed_h;
        self.attack_ticks += u32::from(s.attacking);
        self.wall_ticks += u32::from(s.wall_blocked);
        self.damage += s.damage.max(0);
    }

    fn finish(self, died: bool) -> StallEpisode {
        StallEpisode {
            start_pos: self.start_pos,
            secs: self.secs,
            ticks: self.ticks,
            mean_speed: self.speed_sum / self.ticks.max(1) as f32,
            attack_ticks: self.attack_ticks,
            wall_ticks: self.wall_ticks,
            damage: self.damage,
            died,
        }
    }
}

impl StallMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    /// True while a (confirmed) episode is open — the caller may use this to
    /// gate extra per-tick diagnostics.
    pub fn in_episode(&self) -> bool {
        self.open
    }

    /// Feed one frame. Returns a completed episode when one closes this tick.
    pub fn tick(&mut self, s: StallSample) -> Option<StallEpisode> {
        if !s.alive {
            // Death: close a confirmed episode as `died`; drop any provisional run.
            let ep = self.open.then(|| {
                self.acc
                    .take()
                    .expect("open episode always has accumulators")
                    .finish(true)
            });
            self.reset();
            return ep;
        }

        let hindered = s.intent_mag > INTENT_MIN && s.speed_h < SPEED_STALL;

        if self.open {
            self.acc
                .as_mut()
                .expect("open episode always has accumulators")
                .add(&s);
            if hindered {
                self.free_run = 0;
            } else {
                self.free_run += 1;
                if self.free_run >= CLOSE_AFTER_TICKS {
                    let ep = self
                        .acc
                        .take()
                        .expect("open episode always has accumulators")
                        .finish(false);
                    self.reset();
                    return Some(ep);
                }
            }
            return None;
        }

        if hindered {
            self.acc.get_or_insert_with(|| Acc::new(s.pos)).add(&s);
            self.pending += 1;
            if self.pending >= OPEN_AFTER_TICKS {
                self.open = true;
                self.free_run = 0;
            }
        } else {
            // Provisional run broken before confirmation — discard silently.
            self.pending = 0;
            self.acc = None;
        }
        None
    }

    fn reset(&mut self) {
        self.pending = 0;
        self.acc = None;
        self.open = false;
        self.free_run = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hindered() -> StallSample {
        StallSample {
            pos: Vec3::new(10.0, 20.0, 30.0),
            speed_h: 5.0,
            intent_mag: 1.0,
            attacking: false,
            wall_blocked: true,
            damage: 0,
            alive: true,
            dt: 0.1,
        }
    }

    fn free() -> StallSample {
        StallSample {
            speed_h: 250.0,
            wall_blocked: false,
            ..hindered()
        }
    }

    #[test]
    fn opens_only_after_threshold_and_closes_on_recovery() {
        let mut m = StallMonitor::new();
        // 4 hindered ticks then a free tick: provisional run discarded, no episode.
        for _ in 0..4 {
            assert_eq!(m.tick(hindered()), None);
        }
        assert!(!m.in_episode());
        assert_eq!(m.tick(free()), None);

        // 5 hindered ticks confirm an episode; 3 free ticks close it.
        for _ in 0..5 {
            assert_eq!(m.tick(hindered()), None);
        }
        assert!(m.in_episode());
        assert_eq!(m.tick(free()), None);
        assert_eq!(m.tick(free()), None);
        let ep = m.tick(free()).expect("third free tick closes");
        assert!(!ep.died);
        assert_eq!(ep.ticks, 8, "5 hindered + 3 free ticks accumulated");
        assert!((ep.secs - 0.8).abs() < 1e-4);
        assert_eq!(ep.wall_ticks, 5);
        assert_eq!(ep.start_pos, Vec3::new(10.0, 20.0, 30.0));
        assert!(!m.in_episode(), "monitor resets after close");
    }

    #[test]
    fn a_single_free_tick_does_not_close_an_episode() {
        let mut m = StallMonitor::new();
        for _ in 0..5 {
            m.tick(hindered());
        }
        assert_eq!(m.tick(free()), None);
        assert_eq!(m.tick(hindered()), None, "free-run counter resets");
        assert!(m.in_episode());
    }

    #[test]
    fn death_closes_with_died_flag_and_accumulates_damage() {
        let mut m = StallMonitor::new();
        for _ in 0..5 {
            m.tick(hindered());
        }
        m.tick(StallSample {
            attacking: true,
            damage: 25,
            ..hindered()
        });
        let ep = m
            .tick(StallSample {
                alive: false,
                ..hindered()
            })
            .expect("death closes the open episode");
        assert!(ep.died);
        assert_eq!(ep.damage, 25);
        assert_eq!(ep.attack_ticks, 1);
        // The dead sample itself is not accumulated.
        assert_eq!(ep.ticks, 6);
    }

    #[test]
    fn death_without_open_episode_emits_nothing() {
        let mut m = StallMonitor::new();
        for _ in 0..3 {
            m.tick(hindered()); // provisional only
        }
        assert_eq!(
            m.tick(StallSample {
                alive: false,
                ..hindered()
            }),
            None
        );
        assert!(!m.in_episode());
    }

    #[test]
    fn mean_speed_is_averaged_over_all_accumulated_ticks() {
        let mut m = StallMonitor::new();
        for _ in 0..5 {
            m.tick(StallSample {
                speed_h: 10.0,
                ..hindered()
            });
        }
        for _ in 0..2 {
            m.tick(free()); // 250 u/s
        }
        let ep = m.tick(free()).expect("closes");
        let expected = (5.0 * 10.0 + 3.0 * 250.0) / 8.0;
        assert!((ep.mean_speed - expected).abs() < 1e-3);
    }
}
