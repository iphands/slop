//! # brain::brains::runtester — the movement-scenario brain (Plan 26)
//!
//! `RunTesterBrain` is a pure, combat-free waypoint-seeker: the exact per-tick decision the
//! `spawn-to-spawn`/`spawn-to-weapon` harness used to run inline (`qbots::scenario`). It is a
//! genuinely *different* brain from [`MainBrain`](super::main::MainBrain) — it steers via the
//! corner-cut-safe `pursue_target_safe` look-ahead and a richer 7-ray backoff/escape that
//! MainBrain lacks — so the plugin seam (Plans 23–25) gives it a first-class home and retires the
//! Plan 15 duplication.
//!
//! The harness still owns connection, lazy goal resolution (the farthest reachable spawn, handed
//! in per-tick via [`BrainContext::goal_override`]), `dt` from the serverframe delta, the
//! `MovementRecorder`, and reach/exit handling. The brain owns only the decision + the
//! steering/recovery state it needs as fields.

use glam::Vec3;

use crate::brains::core::{Brain, BrainContext, BrainMap, BrainOutput};
use crate::move_ctrl::MovementIntent;
use crate::recover::{find_best_direction, Recovery, RecoveryAction};
use crate::steer::{move_from_world_dir, Steering};
use crate::water::{
    is_swimming, water_level, EXIT_HYSTERESIS_TICKS, EXIT_LOOKUP_PITCH, SWIM_VERT_SCALE,
};

/// The movement-scenario brain — drives the injected navigator to `ctx.goal_override`, never
/// fights. Owns the same steering/recovery state the scenario loop kept as locals.
pub struct RunTesterBrain {
    steering: Steering,
    recovery: Recovery,
    /// Ticks remaining in forced-backoff mode (set when `BackOffThenRepath` fires so the bot
    /// actually escapes the wall instead of immediately resuming forward nav).
    backoff_ticks: u32,
    /// World-yaw of the most-open direction found when a hard-stuck recovery fires; the bot
    /// steers toward it during the backoff to physically CLEAR a corner.
    escape_yaw: Option<f32>,
    /// Ticks remaining in water-exit climb-out mode (Plan 40 T3): look-up + forward + up to
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
    active_ride: Option<world::RideInfo>,
}

impl RunTesterBrain {
    /// Build a scenario brain with the same mid-skill steering the inline loop used.
    pub fn new() -> Self {
        Self {
            steering: Steering::new(3.0), // mid-skill for scenario runs
            recovery: Recovery::new(),
            backoff_ticks: 0,
            escape_yaw: None,
            exit_ticks: 0,
            ride_boarded: false,
            active_ride: None,
        }
    }
}

impl Default for RunTesterBrain {
    fn default() -> Self {
        Self::new()
    }
}

impl Brain for RunTesterBrain {
    /// No-op: the runtester drives the injected navigator, not its own roam set.
    fn set_map(&mut self, _map: BrainMap) {}

