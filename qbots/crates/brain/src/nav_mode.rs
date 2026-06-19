//! The [`Navigator`] trait — the contract the bot tick loop drives navigation through, so
//! the A* (waypoint-graph) and navmesh backends are interchangeable behind a `--mode` switch.
//!
//! Both [`crate::nav::NavigationDriver`] (the `astar` backend) and (later) `NavmeshDriver`
//! implement this. The scenario tick loop only ever calls these methods, so swapping the
//! backend never touches the steering/movement code below it. Methods with default bodies are
//! backend niceties (telemetry, A*-only smoothing) a backend may legitimately skip.

use glam::Vec3;
use world::CollisionModel;

use crate::nav::NavGoal;

/// The navigation interface the scenario / bot tick loop depends on.
pub trait Navigator {
    /// Set or update the goal; replans only when the goal changes or the path is exhausted.
    fn set_goal(&mut self, goal: NavGoal, from: Vec3);
    /// Per-tick progress + stuck-recovery update. Returns `true` once the goal is reached.
    fn update(&mut self, pos: Vec3, cm: Option<&CollisionModel>) -> bool;
    /// Pure-pursuit look-ahead target along the current path (no safety validation).
    fn pursue_target(&self, from: Vec3) -> Option<Vec3>;
    /// Corner-cut-safe look-ahead target (hull + floor validated).
    fn pursue_target_safe(&self, from: Vec3, cm: &CollisionModel) -> Option<Vec3>;
    /// True if the current path edge is a jump link (the loop presses jump).
    fn current_edge_is_jump(&self) -> bool;
    /// True if the current path edge is a swim link (Plan 39/40: the loop drives
    /// vertical swim thrust + surfacing instead of walking). Defaults to `false` for
    /// backends without water awareness (navmesh).
    fn current_edge_is_swim(&self) -> bool {
        false
    }
    /// Drop the current path so the next `set_goal` replans from scratch.
    fn force_replan(&mut self);
    /// If the current target is hull-blocked from `pos`, blacklist it before a replan.
    fn blacklist_waypoint_if_blocked(&mut self, pos: Vec3, cm: &CollisionModel);

    // ── telemetry / backend-specific (defaults let a backend opt out) ───────────────
    /// Current target node/poly index for the movement recorder (`None` if N/A).
    fn current_waypoint(&self) -> Option<usize> {
        None
    }
    /// World position of the current target for the recorder (`None` if N/A).
    fn current_waypoint_pos(&self) -> Option<[f32; 3]> {
        None
    }
    /// String-pull/smooth the current path (A*-only; navmesh paths are already smooth).
    fn smooth_with_cm(&mut self, _cm: &CollisionModel, _from: Vec3) {}
    /// Apply a per-node risk/popularity cost overlay for the next replan (A* heatmap
    /// planning, Plan 08). No-op for backends without a node graph (navmesh).
    fn set_risk_overlay(&mut self, _overlay: Vec<f32>) {}
    /// True for one tick after the give-up watchdog abandons a stale goal.
    fn goal_abandoned(&self) -> bool {
        false
    }

    /// Forward-speed multiplier for the bot's current position (1.0 = full). A backend can
    /// slow the bot on narrow geometry (thin ledges) so momentum doesn't carry it off the edge.
    fn speed_scale(&self, _pos: Vec3) -> f32 {
        1.0
    }
}

/// A scriptable `Navigator` stub for deterministic brain tests (no nav graph / server needed).
/// Captures the last `set_goal` and returns canned look-ahead / jump-edge / speed values, so a
/// brain's `tick` can be exercised in isolation. Shared across brain unit tests.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct StubNav {
    /// The goal captured from the most recent `set_goal` call.
    pub last_goal: Option<NavGoal>,
    /// Returned by both `pursue_target` and `pursue_target_safe`.
    pub pursue: Option<Vec3>,
    /// Returned by `current_edge_is_jump`.
    pub jump_edge: bool,
    /// Returned by `current_edge_is_swim`.
    pub swim_edge: bool,
    /// Returned by `update` (true = goal reached).
    pub reached: bool,
    /// Returned by `speed_scale` (1.0 unless set).
    pub speed: Option<f32>,
    /// Count of `force_replan` calls (asserts the backoff path replanned).
    pub replans: u32,
}

#[cfg(test)]
impl Navigator for StubNav {
    fn set_goal(&mut self, goal: NavGoal, _from: Vec3) {
        self.last_goal = Some(goal);
    }
    fn update(&mut self, _pos: Vec3, _cm: Option<&CollisionModel>) -> bool {
        self.reached
    }
    fn pursue_target(&self, _from: Vec3) -> Option<Vec3> {
        self.pursue
    }
    fn pursue_target_safe(&self, _from: Vec3, _cm: &CollisionModel) -> Option<Vec3> {
        self.pursue
    }
    fn current_edge_is_jump(&self) -> bool {
        self.jump_edge
    }
    fn current_edge_is_swim(&self) -> bool {
        self.swim_edge
    }
    fn force_replan(&mut self) {
        self.replans += 1;
    }
    fn blacklist_waypoint_if_blocked(&mut self, _pos: Vec3, _cm: &CollisionModel) {}
    fn speed_scale(&self, _pos: Vec3) -> f32 {
        self.speed.unwrap_or(1.0)
    }
}
