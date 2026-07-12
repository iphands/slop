//! `XonNavDriver` — the Xonotic goal-stack **navmode** (`--navmode xg`; Plan 61).
//!
//! Ports the *navigation-layer* half of havocbot as a wrapping [`Navigator`] over the proven
//! A* [`NavigationDriver`] (the Plan 20 "delegate, not rewrite" pattern), so ANY brain can
//! run with Xonotic's route texture (brain × navmode are orthogonal axes, Plan 25):
//!
//! - **Travel-time costs** (`waypoint_getlinearcost`, `waypoints.qc:1010-1060`): swim nodes
//!   are slower to leave (the vendor's underwater `/(maxspeed*0.7)`), expressed as a static
//!   per-node additive overlay. (Fall-time pricing of jump-downs needs an edge-kind-aware
//!   weighted API — deferred, documented.)
//! - **Danger field** (`botframe_updatedangerousobjects`, `navigation.qc:1874-1906`): live
//!   PVS threats push per-node additive cost `max(0, rating − dist)`, refreshed every
//!   0.25 s and decayed when sources go unseen — routes detour around rocket lines and
//!   contested ground. Fed via [`Navigator::note_dangers`] (defaulted no-op elsewhere).
//! - **Shorten-path chase cutover** (`navigation.qc:1555`): a goal within 700 u with a
//!   hull-clear, hazard-free straight line short-circuits the polyline.
//! - **Goal-progress watchdog** (`havocbot_checkgoaldistance`, `havocbot.qc:344-368`):
//!   no 2D/Z progress for 0.5 s → force a replan; a second consecutive stall surfaces
//!   [`Navigator::goal_abandoned`] so the brain re-goals.
//!
//! **Overlay composition**: the externally-set heatmap overlay ([`Navigator::set_risk_overlay`],
//! Plan 08) is SUMMED with the static travel-time + live danger overlays — never overwritten.
//! Everything here is runtime pricing: no mapcache impact.

use glam::Vec3;
use std::sync::Arc;
use world::{CollisionModel, NavGraph, HULL_MAXS, HULL_MINS, MASK_SOLID};

use crate::nav::{NavGoal, NavigationDriver};
use crate::nav_mode::{DangerSource, Navigator};

/// Extra cost (qu-equivalent) to leave a water node — the vendor's swim slowdown
/// (`dist/(maxspeed*0.7)` ⇒ ×1.43 time) expressed additively against ~28 u average edges.
const SWIM_NODE_PENALTY: f32 = 12.0;
/// Danger refresh cadence (`bot_ai_dangerdetectioninterval`), seconds.
const DANGER_REFRESH: f32 = 0.25;
/// A pushed danger source is trusted this long without a re-push (PVS honesty), seconds.
const SOURCE_TTL: f32 = 0.5;
/// Re-plan when the total danger mass changes by more than this since the last replan
/// (avoids replanning every refresh while the field is stable).
const REPLAN_DANGER_DELTA: f32 = 200.0;
/// `MAX_CHASE_DISTANCE` (`navigation.qc:55`): cutover range.
const CUTOVER_DIST: f32 = 700.0;
/// Max height difference for a cutover (a straight walk can climb a step, not a ledge).
const CUTOVER_MAX_DZ: f32 = 32.0;
/// Watchdog stall window (seconds) + progress epsilon (qu).
const WATCHDOG_STALL: f32 = 0.5;
const PROGRESS_EPS: f32 = 8.0;

/// The Xonotic-texture navigator: A* inside, travel-time + danger pricing, route repair.
pub struct XonNavDriver {
    inner: NavigationDriver,
    graph: Arc<NavGraph>,
    /// Static travel-time overlay (water nodes; computed once at build).
    static_overlay: Vec<f32>,
    /// Live danger field (rebuilt every [`DANGER_REFRESH`]).
    danger: Vec<f32>,
    /// External (heatmap) overlay — composed by sum, never overwritten.
    external: Option<Vec<f32>>,
    /// Latest pushed sources, stamped with the driver clock.
    sources: Vec<(DangerSource, f32)>,
    /// Driver clock (advanced by `update` dt-less calls — we accumulate from watchdog dt).
    now: f32,
    next_danger_refresh: f32,
    /// Total danger mass at the last (re)plan — the replan-trigger baseline.
    danger_at_plan: f32,
    /// Current final goal (for cutover + watchdog).
    goal_pos: Option<Vec3>,
    /// Cutover engaged this frame (pursue returns the goal directly).
    cutover: bool,
    /// Watchdog state.
    best_2d: f32,
    best_z: f32,
    stall_secs: f32,
    strikes: u32,
    abandoned: bool,
    /// Last `update` call time (dt derivation; the Navigator trait has no dt input).
    last_update: Option<std::time::Instant>,
}

