//! # brain::traverse — the shared traversal executor (Plan 46)
//!
//! Ladder climbs, swimming / water-exit, and moving-platform (train / lift) rides used to live
//! as **per-brain copies** that drifted: `RunTesterBrain` had the full set (the only ladder
//! machine + the stateful board/carry ride lock), `MainBrain` had swim + a *stateless* generic
//! ride and **no ladders**, and `Q3Brain` had **nothing** — a `q3` bot in a live match could not
//! swim, ride, or climb. This module lifts the **best** copy of each machine (all three are
//! `RunTesterBrain`'s — it is the behavior-preservation regression anchor, Plan 46 T2) into one
//! [`TraversalExecutor`] every brain delegates to.
//!
//! ## Contract
//! Movement is **owned by the executor while a traversal edge is active**; aim (the view) is
//! steered by the executor too (you cannot free-aim while climbing a ladder — the bot fires along
//! the traversal heading, which the plan accepts for v1). The brain keeps its *fire decision*
//! (the attack button) — the executor never touches `attack`.
//!
//! Each tick the brain:
//! 1. computes its normal steering (`view_yaw`, `steer_fwd`, `steer_side`),
//! 2. calls [`TraversalExecutor::gates`] to learn whether it is swimming / riding — and
//!    **suspends stuck-recovery + jump-edge activation** while either holds (a stand-and-wait on
//!    a lift or a slow surface bob is not a wedge; recovery would false-fire and steer away),
//! 3. runs its normal move/recovery only when *not* traversing, then
//! 4. calls [`TraversalExecutor::apply`], which **overwrites** the movement axes when a traversal
//!    is active and returns the recorder flag (`'S'` swim, `'P'` ride, `'L'` ladder).
//!
//! `ride.rs` / `water.rs` stay the pure helpers they always were; this module is the stateful
//! sequencer over them (the `active_ride` edge lock + `ride_boarded` carry state + swim
//! `exit_ticks` climb-out hysteresis).

use glam::Vec3;
use world::{CollisionModel, RideInfo};

use crate::move_ctrl::MovementIntent;
use crate::nav_mode::Navigator;
use crate::perception::Worldview;
use crate::steer::move_from_world_dir;
use crate::water::{
    is_swimming, water_level, EXIT_HYSTERESIS_TICKS, EXIT_LOOKUP_PITCH, SWIM_VERT_SCALE,
};

/// The `PMF_ON_GROUND` bit in the playerstate `pm_flags` — "standing on solid ground this frame"
/// (`SelfState::flags`, a `u32`).
const PMF_ON_GROUND: u32 = 4;

/// The swim / ride gates for one frame — the brain suspends recovery + jump-edge activation while
/// either holds. Returned by [`TraversalExecutor::gates`], consumed as a plain read.
#[derive(Debug, Clone, Copy)]
pub struct TraversalGates {
    /// In/at water (self water-level ≥ swim threshold) or on a swim edge.
    pub swimming: bool,
    /// On a ride edge, or mid-ride (boarded) — the platform is carrying us.
    pub ride_active: bool,
}

impl TraversalGates {
    /// Either traversal mode is engaged this frame.
    pub fn any(&self) -> bool {
        self.swimming || self.ride_active
    }
}

/// The per-frame inputs a traversal override reads (everything except the navigator, which the
/// brain also borrows mutably elsewhere this tick so it is passed separately to keep the borrows
/// non-overlapping). Bundled so [`TraversalExecutor::apply`] stays a two-argument call.
pub struct TraversalFrame<'a> {
    /// This frame's perceived world (self playerstate + PVS entities).
    pub view: &'a Worldview,
    /// Collision model — for the water-level sample. `None` before the map loads (water samples
    /// then read as 0 / dry, matching the brains' own `cm.map_or(0, …)` fallback).
    pub cm: Option<&'a CollisionModel>,
    /// Bot origin this frame.
    pub pos: Vec3,
    /// The brain's already-steered view yaw (movement is view-relative).
    pub view_yaw: f32,
    /// The brain's horizontal steering forward/side (the swim machine reuses these).
    pub steer_fwd: f32,
    pub steer_side: f32,
    /// Seconds this frame covers — drives the lift de-conflict timers (Plan 31).
    pub dt: f32,
}

/// The vertical-lift de-conflict phase (Plan 31): wait clear of the shaft → enter → back off
/// when yielded/pinned, with jittered retry.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum LiftPhase {
    #[default]
    WaitClear,
    Enter,
    BackOff,
}

