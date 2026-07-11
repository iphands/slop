//! # brain::brains::zb2 — the 3ZB2-derived brain (`zb2`; Plan 44)
//!
//! Ports 3rd-Zigock-Bot-II's signature *decision texture* (treating its C as pseudocode —
//! `vendor/3zb2-zigflag/src/bot/`, distilled in `context/distilled/brains/3zb2_brain.md`):
//!
//! - **Committed sequential routes** (`routeindex++`, `bot_za.c`): plan once, then RUN the
//!   memorized polyline — replanning only on goal change, hard-stuck escalation, or death.
//!   This is the opposite of `main`'s reactive per-tick goal churn and gives 3ZB2's
//!   characteristic "purposeful runner" feel.
//! - **`Search_NearlyPod` shortcut-skip** (`za.c:2214`): while following, if a node *further
//!   along the committed path* is visible, skip straight to it (never to arbitrary graph
//!   nodes, and never across a jump/swim/ride edge — those need their movement machines).
//! - **Mover route-states** (`GRS_ONPLAT`/`GRS_ONTRAIN`): on a ride/swim/ladder edge the
//!   route index freezes and movement is delegated to the shared [`TraversalExecutor`]
//!   (Plan 46) until the leg completes — 3ZB2's "don't advance the route while carried".
//! - **Fight on the run**: 3ZB2 keeps running its route while shooting. Combat here reuses
//!   the shared [`CombatDriver`] for target/aim/fire, but movement STAYS on the route (the
//!   view locks onto the enemy; the legs keep the itinerary).
//!
//! **Deliberate deviation from the plan's Navigator reuse**: `Zb2Brain` plans on the A*
//! [`NavGraph`] directly and follows its own committed polyline (the authentic pod-chain
//! shape), exposing it to the executor via the internal [`Zb2Route`] facade. It therefore
//! ignores the injected `--navmode` backend (always A*-graph-routed), exactly as 3ZB2
//! followed its own chain files. We do NOT port `G_FindRouteLink` — our graph's
//! Walk/Jump/Swim/Ride edges are strictly richer than 3ZB2's `linkpod[6]`.

use std::sync::Arc;

use glam::Vec3;
use world::{CollisionModel, EdgeKind, NavGraph};

use crate::brains::core::{Brain, BrainContext, BrainMap, BrainOutput};
use crate::combat::{CombatDecision, CombatDriver};
use crate::move_ctrl::MovementIntent;
use crate::nav::NavGoal;
use crate::nav_mode::Navigator;
use crate::perception::EntityClass;
use crate::recover::{Recovery, RecoveryAction};
use crate::skill::BotSkill;
use crate::steer::{move_from_world_dir, Steering};
use crate::traverse::{TraversalExecutor, TraversalFrame};
use crate::weapons::Weapon;
use crate::{items, los};

/// Within this 3-D range of the current route node, advance to the next (graph spacing is 24u).
const ADVANCE_RADIUS: f32 = 32.0;
/// `Search_NearlyPod` look-ahead cap (nodes scanned past the current index).
const SHORTCUT_LOOKAHEAD: usize = 6;
/// Max |dz| between us and a shortcut target — LOS ≠ walkable (the classic bot trap), so only
/// skip to near-level nodes (a generous step/ramp band; jumps/drops keep their edges).
const SHORTCUT_MAX_DZ: f32 = 32.0;

/// The committed route (3ZB2's memorized pod chain): an A* polyline + the follow cursor.
/// Implements [`Navigator`] so the shared traversal executor (and recorder plumbing) can read
/// the current edge exactly as they do for the real nav backends.
struct Zb2Route {
    graph: Arc<NavGraph>,
    path: Vec<usize>,
    /// Index of the node currently being run at (the route cursor).
    idx: usize,
    /// The committed destination node — a differing goal triggers a replan.
    goal_node: usize,
    /// Set by recovery escalation: replan on the next tick.
    dirty: bool,
}

impl Zb2Route {
    /// The (from, to) node pair of the edge currently being traversed, once under way.
    fn current_edge(&self) -> Option<(usize, usize)> {
        (self.idx > 0 && self.idx < self.path.len())
            .then(|| (self.path[self.idx - 1], self.path[self.idx]))
    }