impl XonNavDriver {
    pub fn new(graph: Arc<NavGraph>) -> Self {
        let static_overlay: Vec<f32> = (0..graph.node_count())
            .map(|i| {
                if graph.is_water_node(i) {
                    SWIM_NODE_PENALTY
                } else {
                    0.0
                }
            })
            .collect();
        let n = graph.node_count();
        let mut s = Self {
            inner: NavigationDriver::new(Arc::clone(&graph)),
            graph,
            static_overlay,
            danger: vec![0.0; n],
            external: None,
            sources: Vec::new(),
            now: 0.0,
            next_danger_refresh: 0.0,
            danger_at_plan: 0.0,
            goal_pos: None,
            cutover: false,
            best_2d: f32::INFINITY,
            best_z: f32::INFINITY,
            stall_secs: 0.0,
            strikes: 0,
            abandoned: false,
            last_update: None,
        };
        s.push_overlay();
        s
    }

    /// Compose static + danger + external and hand the sum to the inner A* driver.
    fn push_overlay(&mut self) {
        let mut combined = self.static_overlay.clone();
        for (i, c) in combined.iter_mut().enumerate() {
            *c += self.danger.get(i).copied().unwrap_or(0.0);
            if let Some(ext) = &self.external {
                *c += ext.get(i).copied().unwrap_or(0.0);
            }
        }
        self.inner.set_risk_overlay(combined);
    }

    /// Rebuild the danger field from the live sources (`navigation.qc:1874-1906` shape:
    /// additive `max(0, rating − dist)` per node within each source's rating radius).
    fn refresh_danger(&mut self) {
        let now = self.now;
        self.sources.retain(|&(_, at)| now - at < SOURCE_TTL);
        for d in self.danger.iter_mut() {
            *d = 0.0;
        }
        for &(src, _) in &self.sources {
            // Radius-capped: only nodes inside `rating` gain cost. Linear falloff.
            for i in 0..self.graph.node_count() {
                let p = Vec3::from(self.graph.node_pos(i));
                let dist = (p - src.pos).length();
                if dist < src.rating {
                    self.danger[i] += src.rating - dist;
                }
            }
        }
        let mass: f32 = self.danger.iter().sum();
        self.push_overlay();
        // Evidence-triggered replan: the field shifted enough that the committed
        // polyline may now cross hot ground.
        if (mass - self.danger_at_plan).abs() > REPLAN_DANGER_DELTA {
            self.danger_at_plan = mass;
            self.inner.force_replan();
        }
    }

    /// Is the straight line `from → goal` walkable enough to cut the polyline: hull-clear,
    /// near-level, and not crossing deadly ground (`tracewalk`-lite; Plan 48 probe on the
    /// segment midpoint direction).
    fn cutover_ok(&self, from: Vec3, goal: Vec3, cm: &CollisionModel) -> bool {
        let d = goal - from;
        if d.truncate().length() > CUTOVER_DIST || d.z.abs() > CUTOVER_MAX_DZ {
            return false;
        }
        // Chest-height hull trace (+32: clear of the floor the hull's -24 z-min would
        // otherwise start inside; walls still block, drops/lava are the probe's job below).
        let t = cm.trace(
            &[from.x, from.y, from.z + 32.0],
            &[goal.x, goal.y, goal.z + 32.0],
            &HULL_MINS,
            &HULL_MAXS,
            MASK_SOLID,
        );
        if t.fraction < 1.0 || t.startsolid {
            return false;
        }
        // A clear hull trace can still cross a lava gap — veto hazardous directions.
        !crate::hazard::dir_is_hazardous(cm, from, d)
    }

