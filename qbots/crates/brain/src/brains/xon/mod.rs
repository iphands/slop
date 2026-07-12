//! # brain::brains::xon — the Xonotic-derived brain (`xon`; Plan 60)
//!
//! Ports havocbot's *decision texture* (research: `context/distilled/xonotic.md`; vendor:
//! `vendor/xonotic/data/xonotic-data.pk3dir/qcsrc/server/bot/default/`) onto qbots'
//! shared infrastructure. What makes `xon` different from `main`/`q3`/`zb2` (distilled §9):
//!
//! - **One smooth objective** — every candidate goal (item, enemy, wander waypoint) is
//!   rated `value * rangebias/(rangebias + travel_time)` and the best wins; no FSM picking
//!   goal *categories* (T2, [`crate::xoncore::rating`]).
//! - **Re-planning on evidence** — observed pickups, a 0.5 s goal-progress watchdog, and a
//!   3 s ignore-list replace pure re-plan timers (T2).
//! - **Aim as a dynamical system** — the five-stage anticipation cascade + mouse-think +
//!   fire cone ([`crate::xoncore::aim::XonAim`], T4).
//! - **Keyboard-emulated movement** ([`crate::xoncore::keyboard::KeyboardEmu`], T5).
//! - **Weapon combos** — switch mid-refire when another gun lands sooner (T3).
//!
//! Locomotion is a copy of `q3`'s proven `locomote` stage (Plan 58's shared extraction was
//! abandoned — see `context/plans/abandoned/58_*`): steering + hazard creep + traversal
//! gates + stuck recovery + jump edges, all delegating to the SAME shared modules
//! (`Steering`/`Recovery`/`TraversalExecutor`/`hazard`) every other brain uses, so `xon`
//! swims/rides/climbs and respects lava like the rest of the fleet.
//!
//! Personality: [`XonSkill`] — Xonotic's 12 additive skill axes (Plan 59).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use glam::Vec3;
use world::NavGraph;

use crate::brains::core::{Brain, BrainConfig, BrainContext, BrainMap, BrainOutput, MapItem};
use crate::items::{self, ItemMemory};
use crate::move_ctrl::MovementIntent;
use crate::nav::NavGoal;
use crate::perception::EntityClass;
use crate::recover::{Recovery, RecoveryAction};
use crate::skill::BotSkill;
use crate::steer::{move_from_world_dir, Steering};
use crate::traverse::{TraversalExecutor, TraversalFrame};
use crate::xonchar::XonSkill;
use crate::xoncore::Lcg;

mod goals;
use goals::{RatingCtx, XonGoals};

/// Process-wide xon ordinal — staggers each bot's first rating session so a fleet doesn't
/// flood the graph on the same frame (the poor-man's strategy token, `bot.qc:784-811`).
static BOT_ORDINAL: AtomicUsize = AtomicUsize::new(0);

/// The Xonotic-derived decision brain. Owns the goal-stack strategy (T2), combat (T3–T5),
/// and the locomotion state; the `Navigator` is injected each tick.
pub struct XonBrain {
    /// The 12-axis personality (Plan 59) — every skill-scaled formula reads from here.
    sk: XonSkill,
    /// Deterministic per-bot RNG (the vendor's `random()`).
    rng: Lcg,
    /// Combat gate (scenarios force `combat_enabled = false` — enemies are then never
    /// rated as goals, and T3-T5 combat stays off).
    cfg: BrainConfig,

    // ── navigation / roam (mirrors Q3Brain until the T2 strategy layer lands) ─────────
    roam_nodes: Vec<usize>,
    roam_idx: usize,
    nav_graph: Option<Arc<NavGraph>>,
    roam_as_position: bool,
    /// Static BSP item table (Plan 30) — the rating-session candidates.
    map_items: Vec<MapItem>,
    /// PVS-honest taken/respawn memory over `map_items` (shared `items::ItemMemory`).
    item_memory: ItemMemory,
    /// The goal-stack strategy layer (T2).
    goals: XonGoals,
    /// Wall-clock seconds since connect (accumulated from `dt`).
    time: f32,
    /// Status label reflecting the committed goal kind (`xon-item`/`xon-enemy`/`xon-wander`).
    status: &'static str,