    fn tick(&mut self, ctx: BrainContext) -> BrainOutput {
        let BrainContext {
            view,
            nav,
            cm,
            dt,
            ticks: _,
            goal_override,
        } = ctx;

        let mut mv = MovementIntent::new();
        // The scenario harness always supplies both nav and cm; if either is missing there's
        // nothing to drive, so emit a no-op intent.
        let (Some(nav), Some(cm)) = (nav, cm) else {
            return BrainOutput {
                intent: mv,
                weapon_request: None,
                intent_forward: 0.0,
            };
        };
        let pos = view.self_state().origin;

        // Drive nav to the goal — no combat. (Body lifted verbatim from the inline scenario
        // tick, Plan 26 T2; the only change is reading the goal from `goal_override`.)
        nav.update(pos, Some(cm));
        if let Some(goal) = goal_override {
            nav.set_goal(goal, pos);
        }
        nav.smooth_with_cm(cm, pos);

        let mut intent_forward = 0.0;

        // Steer via the corner-cut-safe look-ahead (hull + floor validated) so the bot never
        // cuts a corner into a wall or across a gap. Falls back to the next graph node when the
        // straight line is unsafe.
        let pursue_pos = nav.pursue_target_safe(pos, cm);
        let (ideal_yaw, world_move_dir) = if let Some(pt) = pursue_pos {
            let delta = pt - pos;
            if delta.length_squared() > 1.0 {
                let yaw = delta.y.atan2(delta.x).to_degrees();
                let dir = Vec3::new(delta.x, delta.y, 0.0).normalize_or_zero();
                (yaw, dir)
            } else {
                (self.steering.view_yaw(), Vec3::ZERO)
            }
        } else {
            (self.steering.view_yaw(), Vec3::ZERO)
        };
        let view_yaw = self.steering.change_yaw(ideal_yaw, dt);
        let arrive = pursue_pos
            .map(|pt| Steering::arrive_scale((pt - pos).length()))
            .unwrap_or(1.0);
        let (fwd, side) = move_from_world_dir(world_move_dir, view_yaw, true);

        // Water state (Plan 40): recompute waterlevel ourselves (it's not on the wire) so the
        // bot can drive vertical swim thrust, climb out, and so recovery stays out of the water.
        let water = water_level(cm, pos);
        let swimming = is_swimming(water) || nav.current_edge_is_swim();
        // Ride a moving platform (Plan 43): suspend recovery (stand-and-wait / being carried
        // is not a wedge) and override steering below.
        // Ride is active when the nav says so OR we're mid-ride (boarded) — the latter keeps the
        // ride logic + recovery-suspension locked while the platform carries us, even if the
        // navigator advances off the ride edge mid-transit.
        let ride_active = nav.current_edge_is_ride() || self.ride_boarded;

        // Stuck recovery — SUSPENDED while swimming (Plan 40 T4): water move is 0.5× speed and a
        // bob at the surface is not a wedge, so the StuckDetector would false-fire; and
        // find_best_direction actively steers AWAY from water. A real swim dead-end relies on
        // re-path, not blind reverse. So skip recovery entirely while in/at water.
        if !swimming && !ride_active {
            let has_nav_target = nav.pursue_target(pos).is_some();
            let rec_action = self.recovery.evaluate(
                pos,
                dt,
                Some(cm),
                view_yaw,
                has_nav_target,
                false, // never engaging in scenario mode
            );
            match rec_action {
                RecoveryAction::None => {}
                RecoveryAction::Jump => {
                    mv.jump();
                }
                RecoveryAction::Strafe { dir } => {
                    mv.move_side(dir);
                }
                RecoveryAction::BackOffThenRepath => {
                    // Hard stuck (≈3.5 s no progress). Steer toward the most-OPEN direction for 8
                    // ticks (≈0.8 s) to clear the wall/corner the bot is jammed against — straight-
                    // back alone just re-presses the adjacent wall in a corner. Then nav repaths
                    // from the now-open spot. find_best_direction fans 7 rays and returns the
                    // openest; None (fully boxed in) → straight back.
                    self.backoff_ticks = 8;
                    self.escape_yaw = find_best_direction(cm, pos, view_yaw).map(|(y, _)| y);
                    nav.blacklist_waypoint_if_blocked(pos, cm);
                    nav.force_replan();
                }
                RecoveryAction::UseHeading(yaw) => {
                    let r = yaw.to_radians();
                    let free_dir = Vec3::new(r.cos(), r.sin(), 0.0);
                    let (hfwd, hside) = move_from_world_dir(free_dir, view_yaw, true);
                    mv.move_forward(hfwd);
                    mv.move_side(hside);
                }
            }
        }

        if swimming {
            // Swim toward the 3-D target (Plan 40 T2/T3). Use the RAW look-ahead (no floor
            // validation — there's no floor underwater for `pursue_target_safe` to confirm).
            let target = nav
                .pursue_target(pos)
                .or_else(|| nav.current_waypoint_pos().map(Vec3::from))
                .unwrap_or(pos);
            let to = target - pos;
            let hd = to.truncate().length();
            let dz = to.z;
            // Is the target a dry node above us (a water→ledge exit, the railgun climb-out)?
            let target_dry = water_level(cm, target) == 0;
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
            let swim_fwd = if fwd != 0.0 || side != 0.0 {
                fwd.max(0.0)
            } else {
                1.0
            };
            mv.look_at(view_yaw, pitch);
            mv.move_forward(swim_fwd);
            mv.move_side(side);
            intent_forward = swim_fwd;
        } else if self.backoff_ticks > 0 {
            // Sustained escape: move toward the open direction (regardless of facing, so the bot
            // slides out of a corner) rather than straight back into the adjacent wall. Falls
            // back to straight back when no open direction was found.
            self.backoff_ticks -= 1;
            if let Some(ey) = self.escape_yaw {
                let edir = Vec3::new(ey.to_radians().cos(), ey.to_radians().sin(), 0.0);
                let (efwd, eside) = move_from_world_dir(edir, view_yaw, false);
                mv.move_forward(efwd);
                mv.move_side(eside);
            } else {
                mv.move_forward(-1.0);
            }
            if self.backoff_ticks == 0 {
                self.escape_yaw = None;
            }
        } else if fwd > 0.0 || side.abs() > 0.0 {
            // Slow on narrow geometry (thin ledges) so momentum doesn't carry the bot off the
            // edge (navmesh backend; astar returns 1.0).
            let sp = arrive * nav.speed_scale(pos);
            mv.look_at(view_yaw, 0.0);
            mv.move_forward(fwd * sp);
            mv.move_side(side * sp);
            intent_forward = fwd * sp;
        }
        // Ride override (Plan 43): on a ride edge, replace the steer-to-waypoint movement with a
        // stateful board → ride → dismount. Aiming at the far node the whole time would walk the
        // bot off the ledge (before the train is here) or off the moving train (after boarding).
        if ride_active {
            // Prefer the nav's current ride edge; fall back to the one we stored when boarding so
            // a mid-transit nav advance can't strand us on the moving platform.
            if let Some(info) = nav.current_ride_info().or(self.active_ride) {
                let board = Vec3::from(info.board);
                let dismount = Vec3::from(info.dismount);
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
                if info.ladder {
                    // Ladder climb (Plan 35): hug the ladder and press `up` (Q2 `PM_AddCurrents`
                    // ladder rule: `upmove>0` while touching CONTENTS_LADDER → climb). Face the
                    // ladder center so the 1u forward trace hits it (sets `pml.ladder`), press
                    // forward into it, and hold `up` until at the top, then step off.
                    let center = Vec3::from(info.board_ent); // ladder center (facing target)
                                                             // The ladder edge is bidirectional: `dismount` is the EXIT level for THIS
                                                             // direction (above the bot when ascending, below when descending). Drive
                                                             // toward it — `up>0` climbs, `up<0` descends (Q2 `PM_AddCurrents`).
                    let dz = dismount.z - pos.z;
                    tracing::trace!(pz = pos.z, dz, bz = board.z, "ladder climb");
                    if dz.abs() < 20.0 {
                        intent_forward = go(&mut mv, dismount); // level with the exit → step off
                    } else {
                        // Face the ladder so the 1u forward trace hits CONTENTS_LADDER (sets
                        // `pml.ladder`) and press into it.
                        let to_c = center - pos;
                        let lyaw = to_c.y.atan2(to_c.x).to_degrees();
                        mv.look_at(lyaw, 0.0);
                        mv.move_forward(1.0);
                        mv.move_side(0.0);
                        intent_forward = 1.0;
                        if dz > 0.0 {
                            // ASCENDING — hold up; `upmove>0` climbs (Q2 `PM_AddCurrents`).
                            mv.up = 1.0;
                            mv.jump = false;
                        } else if pos.z > board.z - 24.0 {
                            // DESCENDING but still standing on the top floor next to the shaft:
                            // the floor holds us up, so `up<0` does nothing. JUMP forward off the
                            // edge INTO the shaft (CONTENTS_LADDER is open) to start the descent.
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
                    // Vertical lift: walk onto the pad / stand (target up → ~0 horizontal → ride).
                    // JUMP while approaching the pad (T7 — a human hops on); suppress once on the
                    // pad (a jump while rising would launch the bot off it).
                    let board_horiz = (pos.truncate() - board.truncate()).length();
                    if board_horiz > 32.0 {
                        mv.jump();
                    }
                    intent_forward = go(&mut mv, dismount);
                    self.ride_boarded = false;
                } else {
                    let board_horiz = (pos.truncate() - board.truncate()).length();
                    let train_here =
                        crate::ride::platform_present(view, Vec3::from(info.board_ent));
                    let train_far = crate::ride::platform_present(view, Vec3::from(info.far_ent));
                    tracing::trace!(
                        py = pos.y,
                        pz = pos.z,
                        board_horiz,
                        train_here,
                        train_far,
                        boarded = self.ride_boarded,
                        "train ride state"
                    );
                    if !self.ride_boarded {
                        // Are we ACTUALLY standing on the platform deck right now? (grounded, near
                        // its live top-center horizontally, at its height.) Only THEN commit to the
                        // ride — committing the instant `train_here` fired left the bot frozen on
                        // the board ledge (zero-input carry) while the platform left without it.
                        let stand = crate::ride::train_stand_now(view, &info);
                        let grounded = view.self_state().flags & 4 != 0;
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
                            intent_forward = go(&mut mv, board); // walk to the board ledge, wait
                        } else if let Some(s) = stand.filter(|_| train_here) {
                            // Platform is at the near corner → HOP onto its deck (T7: jump the gap).
                            // Aim at the live top-center; the jump clears the ~33u lava gap and we
                            // commit `on_deck` the instant we land grounded on it.
                            intent_forward = go(&mut mv, s);
                            mv.jump();
                        } else {
                            mv.move_forward(0.0); // platform not here yet — wait at the ledge
                            mv.move_side(0.0);
                            intent_forward = 0.0;
                        }
                    } else if (pos - dismount).truncate().length() < 48.0
                        && (view.self_state().flags & 4 != 0)
                    {
                        // We've made it onto the dismount ledge (grounded near it) → ride DONE.
                        self.ride_boarded = false;
                        self.active_ride = None;
                        intent_forward = go(&mut mv, dismount);
                    } else if pos.z < dismount.z - 96.0 {
                        // Fell off into the pit/lava → abandon the ride; let respawn/nav recover.
                        self.ride_boarded = false;
                        self.active_ride = None;
                        intent_forward = 0.0;
                    } else if train_far {
                        // Train at the far corner (near the quad) → JUMP off onto the dismount ledge.
                        intent_forward = go(&mut mv, dismount);
                        mv.jump();
                    } else {
                        // Boarded, mid-transit → DO NOTHING. A Q2 func_train PUSHES riders, so zero
                        // input keeps us perfectly carried (verified: z holds steady on *10).
                        // ANY movement (chasing the moving center) slid us off the small platform
                        // into the lava — the winning move is to stand still and let it carry us to
                        // the far corner, where `train_far` above fires the dismount.
                        mv.move_forward(0.0);
                        mv.move_side(0.0);
                        mv.jump = false;
                        intent_forward = 0.0;
                    }
                }
            }
        } else {
            self.ride_boarded = false;
            self.active_ride = None;
        }
        if nav.current_edge_is_jump() && !swimming && !ride_active {
            mv.jump();
        }

        BrainOutput {
            intent: mv,
            weapon_request: None,
            // The nav-step forward (0 during recovery/backoff) — the recorder's hindered-flag
            // input, preserved exactly from the inline scenario loop.
            intent_forward,
        }
    }

    fn status(&self) -> &str {
        "runtester"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nav::NavGoal;
    use crate::nav_mode::StubNav;
    use crate::perception::Worldview;
    use client::parse::ConfigStrings;
    use q2proto::Frame;

    /// An open test world: solid is 100 k units down, so every trace near the bot is clear.
    /// (`pursue_target_safe` is stubbed, so the CM only feeds recovery / `find_best_direction`.)
    fn open_cm() -> world::CollisionModel {
        world::CollisionModel::half_space([0.0, 0.0, 1.0], -100_000.0)
    }
    fn view0() -> Worldview {
        Worldview::from_frame(&Frame::default(), &ConfigStrings::default(), 0)
    }
    fn ctx<'a>(
        view: &'a Worldview,
        nav: &'a mut StubNav,
        cm: &'a world::CollisionModel,
        goal: Option<NavGoal>,
    ) -> BrainContext<'a> {
        BrainContext {
            view,
            nav: Some(nav),
            cm: Some(cm),
            dt: 0.1,
            ticks: 1,
            goal_override: goal,
        }
    }