    /// Watchdog: track best-ever 2D/Z distance to the goal; stall 0.5 s → replan; twice →
    /// surface `goal_abandoned` for one tick.
    fn watchdog(&mut self, pos: Vec3, dt: f32) {
        let Some(goal) = self.goal_pos else { return };
        let d2 = (goal - pos).truncate().length();
        let dz = (goal.z - pos.z).abs();
        let mut progress = false;
        if d2 < self.best_2d - PROGRESS_EPS {
            self.best_2d = d2;
            progress = true;
        }
        if dz < self.best_z - PROGRESS_EPS {
            self.best_z = dz;
            progress = true;
        }
        if progress {
            self.stall_secs = 0.0;
            self.strikes = 0;
            return;
        }
        self.stall_secs += dt;
        if self.stall_secs >= WATCHDOG_STALL {
            self.stall_secs = 0.0;
            self.strikes += 1;
            if self.strikes >= 2 {
                self.abandoned = true;
                self.strikes = 0;
                tracing::debug!("xg watchdog: goal abandoned (no progress twice)");
            } else {
                self.inner.force_replan();
                tracing::debug!("xg watchdog: stall replan");
            }
        }
    }

    fn reset_watchdog(&mut self, pos: Vec3, goal: Vec3) {
        self.best_2d = (goal - pos).truncate().length();
        self.best_z = (goal.z - pos.z).abs();
        self.stall_secs = 0.0;
        self.strikes = 0;
    }

    /// Test-only view of the composed per-node overlay currently in force.
    #[cfg(test)]
    fn effective_overlay(&self) -> Vec<f32> {
        let mut combined = self.static_overlay.clone();
        for (i, c) in combined.iter_mut().enumerate() {
            *c += self.danger.get(i).copied().unwrap_or(0.0);
            if let Some(ext) = &self.external {
                *c += ext.get(i).copied().unwrap_or(0.0);
            }
        }
        combined
    }
}

impl Navigator for XonNavDriver {
    fn set_goal(&mut self, goal: NavGoal, from: Vec3) {
        let pos = match goal {
            NavGoal::Position(p) | NavGoal::Entity(p) => Some(p),
            NavGoal::Waypoint(w) => {
                (w < self.graph.node_count()).then(|| Vec3::from(self.graph.node_pos(w)))
            }
        };
        if let Some(p) = pos {
            if self
                .goal_pos
                .map(|g| (g - p).length() > 32.0)
                .unwrap_or(true)
            {
                self.reset_watchdog(from, p);
            }
            self.goal_pos = Some(p);
        }
        self.inner.set_goal(goal, from);
    }

    fn update(&mut self, pos: Vec3, cm: Option<&CollisionModel>) -> bool {
        // Derive dt from wall clock (the trait carries none); clamp to sane tick bounds.
        let dt = self
            .last_update
            .map(|t| t.elapsed().as_secs_f32().clamp(0.0, 0.5))
            .unwrap_or(0.1);
        self.last_update = Some(std::time::Instant::now());
        self.now += dt;
        self.abandoned = false;

        if self.now >= self.next_danger_refresh {
            self.next_danger_refresh = self.now + DANGER_REFRESH;
            if !self.sources.is_empty() || self.danger.iter().any(|&d| d > 0.0) {
                self.refresh_danger();
            }
        }

        // Cutover check (`navigation.qc:1555` 0.25 s cadence folded into the tick).
        self.cutover = match (self.goal_pos, cm) {
            (Some(g), Some(c)) => self.cutover_ok(pos, g, c),
            _ => false,
        };

        self.watchdog(pos, dt);
        self.inner.update(pos, cm)
    }

    fn pursue_target(&self, from: Vec3) -> Option<Vec3> {
        if self.cutover {
            return self.goal_pos;
        }
        self.inner.pursue_target(from)
    }

    fn pursue_target_safe(&self, from: Vec3, cm: &CollisionModel) -> Option<Vec3> {
        if self.cutover {
            return self.goal_pos;
        }
        self.inner.pursue_target_safe(from, cm)
    }