/// Stateful traversal sequencer shared by every brain (Plan 46). Owns only the small amount of
/// state a traversal needs to persist across frames; the geometry helpers live in `ride.rs` /
/// `water.rs`.
#[derive(Debug, Default)]
pub struct TraversalExecutor {
    /// Ticks remaining in water-exit climb-out mode (Plan 40 T3): look-up + forward + `up` to
    /// trigger Q2's water-jump onto a dry ledge. Held a few ticks so the bot clears the lip.
    exit_ticks: u32,
    /// True once the bot has stepped onto a moving train (Plan 43): it then HOLDS and lets the
    /// train carry it, stepping off only when the train nears the far corner. Without this the
    /// bot would keep steering at the dismount and walk off the moving platform into the pit.
    ride_boarded: bool,
    /// The ride edge we are mid-traversal on (Plan 35). Stored when boarding so the ride stays
    /// LOCKED active until dismount even if the navigator advances off the ride edge while the
    /// platform carries us (which happened over the central lava — the nav advanced, `ride_active`
    /// went false, and the bot fell off mid-transit).
    active_ride: Option<RideInfo>,
    /// Client-side air clock (Plan 32): ticked every `gates()` call, drives the surface-seek
    /// override in the swim machine so the bot breathes before the server's 12s runs out.
    air: crate::water::AirClock,
    /// The traversal flag applied last frame (`'S'`/`'P'`/`'L'`), for the falling-edge
    /// `EVT traverse done` counter (Plan 47 T1): emitted when a traversal leg ends.
    last_flag: Option<char>,
    /// Vertical-lift de-conflict phase + its timer (Plan 31).
    lift_phase: LiftPhase,
    lift_timer: f32,
}

impl TraversalExecutor {
    /// Fresh executor (no traversal in progress).
    pub fn new() -> Self {
        Self::default()
    }

    /// Compute this frame's swim/ride gates. Call **once, before stuck-recovery**, and skip
    /// recovery + jump-edge activation whenever [`TraversalGates::any`] holds. Also resets the
    /// boarded-ride lock when no ride edge is active (defensive — matches runtester's else-branch:
    /// a ride that ends without going through a dismount branch must not stay latched).
    pub fn gates(
        &mut self,
        nav: &dyn Navigator,
        cm: Option<&CollisionModel>,
        pos: Vec3,
        dt: f32,
    ) -> TraversalGates {
        let level = cm.map_or(0, |c| water_level(c, pos));
        let swimming = is_swimming(level) || nav.current_edge_is_swim();
        // Air clock (Plan 32): ticked here so it advances every frame regardless of which
        // traversal branch runs. Eyes-under = level 3; any breathable frame resets it.
        self.air.tick(level == 3, dt);
        let ride_active = nav.current_edge_is_ride() || self.ride_boarded;
        if !ride_active {
            self.ride_boarded = false;
            self.active_ride = None;
        }
        // EVT counter (Plan 47 T1): a traversal leg just ended (falling edge of any traversal
        // mode) — greppable proof of completed swims / rides / ladder climbs.
        if !swimming && !ride_active {
            if let Some(k) = self.last_flag.take() {
                let kind = match k {
                    'S' => "swim",
                    'L' => "ladder",
                    _ => "ride",
                };
                tracing::info!(kind, "EVT traverse done");
            }
        }
        TraversalGates {
            swimming,
            ride_active,
        }
    }

    /// The server dealt us damage underwater that combat can't explain — drown damage. Re-sync
    /// the air clock to "out of air" so the surface-seek engages immediately (Plan 32 Risk #1).
    pub fn on_underwater_damage(&mut self) {
        tracing::info!("EVT drown"); // Plan 47 T1 counter: the acceptance gate wants this at ZERO
        self.air.on_unexplained_damage();
    }

    /// Apply the traversal movement override into `mv` when a traversal is active this frame.
    ///
    /// Priority: **ride overrides swim** (a ride edge in water rides). Returns the recorder flag
    /// (`'P'` ride, `'L'` ladder, `'S'` swim) and the forward-progress intent (for the recorder's
    /// `H` flag) via [`TraversalApply`]; returns `None` when no traversal is active (the brain's
    /// own steering / recovery output stands).
    ///
    /// `view_yaw`, `steer_fwd`, `steer_side` are the brain's already-computed steering (the swim
    /// machine reuses the horizontal steering; the ride machine converts world directions to
    /// view-relative movement via `view_yaw`).
    pub fn apply(
        &mut self,
        mv: &mut MovementIntent,
        gates: TraversalGates,
        nav: &dyn Navigator,
        frame: &TraversalFrame,
    ) -> Option<TraversalApply> {
        let result = if gates.ride_active {
            self.apply_ride(mv, nav, frame)
        } else if gates.swimming {
            Some(self.apply_swim(mv, nav, frame))
        } else {
            None
        };
        // Remember the active traversal kind for the falling-edge `EVT traverse done` counter.
        if let Some(a) = &result {
            self.last_flag = Some(a.flag);
        }
        result
    }