    #[test]
    fn steers_forward_toward_lookahead() {
        let (cm, view) = (open_cm(), view0());
        let mut nav = StubNav {
            pursue: Some(Vec3::new(100.0, 0.0, 0.0)),
            ..Default::default()
        };
        let out = RunTesterBrain::new().tick(ctx(&view, &mut nav, &cm, None));
        assert!(
            out.intent.forward > 0.0,
            "should advance toward the +x look-ahead, got {}",
            out.intent.forward
        );
    }

    #[test]
    fn drives_goal_override_into_nav() {
        let (cm, view) = (open_cm(), view0());
        let mut nav = StubNav::default();
        let goal = NavGoal::Position(Vec3::new(7.0, 8.0, 9.0));
        let _ = RunTesterBrain::new().tick(ctx(&view, &mut nav, &cm, Some(goal.clone())));
        assert_eq!(nav.last_goal, Some(goal));
    }

    #[test]
    fn presses_jump_on_jump_edge() {
        let (cm, view) = (open_cm(), view0());
        let mut nav = StubNav {
            pursue: Some(Vec3::new(100.0, 0.0, 0.0)),
            jump_edge: true,
            ..Default::default()
        };
        let out = RunTesterBrain::new().tick(ctx(&view, &mut nav, &cm, None));
        assert!(out.intent.jump, "a jump-link edge must press jump");
    }

