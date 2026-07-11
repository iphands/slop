//! Movement-quality scenarios — drive one bot to a known goal and record it.
//!
//! Plan 10's measurement lens: `spawn-to-spawn` / `spawn-to-weapon` connect a single
//! bot like `connect-one`, but pin its nav goal to a scenario target, **disable
//! combat**, and feed every server frame to a [`MovementRecorder`]. The run stops on
//! goal-reach (settled), a `max_secs` cap, or a disconnect, then dumps a structured
//! log + prints the SUMMARY line that Plans 11–14 must beat.
//!
//! This deliberately reuses the brain's nav/steering primitives (it does **not**
//! duplicate combat/aim logic) — only the connect + tick scaffolding is mirrored
//! from [`crate::bot_task`]. It never sets velocity or teleports the bot, so a log
//! showing sustained > ~320 u/s grounded speed flags a physics bug, not a feature.

use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::Vec3;
use tokio::net::UdpSocket;

use brain::nav::NavGoal;
use brain::perception::Worldview;
use brain::recorder::{CmWallProbe, MovementRecorder, Sample, WallProbe};
use brain::{
    build_brain, BotSkill, Brain, BrainConfig, BrainContext, BrainKind, BrainMap,
    MovementController, Navigator,
};
use client::{Conn, ConnState};
use q2proto::Usercmd;
use world::NavGraph;

use crate::config::Config;
use crate::supervisor::{spawn_signal_listener, Shutdown};

/// `PMF_ON_GROUND` (`shared.h:646`) — the bot's pmove grounded bit.
const PMF_ON_GROUND: u32 = 4;
/// Within this 3D distance of the goal, the bot has "reached" it.
const GOAL_TOL: f32 = brain::recorder::GOAL_TOL;
/// A reach only counts once held this long (filters fly-through jitter).
const GOAL_SETTLE: f32 = 0.5;

/// What a scenario drives toward.
#[derive(Clone)]
pub enum ScenarioGoal {
    /// The DM spawn point farthest (3D) from where the bot spawns.
    FarthestSpawn,
    /// A named weapon's BSP origin (e.g. `rocketlauncher` → `weapon_rocketlauncher`).
    /// `instance` selects among multiple matches (q2dm3 has two `weapon_railgun`).
    Weapon { name: String, instance: usize },
    /// A named item's BSP origin, resolved through [`item_classname`] aliases
    /// (e.g. `quaddamage` → `item_quad`). `instance` selects among multiple matches.
    Item { name: String, instance: usize },
    /// An arbitrary world coordinate — used to ISOLATE a single nav feature (e.g. drive to a
    /// func_train's board ledge) without the full item route, so route-reliability and
    /// ride-correctness can be measured separately (Plan 35 T3).
    Point { x: f32, y: f32, z: f32 },
}

/// Resolve a friendly item name to its Q2 entity classname.
///
/// The classnames the engine uses don't always match what an operator types — the quad is
/// `item_quad`, not `item_quaddamage`. This maps common aliases; anything already prefixed
/// `item_` passes through, and anything else gets an `item_` prefix.
pub fn item_classname(name: &str) -> String {
    let n = name.trim().to_ascii_lowercase();
    match n.as_str() {
        "quad" | "quaddamage" | "quad_damage" => "item_quad",
        "invuln" | "invulnerability" | "invulnerable" => "item_invulnerability",
        "mega" | "megahealth" | "mega_health" => "item_health_mega",
        "redarmor" | "bodyarmor" | "red_armor" | "body_armor" => "item_armor_body",
        "yellowarmor" | "combatarmor" | "combat_armor" => "item_armor_combat",
        "greenarmor" | "jacketarmor" | "jacket_armor" => "item_armor_jacket",
        "silencer" => "item_silencer",
        "adrenaline" => "item_adrenaline",
        "bandolier" => "item_bandolier",
        "pack" | "ammopack" | "ammo_pack" => "item_pack",
        other if other.starts_with("item_") => return other.to_string(),
        other => return format!("item_{other}"),
    }
    .to_string()
}