    /// Swim toward the 3-D look-ahead (Plan 40 T2/T3), lifted verbatim from `RunTesterBrain`.
    /// Sustained vertical thrust (never `jump()` in water — a one-shot launch); Q2 water-jump
    /// climb-out onto a dry ledge via `exit_ticks` hysteresis.
    fn apply_swim(
        &mut self,
        mv: &mut MovementIntent,
        nav: &dyn Navigator,
        frame: &TraversalFrame,
    ) -> TraversalApply {
        let TraversalFrame {
            cm,
            pos,
            view_yaw,
            steer_fwd,
            steer_side,
            ..
        } = *frame;
        let water = cm.map_or(0, |c| water_level(c, pos));

        // ── Surface-seek override (Plan 32 T2) ────────────────────────────────────────────
        // Air is critical: abandon the current underwater leg and swim STRAIGHT UP for a breath.
        // Priority above normal swim steering (a dead bot completes no path); the climb-out /
        // path exit logic below resumes once we've breathed (any level<3 frame resets the clock).
        if water == 3 {
            let tts = cm.map_or(0.0, |c| crate::water::time_to_surface(c, pos));
            if self.air.must_surface(tts) {
                tracing::debug!(tts, "air critical — surfacing for a breath");
                mv.look_at(view_yaw, -70.0); // pitch hard up: pml.forward carries us to air
                mv.move_forward(1.0);
                mv.move_side(0.0);
                mv.up = 1.0; // full sustained up-thrust (never jump() in water)
                mv.jump = false;
                return TraversalApply {
                    flag: 'S',
                    intent_forward: 1.0,
                };
            }
        }

        // Use the RAW look-ahead (no floor validation — there's no floor underwater for
        // `pursue_target_safe` to confirm).
        let target = nav
            .pursue_target(pos)
            .or_else(|| nav.current_waypoint_pos().map(Vec3::from))
            .unwrap_or(pos);
        let to = target - pos;
        let hd = to.truncate().length();
        let dz = to.z;
        // Is the target a dry node above us (a water→ledge exit, the railgun climb-out)?
        let target_dry = cm.map_or(0, |c| water_level(c, target)) == 0;
        if nav.current_edge_is_swim() && target_dry && dz > 0.0 {
            self.exit_ticks = EXIT_HYSTERESIS_TICKS;
        }
        if water == 0 {
            self.exit_ticks = 0; // fully out — stop forcing climb-out
        }

        // Vertical thrust: sustained (NEVER `mv.jump()` in water — that's a one-shot launch).
        let pitch;
        if self.exit_ticks > 0 {
            // Q2 water-jump climb-out: look up past -15°, hold up, press forward into the lip.
            self.exit_ticks -= 1;
            mv.up = 1.0;
            pitch = EXIT_LOOKUP_PITCH;
        } else {
            mv.up = (dz / SWIM_VERT_SCALE).clamp(-1.0, 1.0);
            // Pitch toward the 3-D target so `pml.forward` carries the vertical component too.
            pitch = (-dz.atan2(hd.max(1.0)).to_degrees()).clamp(-89.0, 89.0);
        }
        // Forward toward the XY target (water is open volume — no narrow-ledge speed_scale).
        let swim_fwd = if steer_fwd != 0.0 || steer_side != 0.0 {
            steer_fwd.max(0.0)
        } else {
            1.0
        };
        mv.look_at(view_yaw, pitch);
        mv.move_forward(swim_fwd);
        mv.move_side(steer_side);
        mv.jump = false;
        TraversalApply {
            flag: 'S',
            intent_forward: swim_fwd,
        }
    }

