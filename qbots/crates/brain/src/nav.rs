//! Navigation driver — A* over the nav graph with stuck recovery.

use glam::Vec3;
use std::collections::HashSet;
use std::sync::Arc;
use world::collision::MASK_SOLID;
use world::{
    navgraph::{segment_has_floor, HULL_MAXS, HULL_MINS},
    CollisionModel, EdgeKind, NavGraph,
};

/// Hard cap on pursuing a single goal without reaching a waypoint. At 10 Hz
/// this is ~3 s — past it we blacklist the stuck waypoint and replan around it.
/// 1.5 s at 10 Hz. Failing to reach a waypoint in 1.5 s → blacklist + replan.
/// Faster than 3 s so bots don't waste time on false-edge nodes.
const GOAL_GIVEUP_TICKS: i32 = 15;
/// Give-up blacklist cap: 32 lets bots avoid the same wall-blocked false edges
/// across multiple replans without poisoning large graph areas.
const GIVEUP_BLACKLIST_MAX: usize = 32;
/// Ledge blacklist cap: persistent within a goal attempt; can be larger because
/// ledge nodes are confirmed false-edge targets, not just transient obstacles.
const LEDGE_BLACKLIST_MAX: usize = 64;
/// Desperate-requery guard (Plan 08 T3): if a risk-weighted path is more than
/// this many × the straight-line distance, the whole region is hot and we drop
/// the overlay rather than pay an absurd detour ("desperate re-query with W_d=0").
const DEGEN_FACTOR: f32 = 5.0;
/// Only apply the degeneracy guard past this straight-line distance, so a tiny
/// goal (where the ratio is meaningless) can't trigger it.
const DEGEN_MIN_STRAIGHT: f32 = 256.0;
// ── Grid-coupled steering constants ─────────────────────────────────────────────
// These are tuned as RATIOS to the nav-graph node spacing, not absolute distances:
// the bot advances/orbits/looks-ahead relative to how far apart waypoints are. Tuning
// them for grid=24 then changing the grid (without re-scaling) corner-cuts and bumps
// (q2dm1: grid=18 with fixed consts = 14/24; scaled 0.75 = 22/24). Deriving them from
// `world::GRID_SPACING` keeps the ratios fixed so the grid can be changed freely.
// At grid=24 these evaluate to the original 96 / 32 / 24 / 48 (behaviour-preserving).

/// Look-ahead distance for pure-pursuit steering: aim at a point this far ahead along the
/// path polyline from the bot's PROJECTION onto it. This is a FIXED physical distance (a
/// steering-smoothness quantity, like a car's look-ahead), NOT grid-scaled: pure-pursuit is
/// density-independent, and scaling the look-ahead down at finer grids would make steering
/// jaggier. 96u rounds corners smoothly without overshooting q2dm geometry.
pub const LOOKAHEAD: f32 = 96.0;
/// Horizontal waypoint-reach threshold = 4/3 × node spacing (32u at grid 24). Big enough
/// to absorb full-speed overshoot (~30u/frame) without skipping across wall boundaries.
const WP_REACH_HORIZ: f32 = world::GRID_SPACING * 4.0 / 3.0;
/// Vertical waypoint-reach (arrival) tolerance = 1 × node spacing (24u at grid 24).
/// Tolerates step heights on ledges where the bot's XY is already past the node. NOT a
/// step-climb constant (cf. `world::navgraph::STEP = 18`).
const WP_REACH_DZ: f32 = world::GRID_SPACING;
/// Orbit watchdog radius = 2 × node spacing (48u at grid 24): if the bot circles within
/// this of the current waypoint for `ORBIT_FRAMES` ticks without reaching it, force-advance.
/// Small enough not to fire while genuinely rounding a nearby corner (StuckLevel::Hard
/// handles those with a full replan).
const ORBIT_RADIUS: f32 = world::GRID_SPACING * 2.0;
/// Ticks close to the current waypoint before we force-advance (2.5 s at 10 Hz).
/// Extra time lets bots navigate around corners before orbit fires.
pub const ORBIT_FRAMES: u32 = 25;

#[derive(Debug, Clone, PartialEq)]
pub enum NavGoal {
    Waypoint(usize),
    Position(Vec3),
    Entity(Vec3),
}