/// Run a movement scenario: connect one bot, drive it to `goal`, record + dump.
/// Returns a process exit code: `SUCCESS` if the goal was reached, `2` if the run
/// ended without reaching it, `FAILURE` on a setup error or if the bot never became
/// active (so no recorder exists).
#[allow(clippy::too_many_arguments)]
pub async fn run_scenario(
    cfg: &Config,
    addr: SocketAddr,
    name: &str,
    map_arg: Option<&str>,
    goal_kind: ScenarioGoal,
    max_secs: f32,
    qport: u16,
    // Grid spacing of the nav graph to load/build (`--spacing`); cached per-spacing.
    spacing: f32,
    // Navigation backend (`--navmode`): the `astar` waypoint graph or the `navmesh` polygon mesh.
    mode: crate::NavMode,
    // Decision plugin (`--brain`): `runtester` (default — the lifted scenario pathfinder) or
    // `main` for an A/B against the live combat brain (combat is forced off here regardless).
    brain_kind: BrainKind,
) -> std::io::Result<ExitCode> {
    // The caller (`run_scenario_cmd`) autodetects the server's map and passes it here;
    // a `None` at this point means autodetection was skipped/failed, which is a bug,
    // not a reason to silently guess a map (a wrong map produces garbage navigation).
    let map = map_arg
        .ok_or_else(|| io_err("no map resolved for scenario (autodetect failed)".to_string()))?
        .to_string();

    // 1. Load BSP + build collision model + nav graph (cache-first, then live).
    let cache_dir = std::path::Path::new("data/mapcache");
    let built = world::cached_map_nav(&cfg.paths.baseq2, &map, Some(cache_dir), spacing)
        .map_err(|e| io_err(format!("can't build nav for '{map}': {e}")))?;

    // All Q2 dm maps guarantee full spawn reachability, so a fragmented graph is a nav bug.
    // For the *movement-test harness* this is a WARNING, not a fatal abort: a scenario only
    // needs the bot's spawn to reach the pinned goal (checked per-spawn below via A*), and we
    // want to be able to exercise goal-reaching (e.g. q2dm3's quad/railgun) while the broad
    // floor-connectivity work (Plan 35) is still in progress. The fleet/production path keeps
    // its own stricter gates.
    if let Err(diag) = world::check_spawn_connectivity(&built) {
        tracing::warn!(%map, "nav graph not fully spawn-connected (movement-test harness continues): {diag}");
    }

    let cm = Arc::clone(&built.cm);
    let bsp_spawns = built.spawn_origins.clone();
    let seeded = built.seeded;
    let added_jumps = built.added_jumps;
    let in_largest = built.in_largest;
    let total_spawns = built.total_spawns;
    let bsp = built.bsp;
    let mut graph = built.graph;

    // 2. Resolve the scenario label + (when known up front) the goal origin + the
    //    spawn origins for the lazy farthest-spawn pick. A weapon origin is known
    //    now; the farthest spawn is picked once we know where we spawned.
    //    Resolved early (before wrapping graph in Arc) so we can seed the goal.
    let (scenario_name, goal_origin, goal_label, spawn_origins) =
        resolve_goal(&bsp, &map, &goal_kind)?;

    // For a weapon goal, the scenario is only possible if some spawn can reach
    // the goal node. An isolated goal (stranded in a disconnected nav component)
    // makes the run impossible — a nav-graph bug per the all-locations-mutually-
    // reachable invariant. Default true; the spawn→goal A* sweep below sets it
    // for weapon goals (spawn goals are always reachable, so they stay true).
    let mut goal_reachable_from_spawn = true;

    // Seed the scenario goal position as an exact nav node when it isn't already one
    // of the DM spawns (T2). For FarthestSpawn the goal is always one of bsp_spawns
    // (already seeded); for Weapon the goal is a single weapon origin that may lie
    // between grid nodes, causing A* to snap to an imprecise neighbor on the wrong
    // side of a doorway or stair lip.
    if let Some(origin) = goal_origin {
        let extra = graph.seed_spawns(&cm, &[origin]);
        if extra > 0 {
            tracing::info!("seeded scenario goal into nav graph (+{extra} node(s))");
        }
        // Ensure the goal node is connected to the main component. The normal
        // BRIDGE_HDIST=128 may not reach a weapon node in an isolated floor pocket
        // (e.g. a high platform accessible via a staircase that is >128u horizontal
        // from the weapon origin itself). Run a wider-radius bridge from the goal
        // node specifically — walkable_stair still filters false connections.
        if let Some(goal_idx) = graph.nearest(&origin) {
            let bridged = graph.connect_node_to_nearby(&cm, goal_idx, 384.0);
            if bridged > 0 {
                tracing::info!(
                    goal_idx,
                    bridged,
                    "extended bridge: connected scenario goal to nearby nodes"
                );
            }
            // Quick connectivity check: can A* reach goal from spawn?
            let comps_check = graph.components();
            let goal_comp = comps_check
                .iter()
                .position(|c| c.contains(&goal_idx))
                .unwrap_or(999);
            tracing::info!(goal_idx, goal_comp, "weapon goal component after bridging");
            // Diagnostic: log adj neighbors of goal node (pos + count)
            let goal_pos = graph.nodes[goal_idx];
            let adj_count = graph.adj_count(goal_idx);
            let neighbor_zs = graph.adj_neighbor_z_levels(goal_idx);
            tracing::info!(
                goal_idx,
                goal_pos = ?[goal_pos[0] as i32, goal_pos[1] as i32, goal_pos[2] as i32],
                adj_count,
                neighbor_z_levels = ?neighbor_zs,
                "goal node adj info"
            );
            // Check A* from each spawn to goal; remember if *any* spawn reaches it.
            let mut any_reach = false;
            let mut logged_kinds = false;
            for (i, sp) in bsp_spawns.iter().enumerate() {
                if let Some(sp_idx) = graph.nearest(sp) {
                    let path = graph.path(sp_idx, goal_idx);
                    let can_reach = path.is_some();
                    any_reach |= can_reach;
                    let sp_pos = graph.nodes[sp_idx];
                    tracing::info!(
                        spawn = i,
                        sp_idx,
                        sp_pos = ?[sp_pos[0] as i32, sp_pos[1] as i32, sp_pos[2] as i32],
                        can_reach,
                        "spawn→goal A* check"
                    );
                    // Once, log the edge-kind composition of a winning path — tells us which
                    // special traversals (Ride/Jump/Swim) the brain must execute to arrive.
                    if let (false, Some(p)) = (logged_kinds, path) {
                        let (mut walk, mut jump, mut swim, mut ride, mut teleport) =
                            (0, 0, 0, 0, 0);
                        for w in p.windows(2) {
                            match graph.edge_kind(w[0], w[1]) {
                                world::EdgeKind::Walk => walk += 1,
                                world::EdgeKind::Jump { .. } => jump += 1,
                                world::EdgeKind::Swim => swim += 1,
                                world::EdgeKind::Ride => ride += 1,
                                world::EdgeKind::Teleport => teleport += 1,
                            }
                        }
                        tracing::info!(
                            spawn = i,
                            nodes = p.len(),
                            walk,
                            jump,
                            swim,
                            ride,
                            teleport,
                            "goal path edge-kind composition"
                        );
                        // Dump each ride edge's endpoints (board→dismount) so we can see the
                        // exact lift/train/ladder hops the route takes (Plan 35 quad debugging).
                        for w in p.windows(2) {
                            if matches!(graph.edge_kind(w[0], w[1]), world::EdgeKind::Ride) {
                                if let Some(ri) = graph.ride_info(w[0], w[1]) {
                                    let f = graph.nodes[w[0]];
                                    let t = graph.nodes[w[1]];
                                    tracing::info!(
                                        from = ?[f[0] as i32, f[1] as i32, f[2] as i32],
                                        to = ?[t[0] as i32, t[1] as i32, t[2] as i32],
                                        ladder = ri.ladder,
                                        vertical = ri.vertical,
                                        "  ride hop"
                                    );
                                }
                            }
                        }
                        logged_kinds = true;
                    }
                }
            }
            goal_reachable_from_spawn = any_reach;
        }
    }

    tracing::info!(count = bsp_spawns.len(), "bsp spawn points collected");
    for (i, sp) in bsp_spawns.iter().enumerate() {
        tracing::info!("  spawn[{}]: ({}, {}, {})", i, sp[0], sp[1], sp[2]);
    }

    // Diagnostic component logging.
    let comps = graph.components();
    if comps.len() > 1 {
        tracing::warn!(
            count = comps.len(),
            "nav graph has multiple disconnected components - THIS IS A BUG"
        );
        for (i, c) in comps.iter().take(5).enumerate() {
            let (mut mnx, mut mny, mut mnz) = (f32::MAX, f32::MAX, f32::MAX);
            let (mut mxx, mut mxy, mut mxz) = (f32::MIN, f32::MIN, f32::MIN);
            for &ni in c {
                let p = graph.nodes[ni];
                mnx = mnx.min(p[0]);
                mxx = mxx.max(p[0]);
                mny = mny.min(p[1]);
                mxy = mxy.max(p[1]);
                mnz = mnz.min(p[2]);
                mxz = mxz.max(p[2]);
            }
            tracing::warn!(
                "  component[{}]: {} nodes bbox x={:.0}..{:.0} y={:.0}..{:.0} z={:.0}..{:.0}",
                i,
                c.len(),
                mnx,
                mxx,
                mny,
                mxy,
                mnz,
                mxz
            );
        }
        for (i, sp) in bsp_spawns.iter().enumerate() {
            if let Some(nearest_idx) = graph.nearest(sp) {
                let comp_idx = comps
                    .iter()
                    .position(|c| c.contains(&nearest_idx))
                    .unwrap_or(999);
                tracing::info!(
                    "  spawn[{}] at ({}, {}, {}) -> nearest node {} -> component {}",
                    i,
                    sp[0],
                    sp[1],
                    sp[2],
                    nearest_idx,
                    comp_idx
                );
            }
        }
    } else {
        tracing::info!("nav graph is fully connected (single component)");
    }
    tracing::info!(
        map,
        nodes = graph.node_count(),
        edges = graph.edge_count(),
        seeded,
        added_jumps,
        in_largest,
        total_spawns,
        "scenario nav graph"
    );

    // Hard abort: if no spawn can reach the goal, the scenario is impossible.
    // All Q2 dm map locations are mutually reachable by design — an isolated
    // goal node is a bug in BSP parsing / collision / nav generation, never a
    // legitimate map property. Diagnostics above have already been dumped.
    if !goal_reachable_from_spawn {
        crate::fatal!(
            %map,
            goal = %goal_label,
            "scenario goal unreachable from every spawn — nav graph bug (goal node isolated); aborting before connecting"
        );
    }

    let graph = Arc::new(graph);

    // 3. Connect the bot (the same handshake `connect-one` uses).
    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    sock.connect(addr).await?;
    let mut conn = Conn::new(addr, name, qport);
    if let Some(pkt) = conn.start() {
        sock.send(&pkt).await?;
    }
    tracing::info!(%name, %map, %addr, qport, "scenario bot connected; driving to goal");

    // 4. Decision brain (Plan 26) + recorder scaffolding.
    let mut move_ctrl = MovementController::new();
    // The scenario runs a `Box<dyn Brain>` (default `runtester` — the lifted pathfinder).
    // Combat is forced off; the goal is pinned per-tick via `BrainContext::goal_override`.
    let mut brain: Box<dyn Brain + Send> = build_brain(
        brain_kind,
        BotSkill::default(),
        BrainConfig {
            combat_enabled: false,
        },
        None, // scenarios don't select a Q3 personality (no combat)
        None, // ...nor a main persona (combat off)
    );

    // Drive through the `Navigator` trait so the tick loop is backend-agnostic. `+ Send`
    // because this future is spawned on tokio and holds the driver across awaits.
    // Reuse one process-wide navmesh across all bots (built from the same collision model the
    // A* graph used). The factory builds it lazily — only for the navmesh + hybrid modes.
    let mut nav_driver: Box<dyn Navigator + Send> =
        crate::build_navigator(mode, Arc::clone(&graph), || {
            let model = &bsp.models[0];
            crate::supervisor::get_or_build_navmesh(&map, &cm, (model.mins, model.maxs))
        });
    // `runtester` ignores the map (it drives the injected nav); `--brain main` uses it for the
    // navmesh roam-as-position flag (its roam ladder is moot here — `goal_override` always wins).
    brain.set_map(BrainMap {
        roam_nodes: Vec::new(),
        nav_graph: Arc::clone(&graph),
        roam_as_position: matches!(mode, crate::NavMode::Navmesh),
        // Static item table (Plan 30) — populated for `--brain main` A/B runs; combat is off in
        // scenarios so health-seek never fires, but keeping it consistent avoids a divergent path.
        items: brain::items::build_map_items(&bsp, &graph),
    });
    let mut last_serverframe: Option<i32> = None;
    // Monotonic tick counter for `BrainContext` (drives jitter/roam in the `--brain main` A/B;
    // the runtester ignores it).
    let mut tick_count: u32 = 0;
    let shutdown = Shutdown::new();
    let _signals = spawn_signal_listener(shutdown.clone());

    let now = time::OffsetDateTime::now_utc();
    let unix_ts = now.unix_timestamp().max(0) as u64;
    // Build the ISO-8601 label from components (avoids the time crate's
    // feature-gated `format_description` path).
    let started_iso = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
    );
    let probe: Arc<dyn WallProbe> = Arc::new(CmWallProbe::new(Arc::clone(&cm)));

    let mut recorder: Option<MovementRecorder> = None;
    let mut buf = vec![0u8; 4096];
    // Plan 57 opt-out: this movement harness deliberately keeps the free-running 100 ms
    // send (no ack-on-frame re-phasing). The Plan 10–14 baselines in
    // `10_movement_test_harness_tracker.md` were recorded against this exact cadence, so
    // re-phasing the send here could shift mean_speed/elapsed and invalidate them. Ping
    // is irrelevant to movement measurement; the fleet loop (`main.rs`) carries the fix.
    let mut ticker = tokio::time::interval(Duration::from_millis(100));
    let start = Instant::now();
    let mut goal_settle_start: Option<f32> = None;
    // Farthest-spawn goal, resolved lazily on the first active frame.
    let mut resolved_goal: Option<[f32; 3]> = goal_origin;
    let mut reached = false;

    loop {
        if shutdown.requested() {
            break;
        }
        let elapsed = start.elapsed().as_secs_f32();
        if elapsed > max_secs {
            tracing::info!(elapsed, max = max_secs, "scenario: max_secs reached");
            break;
        }

        tokio::select! {
            res = sock.recv(&mut buf) => {
                let n = res?;
                if let Some(pkt) = conn.on_recv(&buf[..n]) {
                    let _ = sock.send(&pkt).await;
                }
                if conn.state() == ConnState::Disconnected {
                    tracing::warn!("scenario: server disconnected");
                    break;
                }
            }

            _ = ticker.tick() => {
                let cmd = if conn.state() == ConnState::Active {
                    let (frame_opt, cs) = (conn.frame.clone(), conn.configstrings().clone());
                    frame_opt
                        .map(|frame| {
                            let playernum =
                                conn.serverdata.as_ref().map(|sd| sd.playernum).unwrap_or(0);
                            let view = Worldview::from_frame(&frame, &cs, playernum);
                            let self_st = view.self_state();
                            let pos = self_st.origin;
                            let origin_arr = [pos.x, pos.y, pos.z];

                            // Lazy goal resolution (farthest spawn) once per run.
                            let goal = resolved_goal.unwrap_or_else(|| {
                                let g = farthest_reachable_spawn(&spawn_origins, origin_arr, &graph);
                                resolved_goal = Some(g);
                                tracing::info!(
                                    "goal selected: farthest reachable spawn at ({}, {}, {})",
                                    g[0], g[1], g[2]
                                );
                                g
                            });
                            if recorder.is_none() {
                                recorder = Some(MovementRecorder::new(
                                    Arc::clone(&probe),
                                    goal,
                                    &goal_label,
                                    &scenario_name,
                                    name,
                                    &map,
                                    &started_iso,
                                ));
                            }

                            // dt from observed serverframe delta (clamped) — the brain
                            // consumes it via `BrainContext`.
                            let current_sf = frame.serverframe;
                            let dt = if let Some(prev_sf) = last_serverframe {
                                ((current_sf - prev_sf).max(0) as f32 * 0.1).clamp(0.02, 0.3)
                            } else {
                                0.1
                            };
                            last_serverframe = Some(current_sf);
                            tick_count = tick_count.wrapping_add(1);

                            // Run the decision brain (combat forced off): it drives the injected
                            // navigator to the pinned goal — `nav.update`/`set_goal`/`smooth`/
                            // `pursue_target_safe`/recovery all live inside `tick` now (Plan 26).
                            let out = brain.tick(BrainContext {
                                view: &view,
                                nav: Some(nav_driver.as_mut() as &mut dyn Navigator),
                                cm: Some(&cm),
                                dt,
                                ticks: tick_count,
                                goal_override: Some(NavGoal::Position(Vec3::from(goal))),
                            });
                            // `intent_forward` is the recorder's hindered-flag input (the
                            // nav-step forward; 0 during recovery/backoff) — preserved by the brain.
                            let intent_forward = out.intent_forward;

                            move_ctrl.set_delta_angles(frame.playerstate.pmove.delta_angles);
                            move_ctrl.set_msec(dt);
                            let cmd = move_ctrl.build_cmd(out.intent);

                            // Sample the recorder with this frame's telemetry. Pull the target
                            // position through the trait (not `graph` directly) so the loop stays
                            // backend-agnostic.
                            let (wp, wp_pos) = (
                                nav_driver.current_waypoint(),
                                nav_driver.current_waypoint_pos(),
                            );
                            let vel = self_st.velocity;
                            let grounded = self_st.flags & PMF_ON_GROUND != 0;
                            // Recompute waterlevel ourselves (not on the wire) for the `S` flag.
                            let swimming =
                                brain::water::is_swimming(brain::water::water_level(&cm, pos));
                            // `P`/`L` flags (Plan 43 T4 + Plan 46 T5): the current nav edge is a
                            // mover ride — split into a ladder climb (`L`) vs a platform/lift/train
                            // ride (`P`), which the shared TraversalExecutor drives differently.
                            let on_ride = nav_driver.current_edge_is_ride();
                            let on_ladder =
                                on_ride && nav_driver.current_ride_info().is_some_and(|i| i.ladder);
                            let riding = on_ride && !on_ladder;
                            if let Some(rec) = recorder.as_mut() {
                                rec.sample(Sample {
                                    t_secs: elapsed,
                                    frame: frame.serverframe,
                                    origin: origin_arr,
                                    velocity: [vel.x, vel.y, vel.z],
                                    view_yaw: self_st.angles.y,
                                    view_pitch: self_st.angles.x,
                                    grounded,
                                    waypoint: wp,
                                    waypoint_pos: wp_pos,
                                    intent_forward,
                                    phantom_target: false, // scenario disables combat
                                    recovery: false,        // no recovery in scenario mode
                                    swimming,
                                    riding,
                                    ladder: on_ladder,
                                });
                            }

                            // Goal-reach settle: hold within GOAL_TOL for GOAL_SETTLE s.
                            let now_reached = dist3(origin_arr, goal) < GOAL_TOL;
                            if now_reached {
                                goal_settle_start.get_or_insert(elapsed);
                            } else {
                                goal_settle_start = None;
                            }
                            cmd
                        })
                        .unwrap_or_default()
                } else {
                    Usercmd::default()
                };

                if let Some(pkt) = conn.transmit_cmd(&cmd) {
                    let _ = sock.send(&pkt).await;
                }
                if goal_settle_start.is_some_and(|s| elapsed - s >= GOAL_SETTLE) {
                    reached = true;
                    tracing::info!("scenario: reached goal (settled)");
                    break;
                }
            }
        }
    }

    // 5. Disconnect cleanly, dump the log, print the SUMMARY line.
    if conn.state() == ConnState::Active {
        if let Some(pkt) = conn.disconnect() {
            let _ = sock.send(&pkt).await;
            let _ = sock.send(&pkt).await;
        }
    }

    Ok(finalize(
        recorder.as_ref(),
        &scenario_name,
        name,
        unix_ts,
        reached,
    ))
}