    /// Ride a moving platform: stateful board → carry → dismount, plus the ladder branch. Lifted
    /// verbatim from `RunTesterBrain` (the only correct copy). Returns `None` if the ride edge has
    /// no [`RideInfo`] (nothing to drive — the brain's steering stands).
    fn apply_ride(
        &mut self,
        mv: &mut MovementIntent,
        nav: &dyn Navigator,
        frame: &TraversalFrame,
    ) -> Option<TraversalApply> {
        let TraversalFrame {
            view,
            pos,
            view_yaw,
            ..
        } = *frame;
        // Prefer the nav's current ride edge; fall back to the one we stored when boarding so a
        // mid-transit nav advance can't strand us on the moving platform.
        let info = nav.current_ride_info().or(self.active_ride)?;
        let board = Vec3::from(info.board);
        let dismount = Vec3::from(info.dismount);
        let intent_forward: f32;
        let go = |mv: &mut MovementIntent, target: Vec3| -> f32 {
            let to = target - pos;
            let dir = if to.length_squared() > 1e-6 {
                to.normalize_or_zero()
            } else {
                Vec3::ZERO
            };
            let (f, s) = move_from_world_dir(dir, view_yaw, true);
            mv.look_at(view_yaw, 0.0);
            mv.move_forward(f);
            mv.move_side(s);
            f
        };
        let flag = if info.ladder { 'L' } else { 'P' };
        if info.ladder {
            // Ladder climb (Plan 35): hug the ladder and press `up` (Q2 `PM_AddCurrents` ladder
            // rule: `upmove>0` while touching CONTENTS_LADDER → climb). Face the ladder center so
            // the 1u forward trace hits it (sets `pml.ladder`), press forward into it, and hold
            // `up` until at the top, then step off.
            let center = Vec3::from(info.board_ent); // ladder center (facing target)
                                                     // The ladder edge is bidirectional: `dismount` is the EXIT level for THIS
                                                     // direction (above the bot when ascending, below when descending). Drive
                                                     // toward it — `up>0` climbs, `up<0` descends (Q2 `PM_AddCurrents`).
            let dz = dismount.z - pos.z;
            let horiz_to_exit = (pos.truncate() - dismount.truncate()).length();
            if dz.abs() < 20.0 && horiz_to_exit < 40.0 {
                intent_forward = go(mv, dismount); // on the exit ledge → step off
            } else if dz > 0.0 {
                // ASCENDING. Face the EXIT ledge (the dismount), not the ladder center: the ladder
                // sits between us and the exit, so the 1u forward trace still hits CONTENTS_LADDER
                // (sets `pml.ladder`, enabling `up`), but climbing "into the exit" carries us
                // up-and-OVER onto the top ledge instead of topping out on the wrong (entry) side
                // of the shaft and falling. JUMP near the top to clear the lip onto the ledge.
                let to_x = dismount - pos;
                let lyaw = to_x.y.atan2(to_x.x).to_degrees();
                mv.look_at(lyaw, 0.0);
                mv.move_forward(1.0);
                mv.move_side(0.0);
                intent_forward = 1.0;
                mv.up = 1.0; // `upmove>0` climbs (Q2 `PM_AddCurrents`)
                mv.jump = dz < 24.0; // near the top → hop onto the ledge
            } else {
                // DESCENDING. Face the ladder center to stay on it while going down.
                let to_c = center - pos;
                let lyaw = to_c.y.atan2(to_c.x).to_degrees();
                mv.look_at(lyaw, 0.0);
                mv.move_forward(1.0);
                mv.move_side(0.0);
                intent_forward = 1.0;
                if pos.z > board.z - 24.0 {
                    // DESCENDING but still standing on the top floor next to the shaft: the floor
                    // holds us up, so `up<0` does nothing. JUMP forward off the edge INTO the shaft
                    // (CONTENTS_LADDER is open) to start the descent.
                    mv.up = 0.0;
                    mv.jump = true;
                } else {
                    // In the shaft, below the top → climb down (`upmove<0`).
                    mv.up = -1.0;
                    mv.jump = false;
                }
            }
            self.ride_boarded = false;
        } else if info.vertical {
            // Vertical lift (Plan 31 de-conflict): a body ANYWHERE in the shaft re-arms Q2's
            // `Touch_Plat_Center` go-down timer (`g_func.c`), so crowding the pad pins the lift
            // and starves the queue — the classic multi-bot deadlock the old `ELEVATOR_PENALTY`
            // hack dodged. The machine:
            //   WaitClear — hold at a standoff OUTSIDE the shaft while it's occupied or the pad
            //               is visibly away; a blind timeout (~4s) proceeds anyway (PVS may hide
            //               both signals; entering is what summons a plat).
            //   Enter     — walk onto the pad (hop like a human). If the pad hasn't lifted us
            //               within ~5s (someone upstairs is pinning it), BACK OFF — leaving the
            //               trigger is precisely what lets a pinned plat descend.
            //   BackOff   — hold at the standoff for a jittered 2–4s (jitter breaks two bots'
            //               yield-loop symmetry), then retry.
            // Once rising (z well above the board), steer for the top center; the ride edge
            // completes when the navigator advances at the top, and route continuation walks the
            // bot OFF the pad promptly (no dwell — dwelling re-arms the stay-up timer).
            let board_horiz = (pos.truncate() - board.truncate()).length();
            let rising = pos.z > board.z + 32.0;
            self.lift_timer += frame.dt;
            if rising {
                self.lift_phase = LiftPhase::WaitClear;
                self.lift_timer = 0.0;
                intent_forward = go(mv, dismount);
            } else {
                // Standoff: 64u back from the pad along OUR approach direction (each bot's own
                // side — queued bots naturally spread instead of stacking).
                let away = (pos - board).truncate();
                let out = if away.length() > 1.0 {
                    away.normalize() * 64.0
                } else {
                    glam::Vec2::new(64.0, 0.0)
                };
                let standoff = Vec3::new(board.x + out.x, board.y + out.y, pos.z);
                let occupied = crate::ride::shaft_occupied(view, &info, pos);
                let pad_down = crate::ride::plat_at_bottom(view, &info);
                match self.lift_phase {
                    LiftPhase::WaitClear => {
                        if !occupied && (pad_down || self.lift_timer > 4.0) {
                            self.lift_phase = LiftPhase::Enter;
                            self.lift_timer = 0.0;
                            intent_forward = go(mv, board);
                        } else {
                            intent_forward = go(mv, standoff); // hold clear of the trigger
                            if self.lift_timer > 8.0 {
                                self.lift_timer = 0.0; // periodic re-check keeps the timers sane
                            }
                        }
                    }
                    LiftPhase::Enter => {
                        if occupied {
                            // Someone claimed it while we approached — yield.
                            tracing::info!("EVT lift_yield reason=occupied");
                            self.lift_phase = LiftPhase::BackOff;
                            self.lift_timer = 0.0;
                            intent_forward = go(mv, standoff);
                        } else if self.lift_timer > 5.0 {
                            // The pad never lifted us — someone unseen is pinning it up. Leaving
                            // the trigger volume is what allows it to come down.
                            tracing::info!("EVT lift_yield reason=pinned");
                            self.lift_phase = LiftPhase::BackOff;
                            self.lift_timer = 0.0;
                            intent_forward = go(mv, standoff);
                        } else {
                            if board_horiz > 32.0 {
                                mv.jump(); // hop on like a human (Plan 43 T7)
                            }
                            intent_forward = go(mv, dismount);
                        }
                    }
                    LiftPhase::BackOff => {
                        // Jittered hold (2–4s): derive the jitter from our standoff spot so two
                        // symmetric bots don't re-approach in lockstep.
                        let jitter = ((pos.x + pos.y).abs() % 32.0) / 16.0; // [0,2)
                        if self.lift_timer > 2.0 + jitter {
                            self.lift_phase = LiftPhase::WaitClear;
                            self.lift_timer = 0.0;
                        }
                        intent_forward = go(mv, standoff);
                    }
                }
            }
            self.ride_boarded = false;
        } else {
            let board_horiz = (pos.truncate() - board.truncate()).length();
            let train_here = crate::ride::platform_present(view, Vec3::from(info.board_ent));
            let train_far = crate::ride::platform_present(view, Vec3::from(info.far_ent));
            if !self.ride_boarded {
                // Are we ACTUALLY standing on the platform deck right now? (grounded, near its live
                // top-center horizontally, at its height.) Only THEN commit to the ride — committing
                // the instant `train_here` fired left the bot frozen on the board ledge (zero-input
                // carry) while the platform left without it.
                let stand = crate::ride::train_stand_now(view, &info);
                let grounded = view.self_state().flags & PMF_ON_GROUND != 0;
                let on_deck = stand.is_some_and(|s| {
                    grounded
                        && (pos.truncate() - s.truncate()).length() < 56.0
                        && (pos.z - s.z).abs() < 36.0
                });
                if on_deck {
                    self.ride_boarded = true;
                    self.active_ride = Some(info); // lock the ride until dismount
                    mv.move_forward(0.0); // carried from here — stand still
                    mv.move_side(0.0);
                    intent_forward = 0.0;
                } else if board_horiz > 48.0 {
                    intent_forward = go(mv, board); // walk to the board ledge, wait
                } else if let Some(s) = stand.filter(|_| train_here) {
                    // Platform is at the near corner → HOP onto its deck (T7: jump the gap). Aim at
                    // the live top-center; the jump clears the ~33u lava gap and we commit `on_deck`
                    // the instant we land grounded on it.
                    intent_forward = go(mv, s);
                    mv.jump();
                } else {
                    mv.move_forward(0.0); // platform not here yet — wait at the ledge
                    mv.move_side(0.0);
                    intent_forward = 0.0;
                }
            } else if (pos - dismount).truncate().length() < 48.0
                && (view.self_state().flags & PMF_ON_GROUND != 0)
            {
                // We've made it onto the dismount ledge (grounded near it) → ride DONE.
                self.ride_boarded = false;
                self.active_ride = None;
                intent_forward = go(mv, dismount);
            } else if pos.z < dismount.z - 96.0 {
                // Fell off into the pit/lava → abandon the ride; let respawn/nav recover.
                self.ride_boarded = false;
                self.active_ride = None;
                intent_forward = 0.0;
            } else if train_far {
                // Train at the far corner (near the quad) → JUMP off onto the dismount ledge.
                intent_forward = go(mv, dismount);
                mv.jump();
            } else {
                // Boarded, mid-transit → DO NOTHING. A Q2 func_train PUSHES riders, so zero input
                // keeps us perfectly carried (verified: z holds steady on *10). ANY movement
                // (chasing the moving center) slid us off the small platform into the lava — the
                // winning move is to stand still and let it carry us to the far corner, where
                // `train_far` above fires the dismount.
                mv.move_forward(0.0);
                mv.move_side(0.0);
                mv.jump = false;
                intent_forward = 0.0;
            }
        }
        Some(TraversalApply {
            flag,
            intent_forward,
        })
    }
}

