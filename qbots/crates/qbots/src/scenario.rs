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
use brain::steer::{move_from_world_dir, Steering};
use brain::{MovementController, MovementIntent, NavigationDriver};
use client::{Conn, ConnState};
use q2proto::Usercmd;

use crate::config::Config;
use crate::supervisor::{spawn_signal_listener, Shutdown};

/// `PMF_ON_GROUND` (`shared.h:646`) — the bot's pmove grounded bit.
const PMF_ON_GROUND: u32 = 4;
/// Within this 3D distance of the goal, the bot has "reached" it.
const GOAL_TOL: f32 = brain::recorder::GOAL_TOL;
/// A reach only counts once held this long (filters fly-through jitter).
const GOAL_SETTLE: f32 = 0.5;
/// Hard wall-clock cap on a scenario run.
pub(crate) const DEFAULT_MAX_SECS: f32 = 30.0;
/// Default map when `--map` is omitted.
const DEFAULT_MAP: &str = "q2dm1";

/// What a scenario drives toward.
pub enum ScenarioGoal {
    /// The DM spawn point farthest (3D) from where the bot spawns.
    FarthestSpawn,
    /// A named weapon's BSP origin (e.g. `rocketlauncher` → `weapon_rocketlauncher`).
    Weapon(String),
}

/// Run a movement scenario: connect one bot, drive it to `goal`, record + dump.
/// Returns a process exit code: `SUCCESS` if the goal was reached, `2` if the run
/// ended without reaching it, `FAILURE` on a setup error or if the bot never became
/// active (so no recorder exists).
pub async fn run_scenario(
    cfg: &Config,
    addr: SocketAddr,
    name: &str,
    map_arg: Option<&str>,
    goal_kind: ScenarioGoal,
    max_secs: f32,
) -> std::io::Result<ExitCode> {
    let map = map_arg
        .map(str::to_string)
        .unwrap_or_else(|| DEFAULT_MAP.to_string());
    if map_arg.is_none() {
        tracing::info!("no --map given; defaulting to {DEFAULT_MAP}");
    }

    // 1. Load the BSP + build the collision model + nav graph (mirrors the fleet's
    //    build_map_nav, but retains the CM for the recorder's wall-bump trace).
    let bsp = world::Bsp::load(&cfg.paths.baseq2, &map)
        .map_err(|e| io_err(format!("can't load BSP for '{map}': {e}")))?;
    let cm = Arc::new(world::CollisionModel::from_bsp(&bsp));
    let model = bsp
        .models
        .first()
        .ok_or_else(|| io_err(format!("BSP for '{map}' has no models")))?;
    let graph = Arc::new(world::NavGraph::generate(
        &cm,
        (model.mins, model.maxs),
        64.0,
    ));
    tracing::info!(
        map,
        nodes = graph.node_count(),
        edges = graph.edge_count(),
        "scenario nav graph"
    );

    // 2. Resolve the scenario label + (when known up front) the goal origin + the
    //    spawn origins for the lazy farthest-spawn pick. A weapon origin is known
    //    now; the farthest spawn is picked once we know where we spawned.
    let (scenario_name, goal_origin, goal_label, spawn_origins) =
        resolve_goal(&bsp, &map, &goal_kind)?;

    // 3. Connect the bot (the same handshake `connect-one` uses).
    let qport = crate::default_qport();
    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    sock.connect(addr).await?;
    let mut conn = Conn::new(addr, name, qport);
    if let Some(pkt) = conn.start() {
        sock.send(&pkt).await?;
    }
    tracing::info!(%name, %map, %addr, qport, "scenario bot connected; driving to goal");

    // 4. Brain primitives + recorder scaffolding.
    let mut move_ctrl = MovementController::new();
    let mut steering = Steering::new(3.0); // mid-skill for scenario runs
    let mut nav_driver = NavigationDriver::new(Arc::clone(&graph));
    let mut last_serverframe: Option<i32> = None;
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
                                let g = farthest_spawn(&spawn_origins, origin_arr);
                                resolved_goal = Some(g);
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

                            // Drive nav to the goal — no combat.
                            nav_driver.update(pos);
                            nav_driver.set_goal(NavGoal::Position(Vec3::from(goal)), pos);

                            // dt from observed serverframe delta (clamped).
                            let current_sf = frame.serverframe;
                            let dt = if let Some(prev_sf) = last_serverframe {
                                ((current_sf - prev_sf).max(0) as f32 * 0.1).clamp(0.02, 0.3)
                            } else {
                                0.1
                            };
                            last_serverframe = Some(current_sf);

                            let mut mv = MovementIntent::new();
                            let mut intent_forward = 0.0;

                            // Steer via pursue_target (T3 look-ahead) with face-then-go.
                            let pursue_pos = nav_driver.pursue_target(pos);
                            let (ideal_yaw, world_move_dir) = if let Some(pt) = pursue_pos {
                                let delta = pt - pos;
                                if delta.length_squared() > 1.0 {
                                    let yaw = delta.y.atan2(delta.x).to_degrees();
                                    let dir = Vec3::new(delta.x, delta.y, 0.0).normalize_or_zero();
                                    (yaw, dir)
                                } else {
                                    (steering.view_yaw(), Vec3::ZERO)
                                }
                            } else {
                                (steering.view_yaw(), Vec3::ZERO)
                            };

                            let view_yaw = steering.change_yaw(ideal_yaw, dt);
                            let arrive = pursue_pos
                                .map(|pt| Steering::arrive_scale((pt - pos).length()))
                                .unwrap_or(1.0);
                            let (fwd, side) = move_from_world_dir(world_move_dir, view_yaw, true);
                            if fwd > 0.0 || side.abs() > 0.0 {
                                mv.look_at(view_yaw, 0.0);
                                mv.move_forward(fwd * arrive);
                                mv.move_side(side * arrive);
                                intent_forward = fwd * arrive;
                            }
                            move_ctrl.set_delta_angles(frame.playerstate.pmove.delta_angles);
                            let cmd = move_ctrl.build_cmd(mv);

                            // Sample the recorder with this frame's telemetry.
                            let (wp, wp_pos) = match nav_driver.current_waypoint() {
                                Some(idx) => (Some(idx), graph.nodes.get(idx).copied()),
                                None => (None, None),
                            };
                            let vel = self_st.velocity;
                            let grounded = self_st.flags & PMF_ON_GROUND != 0;
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
        ScenarioGoal::Weapon(wname) => {
            let cls = format!("weapon_{}", wname.to_ascii_lowercase());
            let origin = bsp.find_class(&cls).first().and_then(|e| e.origin());
            let origin = match origin {
                Some(o) => o,
                None => {
                    let mut avail: Vec<&str> = bsp
                        .entities
                        .iter()
                        .filter_map(|e| e.classname.strip_prefix("weapon_"))
                        .collect();
                    avail.sort();
                    avail.dedup();
                    return Err(io_err(format!(
                        "no '{cls}' on map '{map}'. available weapons: {avail:?}"
                    )));
                }
            };
            Ok((
                "spawn-to-weapon".to_string(),
                Some(origin),
                cls,
                spawn_origins,
            ))
        }
    }
}

/// The DM spawn origin farthest (3D) from `from`, or `from` if there are none.
fn farthest_spawn(spawns: &[[f32; 3]], from: [f32; 3]) -> [f32; 3] {
    spawns
        .iter()
        .copied()
        .max_by(|a, b| dist3_sq(*a, from).total_cmp(&dist3_sq(*b, from)))
        .unwrap_or(from)
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
}
