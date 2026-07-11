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

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use glam::Vec3;
use world::navgraph::segment_has_floor;
use world::{CollisionModel, EdgeKind, NavGraph, HULL_MAXS, HULL_MINS, MASK_SOLID};

use crate::brains::core::{Brain, BrainContext, BrainMap, BrainOutput};
use crate::combat::{CombatDecision, CombatDriver};
use crate::move_ctrl::MovementIntent;
use crate::nav::NavGoal;
use crate::nav_mode::Navigator;
use crate::perception::EntityClass;
use crate::recover::{Recovery, RecoveryAction};
use crate::skill::BotSkill;
use crate::steer::{move_from_world_dir, view_forward, view_right, Steering};
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
/// How long a destination stays blocked after two consecutive hard-stuck replans against it
/// (Plan 48 Z3). Committed routes have no waypoint blacklist, so replanning to the same goal
/// recommits the identical polyline — blocking the goal is what breaks the wall-grind loop.
const GOAL_BLOCK_SECS: f32 = 20.0;
/// Waypoint-progress watchdog (Plan 51 R1): if the best distance to the current waypoint
/// hasn't improved by [`PROGRESS_EPS`] for this long, the route is unrunnable from here —
/// force a replan. The displacement-based `StuckDetector` cannot catch this: its recovery
/// strafe slides the bot ALONG a wall at 30–100 u/s (above its 16 u deadband), so it
/// resets forever and `BackOffThenRepath` never fires (proved in the Plan 51 micro-soak).
const PROGRESS_STALL_SECS: f32 = 2.5;
/// Minimum improvement (u) of the best waypoint distance that counts as progress.
const PROGRESS_EPS: f32 = 8.0;

/// Process-wide zb2 ordinal (Plan 51 R3): every zb2 bot used to start its roam cursor at
/// the SAME index with the same stride, so entire fleets convoyed to identical
/// destinations and deadlocked hull-to-hull. Each brain takes the next ordinal and offsets
/// its cursor one roam-stride apart.
static BOT_ORDINAL: AtomicUsize = AtomicUsize::new(0);

/// Waypoint-progress watchdog state (Plan 51 R1).
#[derive(Debug, Clone, Copy, Default)]
struct RouteProgress {
    /// The waypoint being measured (watchdog resets when it changes).
    wp: Option<usize>,
    /// Best (smallest) distance to that waypoint seen so far.
    best_dist: f32,
    /// Seconds since `best_dist` last improved by [`PROGRESS_EPS`].
    secs_no_gain: f32,
}