/// Resolve the scenario name, goal origin (when known up front), goal label, and
/// the list of DM spawn origins (for the lazy farthest-spawn pick).
#[allow(clippy::type_complexity)]
fn resolve_goal(
    bsp: &world::Bsp,
    map: &str,
    goal_kind: &ScenarioGoal,
) -> std::io::Result<(String, Option<[f32; 3]>, String, Vec<[f32; 3]>)> {
    let spawns = bsp.spawn_points();
    let spawn_origins: Vec<[f32; 3]> = spawns.iter().map(|s| s.origin).collect();
    match goal_kind {
        ScenarioGoal::FarthestSpawn => {
            if spawn_origins.is_empty() {
                return Err(io_err(format!("map '{map}' has no DM spawn points")));
            }
            Ok((
                "spawn-to-spawn".to_string(),
                None,
                "farthest_dm_spawn".to_string(),
                spawn_origins,
            ))
        }
        ScenarioGoal::Weapon { name, instance } => {
            let cls = format!("weapon_{}", name.to_ascii_lowercase());
            let origin = resolve_class_origin(bsp, map, &cls, *instance, "weapon_")?;
            Ok((
                "spawn-to-weapon".to_string(),
                Some(origin),
                cls,
                spawn_origins,
            ))
        }
        ScenarioGoal::Item { name, instance } => {
            let cls = item_classname(name);
            let origin = resolve_class_origin(bsp, map, &cls, *instance, "item_")?;
            Ok((
                "spawn-to-item".to_string(),
                Some(origin),
                cls,
                spawn_origins,
            ))
        }
        ScenarioGoal::Point { x, y, z } => Ok((
            "spawn-to-point".to_string(),
            Some([*x, *y, *z]),
            format!("point_{}_{}_{}", *x as i32, *y as i32, *z as i32),
            spawn_origins,
        )),
    }
}