    fn node_vec(&self, i: usize) -> Vec3 {
        Vec3::from(self.graph.node_pos(self.path[i]))
    }

    /// True once the cursor has consumed the whole polyline.
    fn finished(&self) -> bool {
        self.idx >= self.path.len()
    }
}

impl Navigator for Zb2Route {
    /// Advance the cursor over any nodes we've arrived at (3ZB2's `routeindex++`).
    fn update(&mut self, pos: Vec3, _cm: Option<&CollisionModel>) -> bool {
        let mut advanced = false;
        while self.idx < self.path.len()
            && (self.node_vec(self.idx) - pos).length() < ADVANCE_RADIUS
        {
            self.idx += 1;
            advanced = true;
        }
        advanced
    }
    /// The brain owns goal selection; the facade never re-goals itself.
    fn set_goal(&mut self, _goal: NavGoal, _from: Vec3) {}
    fn pursue_target(&self, _from: Vec3) -> Option<Vec3> {
        (self.idx < self.path.len()).then(|| self.node_vec(self.idx))
    }
    /// Committed nodes ARE graph nodes (never look-ahead cuts), so the raw target is hull-honest.
    fn pursue_target_safe(&self, from: Vec3, _cm: &CollisionModel) -> Option<Vec3> {
        self.pursue_target(from)
    }
    fn current_edge_is_jump(&self) -> bool {
        self.current_edge()
            .is_some_and(|(a, b)| matches!(self.graph.edge_kind(a, b), EdgeKind::Jump { .. }))
    }
    fn current_edge_is_swim(&self) -> bool {
        self.current_edge()
            .is_some_and(|(a, b)| matches!(self.graph.edge_kind(a, b), EdgeKind::Swim))
    }
    fn current_edge_is_ride(&self) -> bool {
        self.current_edge()
            .is_some_and(|(a, b)| matches!(self.graph.edge_kind(a, b), EdgeKind::Ride))
    }
    fn current_ride_info(&self) -> Option<world::RideInfo> {
        self.current_edge()
            .and_then(|(a, b)| self.graph.ride_info(a, b))
    }
    fn current_waypoint(&self) -> Option<usize> {
        self.path.get(self.idx).copied()
    }
    fn current_waypoint_pos(&self) -> Option<[f32; 3]> {
        self.path.get(self.idx).map(|&n| self.graph.node_pos(n))
    }
    fn force_replan(&mut self) {
        self.dirty = true;
    }
    fn blacklist_waypoint_if_blocked(&mut self, _pos: Vec3, _cm: &CollisionModel) {}
}

/// `Search_NearlyPod` (pure, unit-tested): the furthest index in `(idx, idx+cap]` we may skip
/// to — every hop crossed must be a plain Walk edge (a skipped jump/swim/ride edge would strand
/// the movement machine), the target must be near-level (`|dz| <= SHORTCUT_MAX_DZ` of `pos`),
/// and `visible(target)` must hold. Returns `idx` unchanged when no skip qualifies.
fn nearly_pod_skip(
    idx: usize,
    pos: Vec3,
    node_pos: &dyn Fn(usize) -> Vec3,
    path_len: usize,
    walk_edge_into: &dyn Fn(usize) -> bool, // is edge (path[j-1] → path[j]) a Walk edge?
    visible: &dyn Fn(Vec3) -> bool,
) -> usize {
    let mut best = idx;
    let cap = (idx + SHORTCUT_LOOKAHEAD).min(path_len.saturating_sub(1));
    let mut j = idx + 1;
    while j <= cap {
        if !walk_edge_into(j) {
            break; // never skip across a jump/swim/ride edge
        }
        let p = node_pos(j);
        if (p.z - pos.z).abs() <= SHORTCUT_MAX_DZ && visible(p) {
            best = j;
        }
        j += 1;
    }
    best
}

/// The 3ZB2-derived decision brain: committed routes + shortcut skips + shared combat/traversal.
pub struct Zb2Brain {
    skill: BotSkill,
    combat: CombatDriver,
    steering: Steering,
    recovery: Recovery,
    traverse: TraversalExecutor,
    /// Roam goal cursor (largest-component node indices) — the "route destinations".
    roam_nodes: Vec<usize>,
    roam_idx: usize,
    nav_graph: Option<Arc<NavGraph>>,
    /// The committed route being run, if any.
    route: Option<Zb2Route>,
    combat_enabled: bool,
}