    #[test]
    fn swim_edge_drives_vertical_thrust_and_lookup() {
        // A swim edge toward a target ABOVE (a water→ledge exit): the bot must hold up-thrust
        // and look up past -15° to trigger the Q2 water-jump climb-out, while pressing forward.
        let (cm, view) = (open_cm(), view0());
        let mut nav = StubNav {
            pursue: Some(Vec3::new(40.0, 0.0, 100.0)),
            swim_edge: true,
            ..Default::default()
        };
        let out = RunTesterBrain::new().tick(ctx(&view, &mut nav, &cm, None));
        assert!(
            out.intent.up > 0.0,
            "ascending swim must thrust up, got {}",
            out.intent.up
        );
        assert!(
            out.intent.pitch <= -15.0,
            "water-exit must look up (pitch ≤ -15), got {}",
            out.intent.pitch
        );
        assert!(
            out.intent.forward > 0.0,
            "must press forward into the climb-out"
        );
        assert!(!out.intent.jump, "never one-shot jump while swimming");
    }

    #[test]
    fn swim_edge_descends_toward_lower_target() {
        // A swim edge toward a target BELOW: sustained downward thrust (negative up), no jump.
        let (cm, view) = (open_cm(), view0());
        let mut nav = StubNav {
            pursue: Some(Vec3::new(40.0, 0.0, -100.0)),
            swim_edge: true,
            ..Default::default()
        };
        let out = RunTesterBrain::new().tick(ctx(&view, &mut nav, &cm, None));
        assert!(
            out.intent.up < 0.0,
            "descending swim must thrust down, got {}",
            out.intent.up
        );
        assert!(!out.intent.jump, "never one-shot jump while swimming");
    }