/// Resolve the `instance`-th origin of `cls` on the map, logging every candidate so the
/// operator can pick. On no match, list the available classnames sharing `avail_prefix`.
fn resolve_class_origin(
    bsp: &world::Bsp,
    map: &str,
    cls: &str,
    instance: usize,
    avail_prefix: &str,
) -> std::io::Result<[f32; 3]> {
    let origins: Vec<[f32; 3]> = bsp
        .find_class(cls)
        .iter()
        .filter_map(|e| e.origin())
        .collect();

    if origins.is_empty() {
        let mut avail: Vec<&str> = bsp
            .entities
            .iter()
            .filter_map(|e| e.classname.strip_prefix(avail_prefix))
            .collect();
        avail.sort();
        avail.dedup();
        return Err(io_err(format!(
            "no '{cls}' on map '{map}'. available: {avail:?}"
        )));
    }

    tracing::info!(
        %cls,
        count = origins.len(),
        candidates = ?origins
            .iter()
            .map(|o| [o[0] as i32, o[1] as i32, o[2] as i32])
            .collect::<Vec<_>>(),
        "resolved class candidates (use --instance N to pick)"
    );

    origins.get(instance).copied().ok_or_else(|| {
        io_err(format!(
            "--instance {instance} out of range for '{cls}' on '{map}' (have {})",
            origins.len()
        ))
    })
}