impl Zb2Brain {
    pub fn new(skill: BotSkill, combat_enabled: bool) -> Self {
        let steering = Steering::new(skill.combat());
        Self {
            skill,
            combat: CombatDriver::new(),
            steering,
            recovery: Recovery::new(),
            traverse: TraversalExecutor::new(),
            roam_nodes: Vec::new(),
            roam_idx: 0,
            nav_graph: None,
            route: None,
            combat_enabled,
        }
    }

    /// Advance the roam cursor to the next destination (stride mirrors `main`'s roam spread).
    fn next_roam_goal(&mut self) {
        if !self.roam_nodes.is_empty() {
            self.roam_idx = (self.roam_idx + self.roam_nodes.len() / 7 + 1) % self.roam_nodes.len();
        }
    }

    /// Resolve this tick's committed DESTINATION node. Priority: scenario `goal_override` >
    /// weapon run while Blaster-armed (3ZB2's weapon-aware route selection, T4) > roam cursor.
    fn goal_node(
        &mut self,
        graph: &NavGraph,
        view: &crate::perception::Worldview,
        goal_override: &Option<NavGoal>,
    ) -> Option<usize> {
        if let Some(g) = goal_override {
            return match g {
                NavGoal::Waypoint(n) => Some(*n),
                NavGoal::Position(p) | NavGoal::Entity(p) => graph.nearest(&[p.x, p.y, p.z]),
            };
        }
        // T4 weapon run: while stuck on the Blaster, route to a visible weapon pickup.
        let ss = view.self_state();
        if matches!(ss.held_weapon, None | Some(Weapon::Blaster)) {
            if let Some((p, EntityClass::ItemWeapon)) = items::best_item_goal_weighted(
                view,
                &self.skill,
                ss.held_weapon,
                ss.health,
                ss.armor,
            ) {
                return graph.nearest(&[p.x, p.y, p.z]);
            }
        }
        self.roam_nodes.get(self.roam_idx).copied()
    }
}

impl Brain for Zb2Brain {
    fn set_map(&mut self, map: BrainMap) {
        let BrainMap {
            roam_nodes,
            nav_graph,
            roam_as_position: _, // zb2 always routes on the A* graph (see module docs)
            items: _,            // v1 uses the PVS item picker, not the static table
        } = map;
        self.roam_nodes = roam_nodes;
        self.nav_graph = Some(nav_graph);
        self.route = None;
    }