    // ── shared locomotion primitives (same modules as main/q3/zb2) ────────────────────
    steering: Steering,
    recovery: Recovery,
    traverse: TraversalExecutor,
    /// Skill for the shared PVS item picker used by the interim roam ladder.
    item_skill: BotSkill,
}

impl XonBrain {
    /// Build an `xon` brain with the given personality. Roam goals + the nav graph arrive
    /// later via [`set_map`](Brain::set_map).
    pub fn new(sk: XonSkill, cfg: BrainConfig) -> Self {
        // Path-following turn rate scales with movement skill (XonAim owns combat turning
        // from T4); qport-independent, deterministic.
        let steering = Steering::new(1.0 + (sk.movement() / 10.0).clamp(0.0, 1.0) * 4.0);
        let ordinal = BOT_ORDINAL.fetch_add(1, Ordering::Relaxed);
        Self {
            sk,
            rng: Lcg::new(0x584f_4e21 ^ ordinal as u32), // "XON!" + per-bot ordinal
            cfg,
            roam_nodes: Vec::new(),
            roam_idx: 0,
            nav_graph: None,
            roam_as_position: false,
            map_items: Vec::new(),
            item_memory: ItemMemory::new(),
            goals: XonGoals::new(ordinal as f32 * 0.35),
            time: 0.0,
            status: "xon",
            steering,
            recovery: Recovery::new(),
            traverse: TraversalExecutor::new(),
            item_skill: BotSkill::default(),
        }
    }

    /// Interim roam ladder (T1; replaced by the T2 rating session): best visible item, else
    /// the roam cursor, else hold — the same shape every brain boots with.
    fn roam_goal(&mut self, view: &crate::perception::Worldview, ticks: u32, pos: Vec3) -> NavGoal {
        if let Some((item_pos, _)) = items::best_item_goal(view, &self.item_skill) {
            return NavGoal::Position(item_pos);
        }
        if !self.roam_nodes.is_empty() {
            if ticks.is_multiple_of(50) {
                self.roam_idx =
                    (self.roam_idx + self.roam_nodes.len() / 7 + 1) % self.roam_nodes.len();
            }
            let node = self.roam_nodes[self.roam_idx];
            return if self.roam_as_position {
                match &self.nav_graph {
                    Some(g) => NavGoal::Position(Vec3::from(g.node_pos(node))),
                    None => NavGoal::Waypoint(node),
                }
            } else {
                NavGoal::Waypoint(node)
            };
        }
        NavGoal::Position(pos)
    }

    /// Run the T2 strategy layer: build this frame's rating context and delegate to
    /// [`XonGoals::tick`]. Enemies are candidates only when combat is enabled (the
    /// scenario contract). Returns `(goal, replan_requested)`.
    fn strategy_goal(
        &mut self,
        view: &crate::perception::Worldview,
        pos: Vec3,
        dt: f32,
    ) -> Option<(NavGoal, bool)> {
        let graph = self.nav_graph.clone()?;
        let enemies: Vec<(i32, Vec3)> = if self.cfg.combat_enabled {
            view.entities()
                .filter(|e| e.class == EntityClass::EnemyPlayer && !e.is_stale)
                .map(|e| (e.entity_number, e.origin))
                .collect()
        } else {
            Vec::new()
        };
        let ss = view.self_state();
        let ctx = RatingCtx {
            graph: &graph,
            items: &self.map_items,
            memory: &self.item_memory,
            enemies: &enemies,
            roam_nodes: &self.roam_nodes,
            pos,
            health: ss.health as f32,
            armor: ss.armor as f32,
            held: ss.held_weapon.unwrap_or(crate::weapons::Weapon::Blaster),
            now: self.time,
        };
        let d = self.goals.tick(&mut self.rng, &self.sk, &ctx, dt)?;
        self.status = match d.key {
            goals::GoalKey::Item(_) => "xon-item",
            goals::GoalKey::Enemy(_) => "xon-enemy",
            goals::GoalKey::Wander(_) => "xon-wander",
        };
        Some((NavGoal::Position(d.goal_pos), d.replan))
    }