/// The result of an active traversal frame: the recorder flag + forward-progress intent.
#[derive(Debug, Clone, Copy)]
pub struct TraversalApply {
    /// Recorder flag char: `'S'` swimming, `'P'` riding a platform/lift/train, `'L'` on a ladder.
    pub flag: char,
    /// Forward-progress intent for the recorder's hindered (`H`) flag — `0.0` while deliberately
    /// standing still (waiting for / carried by a platform), the nav-step forward otherwise.
    pub intent_forward: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nav_mode::Navigator;
    use client::parse::ConfigStrings;
    use q2proto::{EntityState, Frame};

    /// A stub navigator that reports fixed edge kinds + a fixed pursue target — enough to drive
    /// the executor's swim/ride branches without a real nav graph.
    #[derive(Default)]
    struct StubNav {
        swim: bool,
        ride: Option<RideInfo>,
        pursue: Option<Vec3>,
        waypoint: Option<[f32; 3]>,
    }
    impl Navigator for StubNav {
        fn update(&mut self, _pos: Vec3, _cm: Option<&CollisionModel>) -> bool {
            false
        }
        fn set_goal(&mut self, _goal: crate::nav::NavGoal, _pos: Vec3) {}
        fn pursue_target(&self, _from: Vec3) -> Option<Vec3> {
            self.pursue
        }
        fn pursue_target_safe(&self, _from: Vec3, _cm: &CollisionModel) -> Option<Vec3> {
            self.pursue
        }
        fn current_edge_is_jump(&self) -> bool {
            false
        }
        fn current_edge_is_swim(&self) -> bool {
            self.swim
        }
        fn current_edge_is_ride(&self) -> bool {
            self.ride.is_some()
        }
        fn current_ride_info(&self) -> Option<RideInfo> {
            self.ride
        }
        fn current_waypoint_pos(&self) -> Option<[f32; 3]> {
            self.waypoint
        }
        fn force_replan(&mut self) {}
        fn smooth_with_cm(&mut self, _cm: &CollisionModel, _pos: Vec3) {}
        fn blacklist_waypoint_if_blocked(&mut self, _pos: Vec3, _cm: &CollisionModel) {}
    }