#[derive(Clone)]
pub struct NavigationDriver {
    nav_graph: Arc<NavGraph>,
    current_path: Vec<usize>,
    current_waypoint: Option<usize>,
    /// Goal node from the last successful plan; used to skip redundant replans.
    last_goal_node: Option<usize>,
    last_position: Option<Vec3>,
    /// Ticks since the current goal was set without reaching a waypoint. Drives
    /// the give-up watchdog.
    goal_age_ticks: i32,
    /// True for one tick after the watchdog abandons a stale goal.
    goal_abandoned: bool,
    /// Per-node additive cost overlay for risk-weighted A\* (Plan 08 T3): each
    /// edge leaving node `n` costs `base + overlay[n]`. Set each tick from the
    /// heatmap (`W_d·danger − W_p·popularity`); `None` = unweighted routing.
    risk_overlay: Option<Vec<f32>>,
    /// Consecutive ticks inside `ORBIT_RADIUS` of the current waypoint without
    /// reaching it. Drives the orbit-timeout force-advance (Plan 12 T3).
    near_wp_ticks: u32,
    /// Most recently completed waypoint (Plan 14 T2: jump-edge detection).
    prev_waypoint: Option<usize>,
    /// Waypoints that caused repeated give-up failures. Small cap — evicts oldest
    /// when full so a bursty stuck episode doesn't poison the whole graph.
    waypoint_blacklist: std::collections::VecDeque<usize>,
    /// Confirmed false-ledge nodes (dz > LEDGE_DZ orbit-timeout). Larger cap;
    /// persists for the whole goal attempt so A* routes around them every replan.
    ledge_blacklist: std::collections::VecDeque<usize>,
    /// Specific (prev, dest) edges confirmed to cause falls. More surgical than
    /// node blacklisting: lets A* still reach `dest` via other incoming edges
    /// (different approach directions) while avoiding the exact dangerous path.
    edge_blacklist: HashSet<(usize, usize)>,
}

impl NavigationDriver {
    pub fn new(nav_graph: Arc<NavGraph>) -> Self {
        Self {
            nav_graph,
            current_path: Vec::new(),
            current_waypoint: None,
            last_goal_node: None,
            last_position: None,
            goal_age_ticks: 0,
            goal_abandoned: false,
            risk_overlay: None,
            near_wp_ticks: 0,
            prev_waypoint: None,
            waypoint_blacklist: std::collections::VecDeque::new(),
            ledge_blacklist: std::collections::VecDeque::new(),
            edge_blacklist: HashSet::new(),
        }
    }

    /// Install the risk overlay consumed by the next `set_goal` (Plan 08 T3).
    /// Build it from the heatmap as `W_d·danger − W_p·popularity`. Cheap to call
    /// every tick — `set_goal` only re-runs A\* when the goal changes.
    pub fn set_risk_overlay(&mut self, overlay: Vec<f32>) {
        self.risk_overlay = Some(overlay);
    }

    /// Drop the risk overlay, returning to unweighted routing. Use as the
    /// "desperate re-query" when danger-routing wedges the bot (stuck recovery).
    pub fn clear_risk_overlay(&mut self) {
        self.risk_overlay = None;
    }

    /// Set (or update) the navigation goal. Replans the A* path only when the goal
    /// changes or the current path is exhausted. `set_goal` is safe to call every tick.
    pub fn set_goal(&mut self, goal: NavGoal, from_position: Vec3) {
        let target_waypoint = match goal {
            NavGoal::Waypoint(idx) => Some(idx),
            NavGoal::Position(pos) => {
                tracing::trace!("nav goal Position({:.1},{:.1},{:.1})", pos.x, pos.y, pos.z);
                self.nav_graph.nearest(&[pos.x, pos.y, pos.z])
            }
            NavGoal::Entity(pos) => {
                tracing::trace!("nav goal Entity({:.1},{:.1},{:.1})", pos.x, pos.y, pos.z);
                self.nav_graph.nearest(&[pos.x, pos.y, pos.z])
            }
        };

        let Some(target) = target_waypoint else {
            tracing::warn!("nav goal: no reachable waypoint found");
            return;
        };

        // Don't replan if goal unchanged and we still have a waypoint to follow.
        if self.last_goal_node == Some(target) && self.current_waypoint.is_some() {
            return;
        }
        self.last_goal_node = Some(target);

        let Some(start) =
            self.nav_graph
                .nearest(&[from_position.x, from_position.y, from_position.z])
        else {
            return;
        };
        let sp = self.nav_graph.nodes[start];
        tracing::debug!(
            start,
            start_pos = ?[sp[0] as i32, sp[1] as i32, sp[2] as i32],
            "set_goal: planning from start node"
        );

        if let Some(path) = self.plan_path(start, target) {
            self.commit_path(path);
        } else {
            // Different components: path within start's component toward target.
            let target_pos = self.nav_graph.nodes[target];
            let sc = self.nav_graph.nodes[start];
            let tc = self.nav_graph.nodes[target];
            tracing::debug!(
                start,
                start_pos = ?[sc[0] as i32, sc[1] as i32, sc[2] as i32],
                target,
                target_pos = ?[tc[0] as i32, tc[1] as i32, tc[2] as i32],
                bl_nodes = self.waypoint_blacklist.len() + self.ledge_blacklist.len(),
                bl_edges = self.edge_blacklist.len(),
                "A* failed — falling back to nearest_reachable_from"
            );
            if let Some(alt) = self.nav_graph.nearest_reachable_from(start, &target_pos) {
                if let Some(path) = self.plan_path(start, alt) {
                    self.commit_path(path);
                    return;
                }
            }
            tracing::warn!("nav path not found from {} to {}", start, target);
            self.current_path.clear();
            self.current_waypoint = None;
        }
    }