    /// Drive the injected navigator to `goal` — the canonical path-follow stage (q3's
    /// `locomote` shape): safe pursue → rate-limited yaw → arrive/creep throttle →
    /// traversal gates → stuck recovery → jump edges → traversal override LAST.
    #[allow(clippy::too_many_arguments)]
    fn locomote(
        &mut self,
        nav: &mut dyn crate::nav_mode::Navigator,
        cm: Option<&world::CollisionModel>,
        pos: Vec3,
        goal: NavGoal,
        dt: f32,
        view: &crate::perception::Worldview,
        mv: &mut MovementIntent,
    ) {
        nav.update(pos, None);
        nav.set_goal(goal, pos);
        if let Some(cm) = cm {
            nav.smooth_with_cm(cm, pos);
        }

        // Corner-cut-safe path look-ahead (Plan 48 L3): hull + lava-aware floor validation.
        let pursue_pt = match cm {
            Some(c) => nav.pursue_target_safe(pos, c),
            None => nav.pursue_target(pos),
        };

        // View yaw: steer along the path look-ahead.
        let ideal_yaw = pursue_pt
            .filter(|pt| (pt - pos).length_squared() > 1.0)
            .map(|pt| {
                let d = pt - pos;
                d.y.atan2(d.x).to_degrees()
            })
            .unwrap_or(self.steering.view_yaw());
        let view_yaw = self.steering.change_yaw(ideal_yaw, dt);
        mv.look_at(view_yaw, 0.0);

        // World move direction from the look-ahead; arrive + hazard creep (Plan 50).
        let world_dir = pursue_pt
            .map(|pt| {
                let d = pt - pos;
                Vec3::new(d.x, d.y, 0.0).normalize_or_zero()
            })
            .unwrap_or(Vec3::ZERO);
        let arrive = pursue_pt
            .map(|pt| Steering::arrive_scale((pt - pos).length()))
            .unwrap_or(1.0);
        let creep = crate::hazard::creep_scale(cm, pos, world_dir);
        let (fwd, side) = move_from_world_dir(world_dir, view_yaw, true);
        mv.move_forward(fwd * arrive * creep);
        mv.move_side(side * arrive * creep);

        // Traversal gates (Plan 46): swim/ride/ladder suspend stuck recovery + jump-edge.
        let gates = self.traverse.gates(nav, cm, pos, dt);

        // Stuck recovery (Plan 13/48).
        let has_nav_target = pursue_pt.is_some();
        let rec = if gates.any() {
            RecoveryAction::None
        } else {
            self.recovery
                .evaluate(pos, dt, cm, view_yaw, has_nav_target, false)
        };
        match rec {
            RecoveryAction::None => {}
            RecoveryAction::Jump => mv.jump(),
            RecoveryAction::Strafe { dir } => {
                mv.move_side(crate::hazard::safe_strafe_dir(cm, pos, view_yaw, dir));
            }
            RecoveryAction::BackOffThenRepath => {
                mv.move_forward(-0.5);
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

        if nav.current_edge_is_jump() && !gates.any() {
            mv.jump();
        }

        // Traversal override LAST (Plan 46): the shared executor owns swim/ride/ladder axes.
        let frame = TraversalFrame {
            view,
            cm,
            pos,
            view_yaw,
            steer_fwd: fwd,
            steer_side: side,
            dt,
        };
        self.traverse.apply(mv, gates, nav, &frame);
    }
}

impl Brain for XonBrain {
    fn set_map(&mut self, map: BrainMap) {
        let BrainMap {
            roam_nodes,
            nav_graph,
            roam_as_position,
            items,
        } = map;
        self.roam_nodes = roam_nodes;
        self.nav_graph = Some(nav_graph);
        self.roam_as_position = roam_as_position;
        // The static item table feeds the rating sessions (values × ItemMemory availability).
        self.map_items = items;
    }

    fn status(&self) -> &str {
        self.status
    }

    fn tick(&mut self, ctx: BrainContext) -> BrainOutput {
        let BrainContext {
            view,
            nav,
            cm,
            dt,
            ticks,
            goal_override,
        } = ctx;
        self.time += dt;

        let pos = view.self_state().origin;
        let health = view.self_state().health;

        // PVS-honest item memory (evidence for goal expiry + candidate availability).
        self.item_memory.observe(&self.map_items, view, self.time);

        let mut mv = MovementIntent::new();
        if let Some(nav) = nav {
            // Scenario / pinned-goal override always path-follows (the spawn-to-* contract);
            // otherwise the T2 rating-session strategy picks the goal.
            let goal = match goal_override.clone() {
                Some(g) => g,
                None => match self.strategy_goal(view, pos, dt) {
                    Some((g, replan)) => {
                        if replan {
                            nav.force_replan();
                        }
                        g
                    }
                    // Nothing ratable (no graph candidates yet) — interim roam ladder.
                    None => self.roam_goal(view, ticks, pos),
                },
            };
            self.locomote(nav, cm, pos, goal, dt, view, &mut mv);
        } else {
            // No nav graph yet — walk forward so the bot isn't a statue.
            mv.move_forward(1.0);
        }

        // Survival override (Plan 50 E2): standing in lava/slime outranks everything.
        // Health-gated: a sinking corpse still ticks.
        if let Some(c) = cm {
            if let Some(esc) = (health > 0)
                .then(|| crate::hazard::escape_from_lava(c, pos))
                .flatten()
            {
                let yaw = esc.y.atan2(esc.x).to_degrees();
                self.steering.set_view_yaw(yaw);
                mv.look_at(yaw, 0.0);
                mv.forward = 1.0;
                mv.side = 0.0;
                mv.jump();
                mv.up = 1.0;
                let vel = view.self_state().velocity;
                tracing::info!(
                    x = pos.x as i32,
                    y = pos.y as i32,
                    z = pos.z as i32,
                    vz = vel.z as i32,
                    hs = vel.truncate().length() as i32,
                    "EVT lava_escape"
                );
            }
        }

        BrainOutput {
            intent: mv,
            weapon_request: None,
            intent_forward: mv.forward,
        }
    }

    fn on_death(&mut self) {
        // Respawned elsewhere — steering/recovery state is stale; traversal resets via gates.
        self.recovery.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nav_mode::StubNav;
    use crate::perception::Worldview;
    use client::parse::ConfigStrings;
    use q2proto::Frame;

    fn view0() -> Worldview {
        Worldview::from_frame(&Frame::default(), &ConfigStrings::default(), 0)
    }

    #[test]
    fn walks_toward_lookahead() {
        let mut b = XonBrain::new(XonSkill::default(), BrainConfig::default());
        let view = view0();
        let mut nav = StubNav {
            pursue: Some(Vec3::new(200.0, 0.0, 0.0)),
            ..Default::default()
        };
        let out = b.tick(BrainContext {
            view: &view,
            nav: Some(&mut nav),
            cm: None,
            dt: 0.1,
            ticks: 1,
            goal_override: None,
        });
        assert!(nav.last_goal.is_some(), "a roam goal was set");
        assert!(out.intent.forward > 0.0, "walks toward the look-ahead");
        assert!(out.weapon_request.is_none(), "no combat before T3");
    }

    #[test]
    fn goal_override_drives_the_navigator() {
        let mut b = XonBrain::new(XonSkill::default(), BrainConfig::default());
        let view = view0();
        let mut nav = StubNav::default();
        let goal = NavGoal::Position(Vec3::new(7.0, 8.0, 9.0));
        let _ = b.tick(BrainContext {
            view: &view,
            nav: Some(&mut nav),
            cm: None,
            dt: 0.1,
            ticks: 1,
            goal_override: Some(goal.clone()),
        });
        assert_eq!(
            nav.last_goal,
            Some(goal),
            "scenario contract: honor the override"
        );
    }

    #[test]
    fn no_nav_walks_forward() {
        let mut b = XonBrain::new(XonSkill::default(), BrainConfig::default());
        let view = view0();
        let out = b.tick(BrainContext {
            view: &view,
            nav: None,
            cm: None,
            dt: 0.1,
            ticks: 1,
            goal_override: None,
        });
        assert!(out.intent.forward > 0.0);
        assert_eq!(b.status(), "xon");
    }
}