impl RouteProgress {
    /// Feed one tick of (current waypoint, distance to it). Returns `true` when the bot
    /// has made no progress toward the waypoint for [`PROGRESS_STALL_SECS`] — the caller
    /// should force a replan. Self-resets after firing.
    fn stalled(&mut self, wp: Option<usize>, dist: f32, dt: f32) -> bool {
        if wp != self.wp {
            self.wp = wp;
            self.best_dist = dist;
            self.secs_no_gain = 0.0;
            return false;
        }
        if dist < self.best_dist - PROGRESS_EPS {
            self.best_dist = dist;
            self.secs_no_gain = 0.0;
            return false;
        }
        self.secs_no_gain += dt;
        if self.secs_no_gain >= PROGRESS_STALL_SECS {
            *self = Self::default();
            return true;
        }
        false
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

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
/// `visible(target)` must hold, AND `walkable(target)` must hold. Visibility alone is the
/// classic bot trap (Plan 48 Z1): on q2dm3 the committed polyline curves AROUND the lava, and
/// a same-height node on the far side is eye-visible — the walkability check (hull trace +
/// lava-aware floor continuity at the call site) is what keeps the skip on dry land.
/// Returns `idx` unchanged when no skip qualifies.
fn nearly_pod_skip(
    idx: usize,
    pos: Vec3,
    node_pos: &dyn Fn(usize) -> Vec3,
    path_len: usize,
    walk_edge_into: &dyn Fn(usize) -> bool, // is edge (path[j-1] → path[j]) a Walk edge?
    visible: &dyn Fn(Vec3) -> bool,
    walkable: &dyn Fn(Vec3) -> bool, // is the straight line pos → target hull+floor valid?
) -> usize {
    let mut best = idx;
    let cap = (idx + SHORTCUT_LOOKAHEAD).min(path_len.saturating_sub(1));
    let mut j = idx + 1;
    while j <= cap {
        if !walk_edge_into(j) {
            break; // never skip across a jump/swim/ride edge
        }
        let p = node_pos(j);
        if (p.z - pos.z).abs() <= SHORTCUT_MAX_DZ && visible(p) && walkable(p) {
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
    /// Consecutive hard-stuck (`BackOffThenRepath`) replans since the last clean plan (Z3).
    hard_replans: u32,
    /// A destination node blocked for a few seconds after repeated stuck replans, with its
    /// remaining TTL — `goal_node` routes around it (Z3).
    goal_block: Option<(usize, f32)>,
    /// Waypoint-progress watchdog (Plan 51 R1) — replans routes the bot can't run.
    progress: RouteProgress,
    /// This bot's process-wide zb2 ordinal (Plan 51 R3) — desyncs the roam cursor.
    ordinal: usize,
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
            hard_replans: 0,
            goal_block: None,
            progress: RouteProgress::default(),
            ordinal: BOT_ORDINAL.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// True while `n` is the temporarily-blocked destination (Z3).
    fn is_blocked(&self, n: usize) -> bool {
        self.goal_block.is_some_and(|(b, _)| b == n)
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
                if let Some(n) = graph.nearest(&[p.x, p.y, p.z]) {
                    // A stuck-blocked weapon goal falls through to roaming (Z3).
                    if !self.is_blocked(n) {
                        return Some(n);
                    }
                }
            }
        }
        let roam = self.roam_nodes.get(self.roam_idx).copied();
        if roam.is_some_and(|n| self.is_blocked(n)) {
            self.next_roam_goal();
            return self.roam_nodes.get(self.roam_idx).copied();
        }
        roam
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
        // R3: start each bot one roam-stride apart so a fleet of zb2s doesn't convoy to
        // the same destination sequence and deadlock hull-to-hull (Plan 51).
        if !self.roam_nodes.is_empty() {
            let stride = self.roam_nodes.len() / 7 + 1;
            self.roam_idx = (self.ordinal * stride) % self.roam_nodes.len();
        }
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

        // Tick down the stuck-blocked destination (Z3).
        if let Some((_, ttl)) = &mut self.goal_block {
            *ttl -= dt;
            if *ttl <= 0.0 {
                self.goal_block = None;
            }
        }

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
            // Z3: a hard-stuck replan (route.dirty) to the SAME destination recommits the
            // identical polyline — Zb2Route has no waypoint blacklist, so the bot grinds
            // the same wall forever. Two consecutive stuck replans block the destination
            // for GOAL_BLOCK_SECS and re-goal next tick.
            let stuck_replan = self.route.as_ref().is_some_and(|r| r.dirty);
            if stuck_replan {
                self.hard_replans += 1;
            }
            if stuck_replan && self.hard_replans >= 2 && goal_override.is_none() {
                tracing::debug!(goal = g, "zb2 stuck-replan loop — blocking destination");
                self.goal_block = Some((g, GOAL_BLOCK_SECS));
                self.hard_replans = 0;
                self.route = None;
                self.next_roam_goal();
            } else {
                let planned = graph
                    .nearest(&[pos.x, pos.y, pos.z])
                    .and_then(|from| graph.path(from, g));
                match planned {
                    Some(path) => {
                        if !stuck_replan {
                            self.hard_replans = 0; // clean plan (goal change / finish)
                        }
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
        }

        let mut intent_forward;
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
                        &|p| {
                            // LOS ≠ walkable: the straight run to the skip target must be
                            // hull-clear AND have continuous non-lava floor (Plan 48 Z1).
                            let a = [pos.x, pos.y, pos.z];
                            let b = [p.x, p.y, p.z];
                            let t = cmodel.trace(&a, &b, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
                            !t.startsolid && t.fraction >= 1.0 && segment_has_floor(cmodel, a, b)
                        },
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
            // Creep on hazard-bordered stretches (lava walkways) so 10 Hz tracking
            // error can't step us off the edge (Plan 50).
            let arrive = pursue
                .map(|pt| Steering::arrive_scale((pt - pos).length()))
                .unwrap_or(1.0)
                * crate::hazard::creep_scale(cm, pos, world_dir);
            let (fwd, side) = move_from_world_dir(world_dir, view_yaw, true);
            mv.look_at(view_yaw, 0.0);
            mv.move_forward(fwd * arrive);
            mv.move_side(side * arrive);
            intent_forward = fwd * arrive;

            // ── 5. Recovery (suspended while traversing); escalation → replan ───────
            // Plan 51 probe: remember what recovery asked for this tick so the combat
            // block below can report when it overwrites those legs (observation only).
            let mut recovery_label: Option<&'static str> = None;
            if !gates.any() {
                let has_target = pursue.is_some();
                let action = self.recovery.evaluate(
                    pos,
                    dt,
                    cm,
                    view_yaw,
                    has_target,
                    combat_dec.should_fire,
                );
                recovery_label = action.label();
                if let Some(label) = recovery_label {
                    // Plan 51: per-tick recovery visibility for stall forensics
                    // (debug level — soaks run at info; enable with RUST_LOG=brain=debug).
                    tracing::debug!(
                        action = label,
                        x = pos.x as i32,
                        y = pos.y as i32,
                        z = pos.z as i32,
                        wp = ?route.current_waypoint(),
                        "zb2 recovery"
                    );
                }
                match action {
                    RecoveryAction::None => {}
                    RecoveryAction::Jump => mv.jump(),
                    RecoveryAction::Strafe { dir } => {
                        // Flip a stuck-strafe that would side-step into lava (Plan 48 L2).
                        mv.move_side(crate::hazard::safe_strafe_dir(cm, pos, view_yaw, dir));
                    }
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

                // R1: waypoint-progress watchdog (Plan 51). The committed route only
                // replans on Hard stuck, but recovery's own strafe slides the bot along
                // walls fast enough to reset the displacement-based detector forever.
                // Distance-to-waypoint can't be gamed by sliding: no gain for
                // PROGRESS_STALL_SECS → the route is unrunnable from here → replan
                // (feeds the Z3 goal-block ladder on repeat, exactly like Hard stuck).
                match pursue {
                    Some(pt) => {
                        if self
                            .progress
                            .stalled(route.current_waypoint(), (pt - pos).length(), dt)
                        {
                            tracing::info!(
                                wp = ?route.current_waypoint(),
                                x = pos.x as i32,
                                y = pos.y as i32,
                                z = pos.z as i32,
                                "EVT zb2_progress_replan"
                            );
                            mv.move_forward(-0.5);
                            route.dirty = true;
                        }
                    }
                    None => self.progress.reset(),
                }
            } else {
                // Mover legs (ride/swim/ladder) hold position legitimately — don't let
                // the watchdog count a platform wait as a stall.
                self.progress.reset();
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
                // R2 (Plan 51): this block used to re-derive the legs from the route's
                // `world_dir`, silently DISCARDING whatever recovery wrote above — while
                // firing, a wall-pressed bot lost even its strafe and stood grinding
                // (521 of 806 stalled-damage points came from firing episodes). Instead,
                // re-express the legs `mv` already carries (route steering + arrive +
                // recovery + hazard flips, all view_yaw-relative) against the aim yaw:
                // the world-space travel direction is preserved exactly, the view locks
                // onto the enemy, and recovery keeps working mid-fight (3ZB2's
                // run-and-gun character is unchanged when recovery is idle).
                if let Some(action) = recovery_label {
                    let wp_dist = pursue.map(|p| (p - pos).length() as i32).unwrap_or(-1);
                    tracing::debug!(action, wp_dist, "zb2 combat keeps recovery legs");
                }
                let legs_world =
                    view_forward(view_yaw) * mv.forward + view_right(view_yaw) * mv.side;
                mv.look_at(combat_dec.aim_yaw, combat_dec.aim_pitch);
                let (ff, ss) = move_from_world_dir(legs_world, combat_dec.aim_yaw, false);
                mv.move_forward(ff);
                mv.move_side(ss);
                mv.attack();
            }
        } else {
            // No committed route (plan failed — e.g. off-graph after a fall, or every roam
            // goal is momentarily unpathable). The old code FROZE here when an enemy was
            // visible and blind-ran `forward(1.0)` into walls otherwise (Plan 48 Z2). Steer
            // via recovery's free-space heading (`find_best_direction` avoids walls, ledges
            // AND lava) and keep fighting while we relocate.
            let cur_yaw = self.steering.view_yaw();
            let heading =
                match self
                    .recovery
                    .evaluate(pos, dt, cm, cur_yaw, false, combat_dec.should_fire)
                {
                    RecoveryAction::UseHeading(yaw) => yaw,
                    _ => cur_yaw, // no CM yet — walk the current facing
                };
            let view_yaw = self.steering.change_yaw(heading, dt);
            let r = heading.to_radians();
            let free = Vec3::new(r.cos(), r.sin(), 0.0);
            if combat_dec.should_fire {
                // Run-and-gun even without a route: view locks the enemy, legs keep moving.
                mv.look_at(combat_dec.aim_yaw, combat_dec.aim_pitch);
                let (ff, ss) = move_from_world_dir(free, combat_dec.aim_yaw, false);
                mv.move_forward(ff);
                mv.move_side(ss);
                mv.attack();
                intent_forward = ff;
            } else {
                mv.look_at(view_yaw, 0.0);
                let (ff, ss) = move_from_world_dir(free, view_yaw, true);
                mv.move_forward(ff);
                mv.move_side(ss);
                intent_forward = ff;
            }
        }

        // Survival override (Plan 50 E2): standing in lava/slime outranks combat, dodge,
        // and route — face the nearest safe floor, sprint, swim up, and jump the pool rim.
        // Health-gated: a sinking corpse still ticks — don't "escape" (or log) post-mortem.
        if let Some(c) = cm {
            if let Some(esc) = (view.self_state().health > 0)
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
                intent_forward = 1.0;
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
        self.hard_replans = 0;
        self.goal_block = None;
        self.progress.reset();
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
        // All walk edges, everything visible + walkable → skip the full lookahead cap.
        let skipped = nearly_pod_skip(
            1,
            straight_pos(1),
            &straight_pos,
            8,
            &|_| true,
            &|_| true,
            &|_| true,
        );
        assert_eq!(skipped, 7, "cap = min(idx+6, len-1)");
        // Nothing visible → stay put.
        let none = nearly_pod_skip(
            1,
            straight_pos(1),
            &straight_pos,
            8,
            &|_| true,
            &|_| false,
            &|_| true,
        );
        assert_eq!(none, 1);
    }

    #[test]
    fn shortcut_never_crosses_a_non_walk_edge() {
        // Edge into node 4 is a ride — the scan must stop at 3 even though 5+ are visible.
        let walk = |j: usize| j != 4;
        let skipped = nearly_pod_skip(
            1,
            straight_pos(1),
            &straight_pos,
            8,
            &walk,
            &|_| true,
            &|_| true,
        );
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
        let skipped = nearly_pod_skip(1, pos, &node, 4, &|_| true, &|_| true, &|_| true);
        assert_eq!(skipped, 2, "the elevated node is not a valid skip target");
    }

    #[test]
    fn shortcut_respects_the_walkable_gate() {
        // Plan 48 Z1: nodes past x=300 are across a lava pool — visible and level, but the
        // straight line to them has no continuous floor. The skip must stop at the last
        // walkable node instead of steering across the pool.
        let walkable = |p: Vec3| p.x <= 300.0;
        let skipped = nearly_pod_skip(
            1,
            straight_pos(1),
            &straight_pos,
            8,
            &|_| true,
            &|_| true,
            &walkable,
        );
        assert_eq!(
            skipped, 3,
            "never skip across a floor gap the eye sees over"
        );
    }

    /// Plan 51 R1: the watchdog must ignore genuine approach, catch wall-slide
    /// oscillation (distance never improving past the epsilon), and self-reset.
    #[test]
    fn progress_watchdog_fires_only_without_gain() {
        let mut p = RouteProgress::default();
        // Approaching at 20 u/s: distance keeps improving — never fires.
        let mut d = 500.0;
        for _ in 0..50 {
            assert!(!p.stalled(Some(7), d, 0.1));
            d -= 2.0;
        }
        // Wall-slide: distance oscillates ±4 u (the exact micro-soak signature —
        // fast displacement, zero waypoint progress) — fires within ~2.5 s.
        let mut fired = false;
        for i in 0..30 {
            let dist = 400.0 + if i % 2 == 0 { 4.0 } else { -4.0 };
            if p.stalled(Some(7), dist, 0.1) {
                fired = true;
                break;
            }
        }
        assert!(
            fired,
            "no waypoint progress for 2.5 s must trigger a replan"
        );
        // Self-reset after firing: the next tick starts a fresh measurement.
        assert!(!p.stalled(Some(7), 400.0, 0.1));
    }

    #[test]
    fn progress_watchdog_resets_on_waypoint_change() {
        let mut p = RouteProgress::default();
        for _ in 0..20 {
            assert!(!p.stalled(Some(1), 100.0, 0.1)); // 2.0 s — below threshold
        }
        assert!(
            !p.stalled(Some(2), 100.0, 0.1),
            "advancing to a new waypoint restarts the clock"
        );
        for _ in 0..20 {
            assert!(!p.stalled(Some(2), 100.0, 0.1), "still under 2.5 s");
        }
        // Fires within the next few ticks (~2.5 s; exact tick depends on f32 accumulation).
        let fired = (0..6).any(|_| p.stalled(Some(2), 100.0, 0.1));
        assert!(fired, "~2.5 s on the new waypoint must fire");
    }

    /// Plan 51 R3: fleet zb2 bots must not start their roam cursors in lockstep.
    #[test]
    fn roam_cursor_desyncs_across_bots() {
        let graph = Arc::new(NavGraph::from_raw(
            vec![[0.0, 0.0, 0.0]; 20],
            vec![Vec::new(); 20],
        ));
        let map = || BrainMap {
            roam_nodes: (0..20).collect(),
            nav_graph: Arc::clone(&graph),
            roam_as_position: false,
            items: Vec::new(),
        };
        let mut a = Zb2Brain::new(BotSkill::default(), false);
        let mut b = Zb2Brain::new(BotSkill::default(), false);
        a.set_map(map());
        b.set_map(map());
        let stride = 20 / 7 + 1; // 3
        assert_ne!(a.ordinal, b.ordinal, "each brain takes a unique ordinal");
        assert_eq!(a.roam_idx, (a.ordinal * stride) % 20);
        assert_eq!(b.roam_idx, (b.ordinal * stride) % 20);
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