    /// Build the effective blacklist set for A* (union of giveup + ledge lists).
    fn blacklist_set(&self) -> HashSet<usize> {
        self.waypoint_blacklist
            .iter()
            .chain(self.ledge_blacklist.iter())
            .copied()
            .collect()
    }

    /// Plan a path `start → target`, applying the risk overlay (Plan 08 T3) with
    /// a desperate fallback: weighted A\* first; if that yields no path or a
    /// degenerate (absurdly long) detour — the whole region is hot — re-query
    /// with the overlay dropped ("desperate re-query with W_d=0").
    /// Also applies the waypoint blacklist to avoid repeatedly-stuck nodes.
    fn plan_path(&self, start: usize, target: usize) -> Option<Vec<usize>> {
        let bl = self.blacklist_set();
        // No overlay → blacklist-only A* (with edge blacklist applied).
        let Some(overlay) = self.risk_overlay.as_deref() else {
            return self
                .nav_graph
                .path_excluding_edges(start, target, &bl, &self.edge_blacklist);
        };
        // Weighted path with blacklist penalty already embedded in the overlay.
        // Build a combined overlay: overlay[n] + PENALTY for blacklisted nodes.
        let path = if bl.is_empty() {
            self.nav_graph.path_weighted(start, target, overlay)?
        } else {
            const PENALTY: f32 = 1_000_000.0;
            let combined: Vec<f32> = overlay
                .iter()
                .enumerate()
                .map(|(i, &v)| if bl.contains(&i) { v + PENALTY } else { v })
                .collect();
            self.nav_graph.path_weighted(start, target, &combined)?
        };
        // Degeneracy guard.
        let straight = Vec3::from(self.nav_graph.nodes[start])
            .distance(Vec3::from(self.nav_graph.nodes[target]));
        if straight > DEGEN_MIN_STRAIGHT {
            let len = self.nav_graph.path_len(&path);
            if len > straight * DEGEN_FACTOR {
                tracing::debug!(
                    straight,
                    len,
                    "risk-weighted path degenerate; retrying unweighted"
                );
                return self.nav_graph.path_excluding(start, target, &bl);
            }
        }
        tracing::debug!("nav path found (weighted): {} nodes", path.len());
        Some(path)
    }

    /// Store the path and set the first *meaningful* waypoint (skip the start node,
    /// which is where the bot already is).
    fn commit_path(&mut self, path: Vec<usize>) {
        let coords: Vec<[i32; 3]> = path
            .iter()
            .take(6)
            .map(|&n| {
                let p = self.nav_graph.nodes[n];
                [p[0] as i32, p[1] as i32, p[2] as i32]
            })
            .collect();
        tracing::debug!(
            len = path.len(),
            nodes = ?&path[..path.len().min(6)],
            coords = ?coords,
            "commit_path"
        );
        self.current_path = path;
        // path[0] == start (our position); skip it so we aim at the next node.
        let first = if self.current_path.len() > 1 {
            self.current_path[1]
        } else {
            // Single-node path: already at goal.
            self.current_path[0]
        };
        self.current_waypoint = Some(first);
        self.goal_age_ticks = 0;
        self.goal_abandoned = false;
    }

    pub fn current_waypoint(&self) -> Option<usize> {
        self.current_waypoint
    }

    /// True when the current path edge (prev_waypoint → current_waypoint) is a
    /// jump link (Plan 14 T2). Call this after `update()` to decide whether to
    /// press jump. Safe to call always — returns `false` when no path is active.
    pub fn current_edge_is_jump(&self) -> bool {
        match (self.prev_waypoint, self.current_waypoint) {
            (Some(from), Some(to)) => {
                matches!(self.nav_graph.edge_kind(from, to), EdgeKind::Jump { .. })
            }
            _ => false,
        }
    }

    pub fn next_waypoint_direction(&self, from_position: Vec3) -> Option<Vec3> {
        self.current_waypoint.and_then(|wp_idx| {
            let wp_pos = Vec3::from(self.nav_graph.nodes[wp_idx]);
            let delta = wp_pos - from_position;
            if delta.length_squared() < 1e-6 {
                return None; // avoid NaN from normalizing a zero vector
            }
            Some(delta.normalize())
        })
    }

    /// Position of path node at `current_path[i]`.
    fn path_pos(&self, i: usize) -> Vec3 {
        Vec3::from(self.nav_graph.nodes[self.current_path[i]])
    }

