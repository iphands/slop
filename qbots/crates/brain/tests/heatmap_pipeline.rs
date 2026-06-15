//! Plan 08 T6 — end-to-end verification of the danger/popularity → routing
//! pipeline.
//!
//! The live-server check ("drive a bot into a chokepoint, watch others detour
//! within ~30 s, then watch decay restore the route") needs a running Q2 server.
//! While the server is down this stands in for it deterministically: it wires the
//! real `HeatmapObserver` → `NavigationDriver` → `BotSkill::heatmap_weights`
//! pipeline over a real nav graph and asserts the two headline behaviors —
//! **detour around a kill-zone**, and **decay restores the direct route**. The
//! gravitation mechanism is covered by `world`'s `path_weighted` unit test.

use std::sync::Arc;

use brain::{BotSkill, HeatmapObserver, NavGoal, NavigationDriver, Personality};
use glam::Vec3;
use world::NavGraph;

/// Diamond graph: A(0)→C(2) direct via B(1) (base 200), or longer via D(3) (282).
/// B is the chokepoint we'll turn into a kill-zone.
fn diamond() -> Arc<NavGraph> {
    Arc::new(NavGraph::from_raw(
        vec![
            [0.0, 0.0, 0.0],     // 0 = A
            [0.0, 100.0, 0.0],   // 1 = B (kill-zone)
            [0.0, 200.0, 0.0],   // 2 = C (goal)
            [100.0, 100.0, 0.0], // 3 = D (detour)
        ],
        vec![
            vec![(1, 100.0), (3, 141.0)], // A → B, D
            vec![(0, 100.0), (2, 100.0)], // B → A, C
            vec![(1, 100.0), (3, 141.0)], // C → B, D
            vec![(0, 141.0), (2, 141.0)], // D → A, C
        ],
    ))
}

/// Plan A→C with the observer's current overlay; return the first waypoint
/// (1 = via B, 3 = via D).
fn plan_route(
    obs: &HeatmapObserver,
    nav: &mut NavigationDriver,
    w_d: f32,
    w_p: f32,
) -> Option<usize> {
    nav.set_risk_overlay(obs.cost_overlay(w_d, w_p));
    nav.force_replan();
    nav.set_goal(NavGoal::Waypoint(2), Vec3::new(0.0, 0.0, 0.0));
    nav.current_waypoint()
}

#[test]
fn repeated_deaths_detour_around_killzone_then_decay_restores() {
    let graph = diamond();
    let mut obs = HeatmapObserver::new(Arc::clone(&graph), "me");
    let mut nav = NavigationDriver::new(Arc::clone(&graph));
    let (w_danger, w_pop) = BotSkill::new(5, Personality::Balanced).heatmap_weights();

    // Baseline: no danger → shortest route via B.
    assert_eq!(
        plan_route(&obs, &mut nav, w_danger, w_pop),
        Some(1),
        "baseline routes via B"
    );

    // Die at B three times → it becomes a kill-zone; the route detours via D.
    for _ in 0..3 {
        obs.on_self_death(Vec3::new(0.0, 100.0, 0.0));
    }
    assert_eq!(
        plan_route(&obs, &mut nav, w_danger, w_pop),
        Some(3),
        "danger at B detours via D"
    );

    // Let danger decay well past TAU_DANGER (~45 s) until B cools.
    for _ in 0..5000 {
        obs.tick(0.1);
    }
    assert_eq!(
        plan_route(&obs, &mut nav, w_danger, w_pop),
        Some(1),
        "after decay the direct route via B is restored"
    );
}

#[test]
fn higher_skill_detours_more_readily() {
    // A single death at B should flip a high-skill (risk-averse) bot onto the
    // detour, but barely move a low-skill one.
    let graph = diamond();
    let (w_hi, _) = BotSkill::new(9, Personality::Balanced).heatmap_weights();
    let (w_lo, _) = BotSkill::new(1, Personality::Balanced).heatmap_weights();
    assert!(w_hi > w_lo, "high skill weights danger more");

    // High-skill: one death → detour via D.
    let mut obs = HeatmapObserver::new(Arc::clone(&graph), "me");
    let mut nav = NavigationDriver::new(Arc::clone(&graph));
    obs.on_self_death(Vec3::new(0.0, 100.0, 0.0));
    let (_, w_pop) = BotSkill::new(9, Personality::Balanced).heatmap_weights();
    assert_eq!(
        plan_route(&obs, &mut nav, w_hi, w_pop),
        Some(3),
        "high-skill bot detours after one death at B"
    );

    // Low-skill: one death (tiny W_danger) is not enough to overcome the 82-unit
    // base gap, so it keeps the direct route via B.
    let mut obs = HeatmapObserver::new(Arc::clone(&graph), "me");
    let mut nav = NavigationDriver::new(Arc::clone(&graph));
    obs.on_self_death(Vec3::new(0.0, 100.0, 0.0));
    let (_, w_pop) = BotSkill::new(1, Personality::Balanced).heatmap_weights();
    assert_eq!(
        plan_route(&obs, &mut nav, w_lo, w_pop),
        Some(1),
        "low-skill bot keeps the direct route after one death"
    );
}