/// The DM spawn origin farthest (3D) from `from`, or `from` if there are none.
fn farthest_spawn(spawns: &[[f32; 3]], from: [f32; 3]) -> [f32; 3] {
    spawns
        .iter()
        .copied()
        .max_by(|a, b| dist3_sq(*a, from).total_cmp(&dist3_sq(*b, from)))
        .unwrap_or(from)
}

/// The farthest DM spawn that is in the same nav graph component as the bot.
/// Falls back to the farthest spawn by Euclidean distance if no spawns are in the same component.
/// Excludes spawns that are too close to the bot's current position (< 100 units).
fn farthest_reachable_spawn(
    spawns: &[[f32; 3]],
    from: [f32; 3],
    graph: &Arc<NavGraph>,
) -> [f32; 3] {
    let Some(from_node) = graph.nearest(&from) else {
        return farthest_spawn(spawns, from);
    };

    // Find the component the bot is in
    let components = graph.components();
    let bot_component = components.iter().position(|c| c.contains(&from_node));

    // Find spawns in the same component that are far enough away
    let mut same_component: Vec<([f32; 3], f32)> = Vec::new();
    for &sp in spawns {
        // Skip spawns that are too close to the bot's current position
        if dist3_sq(sp, from) < 100.0 * 100.0 {
            continue;
        }
        if let Some(sp_node) = graph.nearest(&sp) {
            // Check if the spawn is in the same component
            if let Some(idx) = &bot_component {
                if components.get(*idx).is_some_and(|c| c.contains(&sp_node)) {
                    same_component.push((sp, dist3_sq(sp, from)));
                }
            }
        }
    }

    // If we have spawns in the same component, pick the farthest one
    if !same_component.is_empty() {
        same_component.sort_by(|a, b| b.1.total_cmp(&a.1));
        return same_component[0].0;
    }

    // No spawns in the same component - this means the bot is in an isolated component
    // with no other spawns. Return the bot's current position (no goal).
    tracing::warn!("no reachable spawns in same component as bot");
    from
}