    /// Project `from` onto the path polyline, searching segments from `start_seg` forward,
    /// and return `(segment_index, t)` of the closest point (`t` in `[0,1]` along the
    /// segment). Forward-only search prevents snapping back to an already-passed or looped
    /// segment. This is the bot's true progress along the path, independent of node spacing.
    fn project_onto_path(&self, from: Vec3, start_seg: usize) -> (usize, f32) {
        let n = self.current_path.len();
        if n < 2 {
            return (0, 0.0);
        }
        let mut best = (start_seg.min(n - 2), 0.0f32, f32::MAX);
        for seg in start_seg.min(n - 2)..n - 1 {
            let a = self.path_pos(seg);
            let b = self.path_pos(seg + 1);
            let ab = b - a;
            let len2 = ab.length_squared();
            let t = if len2 > 1e-6 {
                ((from - a).dot(ab) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let d2 = (from - (a + ab * t)).length_squared();
            if d2 < best.2 {
                best = (seg, t, d2);
            }
        }
        (best.0, best.1)
    }

    /// The point `dist` units ahead along the polyline starting from `(seg, t)`.
    fn point_ahead(&self, seg: usize, t: f32, dist: f32) -> Vec3 {
        let n = self.current_path.len();
        let mut cur = {
            let a = self.path_pos(seg);
            let b = self.path_pos(seg + 1);
            a + (b - a) * t
        };
        let mut remaining = dist;
        for s in seg..n - 1 {
            let b = self.path_pos(s + 1);
            let seg_len = (b - cur).length();
            if seg_len >= remaining {
                return cur + (b - cur) * (remaining / seg_len);
            }
            remaining -= seg_len;
            cur = b;
        }
        self.path_pos(n - 1)
    }

    /// Pure-pursuit look-ahead target: project `from` onto the path polyline, then return a
    /// point `LOOKAHEAD` units ahead along the polyline from that projection. Because both
    /// the projection and the look-ahead are geometric, the bot follows the SAME line whether
    /// the path is sampled at 24u or 12u — i.e. steering is density-independent (the fix for
    /// "more nodes → jaggier motion"). Falls back to the final node when the path is short.
    pub fn pursue_target(&self, from: Vec3) -> Option<Vec3> {
        let wp_idx = self.current_waypoint?;
        let n = self.current_path.len();
        if n == 0 {
            return None;
        }
        if n == 1 {
            return Some(self.path_pos(0));
        }
        let wi = self.current_path.iter().position(|&w| w == wp_idx)?;
        // The bot is on/near the segment that ENDS at the current waypoint, so start the
        // projection search one segment back (clamped) to catch it.
        let start_seg = wi.saturating_sub(1);
        let (seg, t) = self.project_onto_path(from, start_seg);
        Some(self.point_ahead(seg, t, LOOKAHEAD))
    }

    /// Corner-cut-safe pursuit target. The raw `pursue_target` interpolates a point
    /// `LOOKAHEAD` units ahead **as a straight line through the path nodes**, which can
    /// cut across an inside corner (the bot's hull clips the wall) or across an open gap
    /// (the bot walks off into a fall). This validates the straight line from `from` to
    /// that point with a hull trace (walls) plus a floor-continuity probe (gaps); if
    /// either fails it falls back to the current waypoint node — a graph node that is
    /// hull- and floor-valid by construction, so steering at it never cuts a corner.
    pub fn pursue_target_safe(&self, from: Vec3, cm: &CollisionModel) -> Option<Vec3> {
        let raw = self.pursue_target(from)?;
        let a = [from.x, from.y, from.z];
        let b = [raw.x, raw.y, raw.z];
        let t = cm.trace(&a, &b, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        let wall_blocked = t.startsolid || t.fraction < 1.0;
        if !wall_blocked && segment_has_floor(cm, a, b) {
            return Some(raw);
        }
        // Unsafe straight line — steer at the next graph node instead.
        let wp = self.current_waypoint?;
        Some(Vec3::from(self.nav_graph.nodes[wp]))
    }

    pub fn update(&mut self, position: Vec3, cm: Option<&CollisionModel>) -> bool {
        self.goal_abandoned = false;

        if let Some(wp_idx) = self.current_waypoint {
            let wp_pos = Vec3::from(self.nav_graph.nodes[wp_idx]);
            let delta_xy = (wp_pos - position).truncate();
            let horiz = delta_xy.length();
            let dz = (wp_pos.z - position.z).abs();

            // Z-aware reach: looser than plain 3D distance so steps and ramps
            // don't cause the bot to orbit a node it's already passed laterally.
            // Eraser uses horiz < 12 && |dz| < 16; we relax for the 64-unit graph.
            let reached = horiz < WP_REACH_HORIZ && dz < WP_REACH_DZ;

            // Orbit watchdog: if the bot circles within ORBIT_RADIUS without
            // reaching, force-advance after ORBIT_FRAMES ticks.
            // Reset goal_age_ticks only after 3+ consecutive ticks inside orbit
            // range — prevents a brief orbit-boundary dip (horiz oscillating
            // 47↔52 u) from continuously resetting the giveup timer and making
            // it impossible to ever give up on the stuck waypoint.
            const ORBIT_ENTRY_MIN: u32 = 3;
            let orbit_force = if horiz < ORBIT_RADIUS {
                self.near_wp_ticks += 1;
                if self.near_wp_ticks >= ORBIT_ENTRY_MIN {
                    self.goal_age_ticks = 0; // sustained orbit entry: orbit owns this
                }
                self.near_wp_ticks >= ORBIT_FRAMES
            } else {
                self.near_wp_ticks = 0;
                false
            };

            if reached || orbit_force {
                if orbit_force && !reached {
                    // When the bot is close horizontally but far from the waypoint
                    // vertically (dz > 4×WP_REACH_DZ = 96u), two distinct cases apply.
                    const LEDGE_DZ: f32 = WP_REACH_DZ * 4.0; // 96 u threshold

                    // If the bot has climbed significantly ABOVE the waypoint (e.g. rode
                    // a slope onto the roof) force an immediate replan: force-advancing
                    // to the next node would send the bot in the wrong direction.
                    // Use a tighter threshold than LEDGE_DZ (96u): even 48-82u above
                    // the waypoint z indicates the bot is on a slope/roof, not at the node.
                    let wp_z = self.nav_graph.nodes[wp_idx][2];
                    if position.z > wp_z + WP_REACH_DZ * 2.0 {
                        tracing::debug!(
                            bot_z = position.z as i32,
                            wp_z = wp_z as i32,
                            waypoint = wp_idx,
                            "orbit-timeout: bot above waypoint — replanning"
                        );
                        self.near_wp_ticks = 0;
                        self.current_path.clear();
                        self.current_waypoint = None;
                        self.last_goal_node = None;
                        self.goal_age_ticks = 0;
                        self.goal_abandoned = true;
                        return false;
                    }

                    if dz > LEDGE_DZ {
                        let current_idx_opt = self.current_path.iter().position(|&w| w == wp_idx);
                        let prev_in_path = current_idx_opt.and_then(|i| {
                            if i > 0 {
                                Some(self.current_path[i - 1])
                            } else {
                                None
                            }
                        });
                        let wp_coords = self.nav_graph.nodes[wp_idx];
                        let prev_coords = prev_in_path.map(|p| self.nav_graph.nodes[p]);

                        // Discriminate: false-bridge (edge goes sharply upward from
                        // prev → wp) vs. fell-off-ledge (prev and wp at the same Z,
                        // the bot fell away from the platform while navigating it).
                        //
                        // edge_dz = |prev_z − wp_z|:
                        //   > LEDGE_DZ → false-bridge target (ascending walk edge
                        //     through open staircase air) → blacklist & replan.
                        //   ≤ LEDGE_DZ → fell-off-ledge → skip forward in path to the
                        //     first node near the bot's current Z level so the bot
                        //     routes around the dangerous platform segment.
                        let edge_dz =
                            prev_coords.map_or(LEDGE_DZ + 1.0, |pc| (pc[2] - wp_coords[2]).abs());

                        if edge_dz > LEDGE_DZ {
                            // FALSE BRIDGE: ascending through open staircase air.
                            tracing::debug!(
                                horiz, dz, edge_dz,
                                waypoint = wp_idx,
                                prev_in_path = ?prev_in_path,
                                wp_x = wp_coords[0] as i32,
                                wp_y = wp_coords[1] as i32,
                                wp_z = wp_coords[2] as i32,
                                "orbit-timeout: false bridge — blacklisting and replanning"
                            );
                            if !self.ledge_blacklist.contains(&wp_idx) {
                                self.ledge_blacklist.push_back(wp_idx);
                                if self.ledge_blacklist.len() > LEDGE_BLACKLIST_MAX {
                                    self.ledge_blacklist.pop_front();
                                }
                            }
                            self.near_wp_ticks = 0;
                            self.current_path.clear();
                            self.current_waypoint = None;
                            self.last_goal_node = None;
                            self.goal_age_ticks = 0;
                            self.goal_abandoned = true;
                            return false;
                        } else {
                            // FELL-OFF-LEDGE: bot was navigating at wp's floor level
                            // and fell. Blacklisting wp (a real node) would cut off
                            // the route. Instead, skip forward in the path to the
                            // first waypoint near the bot's current Z level so the bot
                            // navigates around the dangerous platform section.
                            let bot_z = position.z;
                            let skip_idx = current_idx_opt.and_then(|cur| {
                                self.current_path[cur + 1..]
                                    .iter()
                                    .position(|&n| {
                                        (self.nav_graph.nodes[n][2] - bot_z).abs()
                                            <= WP_REACH_DZ * 3.0
                                    })
                                    .map(|off| cur + 1 + off)
                            });
                            tracing::debug!(
                                horiz, dz, edge_dz,
                                waypoint = wp_idx,
                                wp_x = wp_coords[0] as i32,
                                wp_y = wp_coords[1] as i32,
                                wp_z = wp_coords[2] as i32,
                                bot_x = position.x as i32,
                                bot_y = position.y as i32,
                                bot_z = position.z as i32,
                                skip_to = ?skip_idx.map(|i| self.current_path[i]),
                                "orbit-timeout: fell-off-ledge — skipping to same-floor node"
                            );
                            // Blacklist the specific EDGE (prev → wp) that caused the
                            // fall, not the destination node. This lets A* still reach
                            // wp_idx via other incoming edges (different approach angles)
                            // while avoiding the exact dangerous staircase approach.
                            // Only fall back to node-blacklisting when prev is unknown
                            // (bot fell from the very first path node with no predecessor).
                            if let Some(prev_idx) = prev_in_path {
                                self.edge_blacklist.insert((prev_idx, wp_idx));
                            } else if !self.ledge_blacklist.contains(&wp_idx) {
                                self.ledge_blacklist.push_back(wp_idx);
                                if self.ledge_blacklist.len() > LEDGE_BLACKLIST_MAX {
                                    self.ledge_blacklist.pop_front();
                                }
                            }
                            self.near_wp_ticks = 0;
                            if let Some(si) = skip_idx {
                                self.current_waypoint = Some(self.current_path[si]);
                            } else {
                                // No same-floor node ahead: force replan so A* finds
                                // a route that avoids the now-blacklisted fell-from node.
                                self.current_path.clear();
                                self.current_waypoint = None;
                                self.last_goal_node = None;
                                self.goal_age_ticks = 0;
                                self.goal_abandoned = true;
                                return false;
                            }
                            return false;
                        }
                    }
                    // Flat-wall check: if the waypoint is blocked by solid geometry
                    // (same floor level), replanning finds a better route rather than
                    // force-advancing into an unreachable node.
                    if let Some(cm) = cm {
                        let wp_pos = self.nav_graph.nodes[wp_idx];
                        let bot_pos = [position.x, position.y, position.z];
                        let t = cm.trace(&bot_pos, &wp_pos, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
                        if !t.startsolid && t.fraction < 0.9 {
                            tracing::debug!(
                                horiz,
                                dz,
                                fraction = t.fraction,
                                waypoint = wp_idx,
                                "orbit-timeout: wall blocks waypoint — replanning"
                            );
                            self.near_wp_ticks = 0;
                            if !self.waypoint_blacklist.contains(&wp_idx) {
                                self.waypoint_blacklist.push_back(wp_idx);
                                if self.waypoint_blacklist.len() > GIVEUP_BLACKLIST_MAX {
                                    self.waypoint_blacklist.pop_front();
                                }
                            }
                            self.current_path.clear();
                            self.current_waypoint = None;
                            self.last_goal_node = None;
                            self.goal_age_ticks = 0;
                            self.goal_abandoned = true;
                            return false;
                        }
                    }
                    tracing::debug!(
                        horiz,
                        dz,
                        near_wp_ticks = self.near_wp_ticks,
                        "orbit-timeout: force-advancing past waypoint"
                    );
                }
                self.near_wp_ticks = 0;
                // Reached a waypoint — reset the give-up clock and advance.
                self.goal_age_ticks = 0;
                let current_idx = self.current_path.iter().position(|&w| w == wp_idx);
                if let Some(idx) = current_idx {
                    if idx + 1 < self.current_path.len() {
                        let next_wp = self.current_path[idx + 1];
                        let nw = self.nav_graph.nodes[next_wp];
                        tracing::trace!(
                            from = wp_idx,
                            to = next_wp,
                            to_pos = ?[nw[0] as i32, nw[1] as i32, nw[2] as i32],
                            "wp advance"
                        );
                        self.prev_waypoint = Some(wp_idx);
                        self.current_waypoint = Some(next_wp);
                    } else {
                        // Reached the goal; clear both blacklists.
                        self.waypoint_blacklist.clear();
                        self.ledge_blacklist.clear();
                        self.prev_waypoint = None;
                        self.current_waypoint = None;
                        self.last_goal_node = None;
                        return true;
                    }
                }
            } else {
                // Still pursuing — age the goal toward the give-up cap.
                self.goal_age_ticks += 1;
                if self.goal_age_ticks > GOAL_GIVEUP_TICKS {
                    // Blacklist the stuck waypoint so the next plan routes around it.
                    tracing::debug!(
                        age = self.goal_age_ticks,
                        waypoint = wp_idx,
                        "goal give-up: blacklisting waypoint and replanning"
                    );
                    self.waypoint_blacklist.push_back(wp_idx);
                    if self.waypoint_blacklist.len() > GIVEUP_BLACKLIST_MAX {
                        self.waypoint_blacklist.pop_front();
                    }
                    // Force a fresh plan (excluding the blacklisted node).
                    self.current_path.clear();
                    self.current_waypoint = None;
                    self.last_goal_node = None;
                    self.goal_age_ticks = 0;
                    self.near_wp_ticks = 0;
                    self.goal_abandoned = true;
                }
            }
        }

        self.last_position = Some(position);

        false
    }

    /// True for one tick after the give-up watchdog abandoned a stale goal. The
    /// caller can use this to fall back to roaming instead of re-issuing the same
    /// (unreachable) goal.
    pub fn goal_abandoned(&self) -> bool {
        self.goal_abandoned
    }

    /// Force the next `set_goal` to replan from scratch, even if the goal is
    /// unchanged. Call after clearing an obstacle so the bot doesn't re-attempt
    /// the same wedged waypoint. Does NOT clear the blacklist — a stuck waypoint
    /// that caused give-up should stay blacklisted until the goal is reached.
    pub fn force_replan(&mut self) {
        self.current_path.clear();
        self.current_waypoint = None;
        self.last_goal_node = None;
    }

    /// Trace from `position` to the current waypoint. If solid blocks the hull
    /// path, blacklist the waypoint so the next A* plan routes around the false
    /// edge. Call this before `force_replan` on BackOffThenRepath to avoid
    /// blacklisting waypoints that are reachable via a different approach angle.
    pub fn blacklist_waypoint_if_blocked(&mut self, position: Vec3, cm: &CollisionModel) {
        let Some(wp_idx) = self.current_waypoint else {
            return;
        };
        let wp_pos = self.nav_graph.nodes[wp_idx];
        let bot_pos = [position.x, position.y, position.z];
        let t = cm.trace(&bot_pos, &wp_pos, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        if !t.startsolid && t.fraction < 0.9 {
            tracing::debug!(
                waypoint = wp_idx,
                fraction = t.fraction,
                "BackOff: wall blocks waypoint — blacklisting"
            );
            if !self.waypoint_blacklist.contains(&wp_idx) {
                self.waypoint_blacklist.push_back(wp_idx);
                if self.waypoint_blacklist.len() > GIVEUP_BLACKLIST_MAX {
                    self.waypoint_blacklist.pop_front();
                }
            }
        }
    }

    pub fn path_length(&self) -> usize {
        self.current_path.len()
    }

    /// Summed base edge cost of the current path (units), or `None` when no path is active.
    /// Used by `hybrid-race` to score this backend against the navmesh's `planned_len`.
    pub fn planned_cost(&self) -> Option<f32> {
        if self.current_path.len() < 2 {
            return None;
        }
        Some(self.nav_graph.path_len(&self.current_path))
    }

    /// Number of jump-link edges on the current path. `hybrid-race` penalizes jumps (they
    /// are riskier than walking); `hybrid-segment` uses jump presence to hand a segment to A*.
    pub fn planned_jump_count(&self) -> usize {
        self.current_path
            .windows(2)
            .filter(|w| matches!(self.nav_graph.edge_kind(w[0], w[1]), EdgeKind::Jump { .. }))
            .count()
    }

    /// String-pull the current path using `cm` so the bot cuts corners instead of
    /// zigzagging at every 64-unit grid node. Call once after `set_goal` replans.
    ///
    /// Safe to call every tick — it only re-smoothes when the path has ≥3 nodes
    /// and the smoothed version is shorter. Plan 14 T1.
    pub fn smooth_with_cm(&mut self, cm: &CollisionModel, from: Vec3) {
        if self.current_path.len() < 3 {
            return;
        }
        let smoothed = self
            .nav_graph
            .smooth_path(cm, &self.current_path, [from.x, from.y, from.z]);
        if smoothed.len() < self.current_path.len() {
            let old_wp = self.current_waypoint;
            let old_len = self.current_path.len();
            self.current_path = smoothed;
            // Re-anchor the current waypoint to the first valid node in the new path.
            let new_wp = if self.current_path.len() > 1 {
                // Keep the old waypoint if it's still in the smoothed path; else use path[1].
                if old_wp.is_some_and(|w| self.current_path.contains(&w)) {
                    old_wp
                } else {
                    Some(self.current_path[1])
                }
            } else {
                self.current_path.first().copied()
            };
            let nw = new_wp.map(|n| {
                let p = self.nav_graph.nodes[n];
                [p[0] as i32, p[1] as i32, p[2] as i32]
            });
            tracing::debug!(
                old_len,
                new_len = self.current_path.len(),
                new_wp = ?new_wp,
                new_wp_pos = ?nw,
                "smooth_with_cm: path shortened"
            );
            self.current_waypoint = new_wp;
        }
    }
}

/// The waypoint-graph (`astar`) backend's implementation of the shared [`Navigator`]
/// contract — delegates to the inherent methods so the scenario tick loop can drive it
/// interchangeably with the navmesh backend. `current_waypoint_pos` resolves the node
/// index through the graph so the loop never reaches into `NavGraph` directly.
impl crate::nav_mode::Navigator for NavigationDriver {
    fn set_goal(&mut self, goal: NavGoal, from: Vec3) {
        NavigationDriver::set_goal(self, goal, from)
    }
    fn update(&mut self, pos: Vec3, cm: Option<&CollisionModel>) -> bool {
        NavigationDriver::update(self, pos, cm)
    }
    fn pursue_target(&self, from: Vec3) -> Option<Vec3> {
        NavigationDriver::pursue_target(self, from)
    }
    fn pursue_target_safe(&self, from: Vec3, cm: &CollisionModel) -> Option<Vec3> {
        NavigationDriver::pursue_target_safe(self, from, cm)
    }
    fn current_edge_is_jump(&self) -> bool {
        NavigationDriver::current_edge_is_jump(self)
    }
    fn force_replan(&mut self) {
        NavigationDriver::force_replan(self)
    }
    fn blacklist_waypoint_if_blocked(&mut self, pos: Vec3, cm: &CollisionModel) {
        NavigationDriver::blacklist_waypoint_if_blocked(self, pos, cm)
    }
    fn current_waypoint(&self) -> Option<usize> {
        NavigationDriver::current_waypoint(self)
    }
    fn current_waypoint_pos(&self) -> Option<[f32; 3]> {
        NavigationDriver::current_waypoint(self).map(|i| self.nav_graph.nodes[i])
    }
    fn smooth_with_cm(&mut self, cm: &CollisionModel, from: Vec3) {
        NavigationDriver::smooth_with_cm(self, cm, from)
    }
    fn goal_abandoned(&self) -> bool {
        NavigationDriver::goal_abandoned(self)
    }
    fn set_risk_overlay(&mut self, overlay: Vec<f32>) {
        NavigationDriver::set_risk_overlay(self, overlay)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StuckAction {
    Jump,
    BackOff,
    Repath,
    None,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stuck_action_variants() {
        let action = StuckAction::Jump;
        assert_eq!(action, StuckAction::Jump);
    }

    #[test]
    fn planned_cost_and_jump_count_reflect_current_path() {
        use std::sync::Arc;
        // Chain A(0)→B(1)→C(2) at 100u spacing; B→C is a jump link.
        let g = Arc::new(NavGraph::from_raw_with_jumps(
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [200.0, 0.0, 0.0]],
            vec![
                vec![(1, 100.0)],
                vec![(0, 100.0), (2, 100.0)],
                vec![(1, 100.0)],
            ],
            vec![(1, 2, 90.0)],
        ));
        let mut nav = NavigationDriver::new(Arc::clone(&g));
        // No path yet.
        assert_eq!(nav.planned_cost(), None);
        assert_eq!(nav.planned_jump_count(), 0);
        // Plan A→C: cost ~200u, one jump edge (B→C).
        nav.set_goal(NavGoal::Waypoint(2), Vec3::new(0.0, 0.0, 0.0));
        let cost = nav.planned_cost().expect("path planned");
        assert!((cost - 200.0).abs() < 1e-3, "cost={cost}");
        assert_eq!(nav.planned_jump_count(), 1);
    }

    #[test]
    fn risk_overlay_detours_driver_around_danger() {
        use std::sync::Arc;
        // Diamond: 0=A 1=B 2=C 3=D. A→C direct via B (200), longer via D (282).
        let g = Arc::new(NavGraph::from_raw(
            vec![
                [0.0, 0.0, 0.0],
                [0.0, 100.0, 0.0],
                [0.0, 200.0, 0.0],
                [100.0, 100.0, 0.0],
            ],
            vec![
                vec![(1, 100.0), (3, 141.0)],
                vec![(0, 100.0), (2, 100.0)],
                vec![(1, 100.0), (3, 141.0)],
                vec![(0, 141.0), (2, 141.0)],
            ],
        ));
        let mut nav = NavigationDriver::new(Arc::clone(&g));

        // No overlay → A→C routes via B (waypoint 1).
        nav.set_goal(NavGoal::Waypoint(2), Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(nav.current_waypoint(), Some(1), "unweighted picks B");

        // Danger at B → replan routes via D (waypoint 3).
        nav.set_risk_overlay(vec![0.0, 1000.0, 0.0, 0.0]);
        nav.force_replan();
        nav.set_goal(NavGoal::Waypoint(2), Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(nav.current_waypoint(), Some(3), "danger at B detours via D");

        // Dropping the overlay restores the direct route.
        nav.clear_risk_overlay();
        nav.force_replan();
        nav.set_goal(NavGoal::Waypoint(2), Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(
            nav.current_waypoint(),
            Some(1),
            "cleared overlay returns to B"
        );
    }
}
