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
        let ride_active = nav.current_edge_is_ride();

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
        // Ride override (Plan 43): on a ride edge, replace the steer-to-waypoint movement with
        // walk-to-board → WAIT (no forward) until the platform arrives → cross to the dismount.
        // Aiming at the far node directly (the normal pursue target) would walk the bot off the
        // ledge before the platform is there.
        if ride_active {
            if let Some(info) = nav.current_ride_info() {
                let phase = crate::ride::ride_phase(pos, &info, view);
                match phase {
                    crate::ride::RidePhase::Wait => {
                        mv.move_forward(0.0);
                        mv.move_side(0.0);
                        intent_forward = 0.0;
                    }
                    crate::ride::RidePhase::Approach | crate::ride::RidePhase::Cross => {
                        let target = crate::ride::ride_target(phase, &info);
                        let to = target - pos;
                        let dir = if to.length_squared() > 1e-6 {
                            to.normalize_or_zero()
                        } else {
                            Vec3::ZERO
                        };
                        let (rfwd, rside) = move_from_world_dir(dir, view_yaw, true);
                        mv.look_at(view_yaw, 0.0);
                        mv.move_forward(rfwd);
                        mv.move_side(rside);
                        intent_forward = rfwd;
                    }
                }
            }
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