    fn view_at(origin: [f32; 3], grounded: bool, entity_origins: &[[f32; 3]]) -> Worldview {
        let mut frame = Frame::default();
        // `pmove.origin` is raw 12.3 fixed-point (×8); `pm_flags` carries PMF_ON_GROUND.
        frame.playerstate.pmove.origin = [
            (origin[0] * 8.0) as i16,
            (origin[1] * 8.0) as i16,
            (origin[2] * 8.0) as i16,
        ];
        frame.playerstate.pmove.pm_flags = if grounded { PMF_ON_GROUND as u8 } else { 0 };
        for (i, o) in entity_origins.iter().enumerate() {
            frame.entities.push(EntityState {
                number: 50 + i as i32,
                origin: *o,
                ..Default::default()
            });
        }
        Worldview::from_frame(&frame, &ConfigStrings::default(), 0)
    }

    fn ladder_info() -> RideInfo {
        RideInfo {
            board: [0.0, 0.0, 0.0],
            far: [0.0, 0.0, 200.0],
            dismount: [0.0, 40.0, 200.0],
            model_index: 0,
            vertical: false,
            board_ent: [0.0, 0.0, 100.0],
            far_ent: [0.0, 0.0, 200.0],
            ladder: true,
            stand_offset: [0.0; 3],
        }
    }

    fn train_info() -> RideInfo {
        RideInfo {
            board: [100.0, 0.0, 50.0],
            far: [400.0, 0.0, 50.0],
            dismount: [430.0, 0.0, 50.0],
            model_index: 3,
            vertical: false,
            board_ent: [100.0, 0.0, 50.0],
            far_ent: [400.0, 0.0, 50.0],
            ladder: false,
            stand_offset: [0.0; 3],
        }
    }

    // A CollisionModel is required by `gates`/`apply` for the water level. A half-space floor far
    // below reports water_level 0 everywhere (no CONTENTS_WATER), which is what these tests want —
    // swim is forced via `StubNav::swim` (the `current_edge_is_swim` gate) instead.
    fn empty_cm() -> CollisionModel {
        CollisionModel::half_space([0.0, 0.0, 1.0], -100_000.0)
    }

    #[test]
    fn no_edge_active_returns_none() {
        let mut ex = TraversalExecutor::new();
        let nav = StubNav::default();
        let cm = empty_cm();
        let view = view_at([0.0, 0.0, 0.0], true, &[]);
        let gates = ex.gates(&nav, Some(&cm), Vec3::ZERO, 0.1);
        assert!(!gates.any());
        let mut mv = MovementIntent::new();
        let frame = TraversalFrame {
            view: &view,
            cm: Some(&cm),
            pos: Vec3::ZERO,
            view_yaw: 0.0,
            steer_fwd: 0.0,
            steer_side: 0.0,
            dt: 0.1,
        };
        assert!(ex.apply(&mut mv, gates, &nav, &frame).is_none());
    }