    #[test]
    fn speed_scale_throttles_forward() {
        let (cm, view) = (open_cm(), view0());
        let mut nav_full = StubNav {
            pursue: Some(Vec3::new(100.0, 0.0, 0.0)),
            ..Default::default()
        };
        let mut nav_half = StubNav {
            pursue: Some(Vec3::new(100.0, 0.0, 0.0)),
            speed: Some(0.5),
            ..Default::default()
        };
        let full = RunTesterBrain::new()
            .tick(ctx(&view, &mut nav_full, &cm, None))
            .intent
            .forward;
        let half = RunTesterBrain::new()
            .tick(ctx(&view, &mut nav_half, &cm, None))
            .intent
            .forward;
        assert!(
            half > 0.0 && half < full,
            "speed_scale 0.5 must halve forward (half={half} full={full})"
        );
    }

    #[test]
    fn never_requests_a_weapon() {
        let (cm, view) = (open_cm(), view0());
        let mut nav = StubNav {
            pursue: Some(Vec3::new(100.0, 0.0, 0.0)),
            ..Default::default()
        };
        let out = RunTesterBrain::new().tick(ctx(&view, &mut nav, &cm, None));
        assert!(out.weapon_request.is_none());
        assert_eq!(out.intent_forward, out.intent.forward); // active branch: telemetry == forward
    }

    #[test]
    fn backoff_replans_after_sustained_no_progress() {
        let (cm, view) = (open_cm(), view0());
        // Fixed position every tick + a live nav target → Recovery escalates to BackOffThenRepath.
        let mut nav = StubNav {
            pursue: Some(Vec3::new(100.0, 0.0, 0.0)),
            ..Default::default()
        };
        let mut brain = RunTesterBrain::new();
        for _ in 0..80 {
            let _ = brain.tick(ctx(&view, &mut nav, &cm, None));
        }
        assert!(
            nav.replans > 0,
            "sustained no-progress must trigger a backoff replan"
        );
    }
}