    fn tick(&mut self, ctx: BrainContext) -> BrainOutput {
        let BrainContext {
            view,
            nav: _, // deliberate: zb2 runs its own committed route (module docs)
            cm,
            dt,
            ticks,
            goal_override,
        } = ctx;
        let pos = view.self_state().origin;
        let mut mv = MovementIntent::new();

        // ── 1. Combat read (shared driver; movement stays on the route) ─────────────
        let combat_dec = if self.combat_enabled {
            self.combat
                .evaluate(view, &self.skill, (ticks as f32) * 0.1, cm)
        } else {
            CombatDecision::default()
        };

        let Some(graph) = self.nav_graph.clone() else {
            // No map yet — just walk forward so the bot isn't a statue.
            mv.move_forward(1.0);
            return BrainOutput {
                intent: mv,
                weapon_request: combat_dec.weapon_request.map(|r| r.0),
                intent_forward: 1.0,
            };
        };

        // ── 2. Route commitment (3ZB2: plan once, then RUN it) ──────────────────────
        let goal = self.goal_node(&graph, view, &goal_override);
        let needs_plan = match (&self.route, goal) {
            (_, None) => false,
            (None, Some(_)) => true,
            (Some(r), Some(g)) => r.goal_node != g || r.dirty || r.finished(),
        };
        if needs_plan {
            let g = goal.expect("needs_plan implies goal");
            let planned = graph
                .nearest(&[pos.x, pos.y, pos.z])
                .and_then(|from| graph.path(from, g));
            match planned {
                Some(path) => {
                    self.route = Some(Zb2Route {
                        graph: Arc::clone(&graph),
                        path,
                        idx: 0,
                        goal_node: g,
                        dirty: false,
                    });
                }
                None => {
                    // Unreachable destination: rotate the roam cursor and try again next tick.
                    self.route = None;
                    self.next_roam_goal();
                }
            }
        }

        let mut intent_forward = 0.0;
        if let Some(route) = self.route.as_mut() {
            // ── 3. Run the route: advance, then traversal gates, then shortcut ──────
            route.update(pos, cm);
            if route.finished() && !self.roam_nodes.is_empty() {
                // Destination reached — rotate the roam cursor (inlined: a method call would
                // borrow all of `self` while `route` holds `self.route`).
                self.roam_idx =
                    (self.roam_idx + self.roam_nodes.len() / 7 + 1) % self.roam_nodes.len();
            }
            let gates = self.traverse.gates(route, cm, pos, dt);

            // `Search_NearlyPod`: only while on plain ground (a mover leg owns the cursor).
            if !gates.any() {
                if let Some(cmodel) = cm {
                    let eye = los::eye_origin(pos.into());
                    let g2 = &route.graph;
                    let path = &route.path;
                    let skipped = nearly_pod_skip(
                        route.idx,
                        pos,
                        &|j| Vec3::from(g2.node_pos(path[j])),
                        path.len(),
                        &|j| matches!(g2.edge_kind(path[j - 1], path[j]), EdgeKind::Walk),
                        &|p| los::has_los_player(cmodel, eye, [p.x, p.y, p.z]),
                    );
                    if skipped > route.idx {
                        tracing::debug!(from = route.idx, to = skipped, "zb2 shortcut skip");
                        route.idx = skipped;
                    }
                }
            }

            // ── 4. Steering along the committed route ───────────────────────────────
            let pursue = route.pursue_target(pos);
            let (ideal_yaw, world_dir) = match pursue {
                Some(pt) if (pt - pos).length_squared() > 1.0 => {
                    let d = pt - pos;
                    (
                        d.y.atan2(d.x).to_degrees(),
                        Vec3::new(d.x, d.y, 0.0).normalize_or_zero(),
                    )
                }
                _ => (self.steering.view_yaw(), Vec3::ZERO),
            };
            let view_yaw = self.steering.change_yaw(ideal_yaw, dt);
            let arrive = pursue
                .map(|pt| Steering::arrive_scale((pt - pos).length()))
                .unwrap_or(1.0);
            let (fwd, side) = move_from_world_dir(world_dir, view_yaw, true);
            mv.look_at(view_yaw, 0.0);
            mv.move_forward(fwd * arrive);
            mv.move_side(side * arrive);
            intent_forward = fwd * arrive;

            // ── 5. Recovery (suspended while traversing); escalation → replan ───────
            if !gates.any() {
                let has_target = pursue.is_some();
                match self.recovery.evaluate(
                    pos,
                    dt,
                    cm,
                    view_yaw,
                    has_target,
                    combat_dec.should_fire,
                ) {
                    RecoveryAction::None => {}
                    RecoveryAction::Jump => mv.jump(),
                    RecoveryAction::Strafe { dir } => mv.move_side(dir),
                    RecoveryAction::BackOffThenRepath => {
                        mv.move_forward(-0.5);
                        route.dirty = true; // hard stuck — recommit a fresh route next tick
                    }
                    RecoveryAction::UseHeading(yaw) => {
                        let r = yaw.to_radians();
                        let free = Vec3::new(r.cos(), r.sin(), 0.0);
                        let (hf, hs) = move_from_world_dir(free, view_yaw, true);
                        mv.move_forward(hf);
                        mv.move_side(hs);
                    }
                }
                if route.current_edge_is_jump() {
                    mv.jump();
                }
            }

            // ── 6. Fight on the run (only outside traversal legs — the executor owns
            // the view while climbing/riding/surfacing) ──────────────────────────────
            let mut traversing = false;
            let frame = TraversalFrame {
                view,
                cm,
                pos,
                view_yaw,
                steer_fwd: fwd,
                steer_side: side,
                dt,
            };
            if let Some(applied) = self.traverse.apply(&mut mv, gates, route, &frame) {
                intent_forward = applied.intent_forward;
                traversing = true;
            }
            if combat_dec.should_fire && !traversing {
                // Lock the view on the enemy but KEEP the legs on the route (3ZB2's run-and-gun):
                // re-decompose the same world direction against the aim yaw (raw mode) so the
                // world-space travel direction is preserved while facing the target.
                mv.look_at(combat_dec.aim_yaw, combat_dec.aim_pitch);
                let (ff, ss) = move_from_world_dir(world_dir, combat_dec.aim_yaw, false);
                mv.move_forward(ff * arrive);
                mv.move_side(ss * arrive);
                mv.attack();
            }
        } else if !combat_dec.should_fire {
            mv.move_forward(1.0); // no route yet — keep moving
            intent_forward = 1.0;
        }

        BrainOutput {
            intent: mv,
            weapon_request: combat_dec.weapon_request.map(|r| r.0),
            intent_forward,
        }
    }