    #[test]
    fn swim_edge_drives_vertical_thrust_and_flag() {
        let mut ex = TraversalExecutor::new();
        // A swim edge with a target well above → up-thrust, 'S' flag.
        let nav = StubNav {
            swim: true,
            pursue: Some(Vec3::new(0.0, 0.0, 200.0)),
            ..Default::default()
        };
        let cm = empty_cm();
        let view = view_at([0.0, 0.0, 0.0], false, &[]);
        let gates = ex.gates(&nav, Some(&cm), Vec3::ZERO, 0.1);
        assert!(gates.swimming && !gates.ride_active);
        let mut mv = MovementIntent::new();
        let frame = TraversalFrame {
            view: &view,
            cm: Some(&cm),
            pos: Vec3::ZERO,
            view_yaw: 0.0,
            steer_fwd: 0.0,
            steer_side: 0.0,
            dt: 0.1,
        };
        let out = ex.apply(&mut mv, gates, &nav, &frame).expect("swim active");
        assert_eq!(out.flag, 'S');
        assert!(mv.up > 0.0, "target above → up-thrust, got {}", mv.up);
        assert!(!mv.jump, "never jump in water");
    }

    /// Plan 32 T2: a bot submerged past its air budget abandons the swim path and drives straight
    /// up for a breath (full up-thrust + hard up-pitch), regardless of the path target.
    #[test]
    fn air_critical_overrides_swim_toward_surface() {
        let mut ex = TraversalExecutor::new();
        // Path target DOWNWARD (a deep item) — without the override the swim machine would dive.
        let nav = StubNav {
            swim: true,
            pursue: Some(Vec3::new(0.0, 0.0, -200.0)),
            ..Default::default()
        };
        let cm = world::water_channel_world(); // water 0..120 in the central channel
        let pos = Vec3::new(0.0, 0.0, 60.0); // eyes under (level 3)
        let view = view_at([0.0, 0.0, 60.0], false, &[]);
        // Burn ~10s of the 12s air budget (tts here ≈1.1s + 2s margin → critical ≤ ~3.1s left).
        let mut gates = ex.gates(&nav, Some(&cm), pos, 0.1);
        for _ in 0..100 {
            gates = ex.gates(&nav, Some(&cm), pos, 0.1);
        }
        let mut mv = MovementIntent::new();
        let frame = TraversalFrame {
            view: &view,
            cm: Some(&cm),
            pos,
            view_yaw: 0.0,
            steer_fwd: 0.0,
            steer_side: 0.0,
            dt: 0.1,
        };
        let out = ex.apply(&mut mv, gates, &nav, &frame).expect("swim active");
        assert_eq!(out.flag, 'S');
        assert_eq!(mv.up, 1.0, "air critical → full up-thrust, got {}", mv.up);
        assert!(mv.pitch < -45.0, "pitched hard up, got {}", mv.pitch);
        assert!(!mv.jump);

        // One breath at the surface resets the clock: the override disengages.
        let dry_pos = Vec3::new(100.0, 0.0, 30.0); // the dry ledge — level 0
        ex.gates(&nav, Some(&cm), dry_pos, 0.1);
        let gates = ex.gates(&nav, Some(&cm), pos, 0.1); // back under, fresh air
        let mut mv2 = MovementIntent::new();
        ex.apply(&mut mv2, gates, &nav, &frame)
            .expect("swim active");
        assert!(
            mv2.up < 0.0,
            "fresh air → normal swim resumes (dives toward the target), got {}",
            mv2.up
        );
    }

    #[test]
    fn ladder_edge_ascending_presses_up_and_flags_l() {
        let mut ex = TraversalExecutor::new();
        let nav = StubNav {
            ride: Some(ladder_info()),
            ..Default::default()
        };
        let cm = empty_cm();
        // Bot near the bottom of the shaft, exit far above → ascend.
        let view = view_at([0.0, 0.0, 10.0], true, &[]);
        let gates = ex.gates(&nav, Some(&cm), Vec3::new(0.0, 0.0, 10.0), 0.1);
        assert!(gates.ride_active);
        let mut mv = MovementIntent::new();
        let frame = TraversalFrame {
            view: &view,
            cm: Some(&cm),
            pos: Vec3::new(0.0, 0.0, 10.0),
            view_yaw: 0.0,
            steer_fwd: 0.0,
            steer_side: 0.0,
            dt: 0.1,
        };
        let out = ex
            .apply(&mut mv, gates, &nav, &frame)
            .expect("ladder active");
        assert_eq!(out.flag, 'L');
        assert!(mv.up > 0.0, "ascending → up>0, got {}", mv.up);
        assert!(mv.forward > 0.0, "climb into the ladder");
    }

