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
use crate::traverse::{TraversalExecutor, TraversalFrame};

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
    /// The shared ladder/swim/ride executor (Plan 46) — owns the water-exit hysteresis + the
    /// stateful board/carry lock that used to live as inline fields here.
    traverse: TraversalExecutor,
}

impl RunTesterBrain {
    /// Build a scenario brain with the same mid-skill steering the inline loop used.
    pub fn new() -> Self {
        Self {
            steering: Steering::new(3.0), // mid-skill for scenario runs
            recovery: Recovery::new(),
            backoff_ticks: 0,
            escape_yaw: None,
            traverse: TraversalExecutor::new(),
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

        // Traversal gates (Plan 46): the shared TraversalExecutor tells us whether we're swimming
        // (Plan 40) or riding a moving platform (Plan 43). Both SUSPEND stuck recovery — water move
        // is 0.5× speed and a surface bob is not a wedge (the StuckDetector would false-fire and
        // find_best_direction steers AWAY from water); a stand-and-wait on a lift / being carried is
        // not a wedge either. `gates` also keeps the ride LOCKED active while boarded even if the
        // navigator advances off the ride edge mid-transit. The actual swim/ride movement override
        // is applied further down (after normal steering) via `self.traverse.apply`.
        let gates = self.traverse.gates(nav, Some(cm), pos);

        if !gates.swimming && !gates.ride_active {
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

        if gates.swimming {
            // Swim movement (Plan 40) is now owned by `self.traverse.apply` below — skip the normal
            // backoff/forward steering while in water (this branch just preserves the else-if chain
            // so a swim frame doesn't also run the dry-land move).
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
        // Traversal override (Plan 46): swim (Plan 40) + stateful ride/ladder (Plan 43/35) movement
        // is owned by the shared `TraversalExecutor`. On a swim/ride edge it OVERWRITES the movement
        // axes computed above (aiming at the far node the whole time would walk the bot off the
        // ledge before the train is here, or off the moving train after boarding) and returns the
        // forward-progress intent for the recorder's hindered flag.
        let frame = TraversalFrame {
            view,
            cm: Some(cm),
            pos,
            view_yaw,
            steer_fwd: fwd,
            steer_side: side,
        };
        if let Some(applied) = self.traverse.apply(&mut mv, gates, nav, &frame) {
            intent_forward = applied.intent_forward;
        }
        if nav.current_edge_is_jump() && !gates.swimming && !gates.ride_active {
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