/// Dump the recorder log + emit the SUMMARY line; map outcome → exit code.
fn finalize(
    recorder: Option<&MovementRecorder>,
    scenario_name: &str,
    name: &str,
    unix_ts: u64,
    reached: bool,
) -> ExitCode {
    let Some(rec) = recorder else {
        tracing::warn!("scenario ended before the bot became active (no recorder)");
        return ExitCode::FAILURE;
    };
    let dir = std::path::Path::new("logs").join(scenario_name);
    let path = dir.join(format!("{unix_ts}.{name}.log"));
    if let Err(e) = rec.dump(&path) {
        tracing::warn!("recorder dump failed: {e}");
    } else {
        tracing::info!(path = %path.display(), "movement log written");
    }
    let s = rec.summary();
    tracing::info!(
        reached = reached,
        elapsed = format!("{:.2}", s.elapsed_secs),
        distance = format!("{:.0}", s.distance),
        mean_speed = format!("{:.0}", s.mean_speed),
        max_speed = format!("{:.0}", s.max_speed),
        bumps = s.bumps,
        wrong_turns = s.wrong_turns,
        hindered_frames = s.hindered_frames,
        "SUMMARY",
    );
    if reached {
        ExitCode::SUCCESS
    } else {
        // Exit code 2 = ran but did not reach the goal (distinct from a setup error).
        ExitCode::from(2)
    }
}

fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    dist3_sq(a, b).sqrt()
}

fn dist3_sq(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

fn io_err(msg: String) -> std::io::Error {
    std::io::Error::other(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn farthest_spawn_picks_the_distant_one() {
        let spawns = [[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [1000.0, 0.0, 0.0]];
        assert_eq!(farthest_spawn(&spawns, [0.0, 0.0, 0.0]), [1000.0, 0.0, 0.0]);
        // From the far end, the nearest-spawn (0,0,0) is now farthest.
        assert_eq!(farthest_spawn(&spawns, [900.0, 0.0, 0.0]), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn farthest_spawn_empty_returns_from() {
        assert_eq!(farthest_spawn(&[], [5.0, 6.0, 7.0]), [5.0, 6.0, 7.0]);
    }

    #[test]
    fn item_classname_aliases() {
        // The quad's real classname is `item_quad`, not `item_quaddamage`.
        assert_eq!(item_classname("quaddamage"), "item_quad");
        assert_eq!(item_classname("quad"), "item_quad");
        assert_eq!(item_classname("QuadDamage"), "item_quad");
        assert_eq!(item_classname("invuln"), "item_invulnerability");
        assert_eq!(item_classname("mega"), "item_health_mega");
        // Already-prefixed names pass through (lowercased).
        assert_eq!(item_classname("item_health"), "item_health");
        assert_eq!(item_classname("Item_Health"), "item_health");
        // Unknown names get the `item_` prefix.
        assert_eq!(item_classname("health"), "item_health");
    }
}