    #[test]
    fn train_boards_only_when_on_deck() {
        let mut ex = TraversalExecutor::new();
        let info = train_info();
        let nav = StubNav {
            ride: Some(info),
            ..Default::default()
        };
        let cm = empty_cm();
        // At the board ledge, NO platform entity present → wait (no board, stand still).
        let view = view_at([110.0, 0.0, 50.0], true, &[]);
        let gates = ex.gates(&nav, Some(&cm), Vec3::new(110.0, 0.0, 50.0), 0.1);
        let mut mv = MovementIntent::new();
        let frame = TraversalFrame {
            view: &view,
            cm: Some(&cm),
            pos: Vec3::new(110.0, 0.0, 50.0),
            view_yaw: 0.0,
            steer_fwd: 0.0,
            steer_side: 0.0,
            dt: 0.1,
        };
        ex.apply(&mut mv, gates, &nav, &frame);
        assert_eq!(mv.forward, 0.0, "no platform → wait, not board");

        // Platform entity sitting on the board point AND the bot grounded on its deck → board.
        let view2 = view_at([100.0, 0.0, 50.0], true, &[[100.0, 0.0, 50.0]]);
        let gates2 = ex.gates(&nav, Some(&cm), Vec3::new(100.0, 0.0, 50.0), 0.1);
        let mut mv2 = MovementIntent::new();
        let frame2 = TraversalFrame {
            view: &view2,
            cm: Some(&cm),
            pos: Vec3::new(100.0, 0.0, 50.0),
            view_yaw: 0.0,
            steer_fwd: 0.0,
            steer_side: 0.0,
            dt: 0.1,
        };
        ex.apply(&mut mv2, gates2, &nav, &frame2);
        // Once boarded, the ride locks active even if the nav edge later clears.
        let nav_off = StubNav::default();
        let gates3 = ex.gates(&nav_off, Some(&cm), Vec3::new(100.0, 0.0, 50.0), 0.1);
        assert!(gates3.ride_active, "boarded → ride stays locked active");
    }

    /// Plan 31: at a vertical lift, an occupied shaft holds the bot at a standoff OUTSIDE the
    /// trigger; a clear shaft with the pad visibly down enters. The bot sits at x=140 between
    /// the pad (x=100) and its standoff (x≈164), so the steering sign distinguishes the two.
    #[test]
    fn lift_waits_clear_when_occupied_then_enters() {
        let mut ex = TraversalExecutor::new();
        let lift = RideInfo {
            board: [100.0, 0.0, 24.0],
            far: [100.0, 0.0, 224.0],
            dismount: [100.0, 0.0, 224.0],
            model_index: 5,
            vertical: true,
            board_ent: [100.0, 0.0, 24.0],
            far_ent: [100.0, 0.0, 224.0],
            ladder: false,
            stand_offset: [0.0; 3],
        };
        let nav = StubNav {
            ride: Some(lift),
            ..Default::default()
        };
        let cm = empty_cm();
        let pos = Vec3::new(140.0, 0.0, 24.0);

        // A rider mid-shaft (player entity at the pad column, z=120) → WaitClear at standoff.
        let mut occupied_frame = Frame::default();
        occupied_frame.playerstate.pmove.origin = [140 * 8, 0, 24 * 8];
        occupied_frame.entities.push(EntityState {
            number: 7,
            modelindex: 255, // player
            origin: [100.0, 0.0, 120.0],
            ..Default::default()
        });
        let view = Worldview::from_frame(&occupied_frame, &ConfigStrings::default(), 0);
        let gates = ex.gates(&nav, Some(&cm), pos, 0.1);
        let mut mv = MovementIntent::new();
        let frame = TraversalFrame {
            view: &view,
            cm: Some(&cm),
            pos,
            view_yaw: 0.0,
            steer_fwd: 0.0,
            steer_side: 0.0,
            dt: 0.1,
        };
        ex.apply(&mut mv, gates, &nav, &frame).expect("ride active");
        assert!(
            mv.forward > 0.1,
            "occupied shaft → retreat to the standoff (+x), got forward {}",
            mv.forward
        );

        // Shaft clear + pad visibly at the bottom (mover at wire origin z=-travel) → Enter.
        // Face the pad (yaw 180 → -x): `face_then_go` steering only walks toward a target the
        // view already faces, so forward>0 here proves the target flipped to the PAD direction
        // (a standoff target at +x would decompose to ~0 at this facing).
        let clear = view_at([140.0, 0.0, 24.0], true, &[[0.0, 4.0, -200.0]]);
        let gates = ex.gates(&nav, Some(&cm), pos, 0.1);
        let mut mv2 = MovementIntent::new();
        let frame2 = TraversalFrame {
            view: &clear,
            cm: Some(&cm),
            pos,
            view_yaw: 180.0,
            steer_fwd: 0.0,
            steer_side: 0.0,
            dt: 0.1,
        };
        ex.apply(&mut mv2, gates, &nav, &frame2)
            .expect("ride active");
        assert!(
            mv2.forward > 0.1,
            "clear shaft + pad down → enter toward the pad, got forward {}",
            mv2.forward
        );
    }

    #[test]
    fn ride_lock_resets_when_never_boarded() {
        let mut ex = TraversalExecutor::new();
        let nav = StubNav::default(); // no ride edge
        let cm = empty_cm();
        let gates = ex.gates(&nav, Some(&cm), Vec3::ZERO, 0.1);
        assert!(!gates.ride_active);
    }
}