    fn current_edge_is_jump(&self) -> bool {
        !self.cutover && self.inner.current_edge_is_jump()
    }
    fn current_edge_is_swim(&self) -> bool {
        // Swim/ride flags stay live during a cutover — the traversal executor must keep
        // owning water/platform movement (its gates read these).
        self.inner.current_edge_is_swim()
    }
    fn current_edge_is_ride(&self) -> bool {
        self.inner.current_edge_is_ride()
    }
    fn current_ride_info(&self) -> Option<world::RideInfo> {
        self.inner.current_ride_info()
    }
    fn force_replan(&mut self) {
        self.inner.force_replan();
    }
    fn blacklist_waypoint_if_blocked(&mut self, pos: Vec3, cm: &CollisionModel) {
        self.inner.blacklist_waypoint_if_blocked(pos, cm);
    }
    fn current_waypoint(&self) -> Option<usize> {
        self.inner.current_waypoint()
    }
    fn current_waypoint_pos(&self) -> Option<[f32; 3]> {
        self.inner.current_waypoint_pos()
    }
    fn smooth_with_cm(&mut self, cm: &CollisionModel, from: Vec3) {
        self.inner.smooth_with_cm(cm, from);
    }
    /// External heatmap overlay: COMPOSED (summed), never overwritten (Plan 08 seam).
    fn set_risk_overlay(&mut self, overlay: Vec<f32>) {
        self.external = Some(overlay);
        self.push_overlay();
    }
    fn goal_abandoned(&self) -> bool {
        self.abandoned || self.inner.goal_abandoned()
    }
    fn speed_scale(&self, pos: Vec3) -> f32 {
        self.inner.speed_scale(pos)
    }
    fn note_dangers(&mut self, dangers: &[DangerSource]) {
        let now = self.now;
        for &d in dangers {
            self.sources.push((d, now));
        }
        // Cap runaway growth (a busy frame pushes a handful; TTL prunes the rest).
        if self.sources.len() > 256 {
            let drop = self.sources.len() - 256;
            self.sources.drain(..drop);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Diamond: 0→1→3 (cheap) vs 0→2→3 (expensive).
    fn diamond() -> Arc<NavGraph> {
        Arc::new(NavGraph::from_raw(
            vec![
                [0.0, 0.0, 0.0],
                [100.0, 100.0, 0.0],
                [100.0, -100.0, 0.0],
                [200.0, 0.0, 0.0],
            ],
            vec![
                vec![(1, 100.0), (2, 150.0)],
                vec![(0, 100.0), (3, 100.0)],
                vec![(0, 150.0), (3, 150.0)],
                vec![(1, 100.0), (2, 150.0)],
            ],
        ))
    }

    /// Flat world with the floor just under the origin (so hazard probes see ground).
    fn open_cm() -> CollisionModel {
        CollisionModel::half_space([0.0, 0.0, 1.0], -0.25)
    }

    #[test]
    fn passthrough_parity_when_inert() {
        // With no water, no dangers, no external overlay: xg must route exactly like A*.
        let g = diamond();
        let mut xg = XonNavDriver::new(Arc::clone(&g));
        let mut astar = NavigationDriver::new(Arc::clone(&g));
        let from = Vec3::new(0.0, 0.0, 0.0);
        xg.set_goal(NavGoal::Waypoint(3), from);
        astar.set_goal(NavGoal::Waypoint(3), from);
        xg.update(from, None);
        astar.update(from, None);
        assert_eq!(xg.current_waypoint(), astar.current_waypoint());
        assert_eq!(
            xg.pursue_target(from).map(|v| v.to_array()),
            astar.pursue_target(from).map(|v| v.to_array())
        );
    }

    #[test]
    fn danger_source_reroutes_around_the_cheap_route() {
        let g = diamond();
        let mut xg = XonNavDriver::new(Arc::clone(&g));
        let from = Vec3::new(0.0, 0.0, 0.0);
        // A rocket parked on node 1 (the cheap route's middle).
        xg.note_dangers(&[DangerSource {
            pos: Vec3::new(100.0, 100.0, 0.0),
            rating: 300.0,
        }]);
        xg.now = 1.0; // inside the TTL of the push above? push stamped at now=0 → refresh at 0.25
        xg.sources[0].1 = 1.0; // keep the source fresh for the refresh
        xg.refresh_danger();
        assert!(
            xg.effective_overlay()[1] > xg.effective_overlay()[2],
            "node 1 must be hotter than node 2"
        );
        xg.set_goal(NavGoal::Waypoint(3), from);
        xg.inner.update(from, None);
        // The plan must route via node 2 despite the higher base cost.
        assert_eq!(xg.current_waypoint(), Some(2), "detour around the danger");
    }

    #[test]
    fn external_overlay_is_summed_not_overwritten() {
        let g = diamond();
        let mut xg = XonNavDriver::new(Arc::clone(&g));
        xg.note_dangers(&[DangerSource {
            pos: Vec3::new(100.0, 100.0, 0.0),
            rating: 200.0,
        }]);
        xg.sources[0].1 = 0.4;
        xg.now = 0.4;
        xg.refresh_danger();
        let danger_only = xg.effective_overlay()[1];
        assert!(danger_only > 0.0);
        xg.set_risk_overlay(vec![0.0, 50.0, 0.0, 0.0]);
        let both = xg.effective_overlay()[1];
        assert!(
            (both - (danger_only + 50.0)).abs() < 1e-3,
            "sum, not overwrite: {both} vs {danger_only}+50"
        );
    }

    #[test]
    fn cutover_shortcircuits_close_clear_goals() {
        let g = diamond();
        let cm = open_cm();
        let mut xg = XonNavDriver::new(Arc::clone(&g));
        let from = Vec3::new(0.0, 0.0, 0.0);
        let goal = Vec3::new(300.0, 0.0, 0.0); // 300 u, level, open world
        xg.set_goal(NavGoal::Position(goal), from);
        xg.update(from, Some(&cm));
        assert_eq!(
            xg.pursue_target_safe(from, &cm).map(|v| v.to_array()),
            Some(goal.to_array()),
            "close + hull-clear + level → steer straight at the goal"
        );

        // Far goal: no cutover (polyline pursuit).
        let far = Vec3::new(5000.0, 0.0, 0.0);
        xg.set_goal(NavGoal::Position(far), from);
        xg.update(from, Some(&cm));
        assert_ne!(
            xg.pursue_target_safe(from, &cm).map(|v| v.to_array()),
            Some(far.to_array()),
            "700 u cap"
        );
    }

    #[test]
    fn watchdog_abandons_after_two_stalls() {
        let g = diamond();
        let mut xg = XonNavDriver::new(Arc::clone(&g));
        let from = Vec3::new(0.0, 0.0, 0.0);
        xg.set_goal(NavGoal::Waypoint(3), from);
        // Pin the bot: drive the watchdog directly (update() derives wall-clock dt, so
        // feed it via the internal API for determinism).
        let mut abandoned = false;
        for _ in 0..30 {
            xg.abandoned = false;
            xg.watchdog(from, 0.1);
            if xg.abandoned {
                abandoned = true;
                break;
            }
        }
        assert!(abandoned, "two 0.5 s stalls must surface goal_abandoned");
    }

    #[test]
    fn swim_nodes_get_the_static_penalty() {
        let cm = world::collision::water_channel_world();
        let bounds = ([-144.0, -32.0, -16.0], [144.0, 32.0, 200.0]);
        let g = Arc::new(NavGraph::generate(&cm, bounds, 24.0));
        let water: Vec<usize> = (0..g.node_count())
            .filter(|&i| g.is_water_node(i))
            .collect();
        assert!(!water.is_empty(), "test world must have water nodes");
        let xg = XonNavDriver::new(Arc::clone(&g));
        for &w in &water {
            assert_eq!(xg.effective_overlay()[w], SWIM_NODE_PENALTY);
        }
        // And dry nodes are free.
        let dry = (0..g.node_count()).find(|&i| !g.is_water_node(i)).unwrap();
        assert_eq!(xg.effective_overlay()[dry], 0.0);
    }
}