    fn on_kill(&mut self) {
        self.skill.on_kill();
    }

    fn on_death(&mut self) {
        self.combat.on_respawn();
        self.skill.on_death();
        self.route = None; // we respawned elsewhere — the committed route is void
    }

    fn status(&self) -> &str {
        "zb2"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight 8-node path along +x at z=0, 100u apart.
    fn straight_pos(j: usize) -> Vec3 {
        Vec3::new(100.0 * j as f32, 0.0, 0.0)
    }

    #[test]
    fn shortcut_skips_to_furthest_visible_walk_node() {
        // All walk edges, everything visible → skip the full lookahead cap.
        let skipped = nearly_pod_skip(1, straight_pos(1), &straight_pos, 8, &|_| true, &|_| true);
        assert_eq!(skipped, 7, "cap = min(idx+6, len-1)");
        // Nothing visible → stay put.
        let none = nearly_pod_skip(1, straight_pos(1), &straight_pos, 8, &|_| true, &|_| false);
        assert_eq!(none, 1);
    }

    #[test]
    fn shortcut_never_crosses_a_non_walk_edge() {
        // Edge into node 4 is a ride — the scan must stop at 3 even though 5+ are visible.
        let walk = |j: usize| j != 4;
        let skipped = nearly_pod_skip(1, straight_pos(1), &straight_pos, 8, &walk, &|_| true);
        assert_eq!(skipped, 3, "stop before the ride edge");
    }

    #[test]
    fn shortcut_respects_the_dz_gate() {
        // Node 3 is a floor above — visible through a railing, but LOS ≠ walkable.
        let pos = straight_pos(1);
        let node = |j: usize| {
            if j == 3 {
                Vec3::new(300.0, 0.0, 128.0)
            } else {
                straight_pos(j)
            }
        };
        let skipped = nearly_pod_skip(1, pos, &node, 4, &|_| true, &|_| true);
        assert_eq!(skipped, 2, "the elevated node is not a valid skip target");
    }

    #[test]
    fn route_facade_reports_edges_and_advances() {
        // A tiny 3-node line graph via from_raw: 0-(walk)-1-(walk)-2.
        let graph = Arc::new(NavGraph::from_raw(
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [200.0, 0.0, 0.0]],
            vec![
                vec![(1, 100.0)],
                vec![(0, 100.0), (2, 100.0)],
                vec![(1, 100.0)],
            ],
        ));
        let mut route = Zb2Route {
            graph,
            path: vec![0, 1, 2],
            idx: 0,
            goal_node: 2,
            dirty: false,
        };
        // At the start node → cursor advances past it to node 1.
        route.update(Vec3::new(0.0, 0.0, 0.0), None);
        assert_eq!(route.current_waypoint(), Some(1));
        assert!(!route.current_edge_is_ride() && !route.current_edge_is_swim());
        // Arrive at node 1 → cursor moves to 2; finish at 2.
        route.update(Vec3::new(100.0, 0.0, 0.0), None);
        assert_eq!(route.current_waypoint(), Some(2));
        route.update(Vec3::new(200.0, 0.0, 0.0), None);
        assert!(route.finished());
    }
}
