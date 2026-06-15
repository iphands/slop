//! # qbots — external Quake 2 bot client fleet
//!
//! CLI entry point. `connect-one` connects a single bot and keeps it alive; the fleet
//! runner lands in Plan 07. Server address and on-disk Q2 paths come from `config.yaml`.

mod config;

use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use config::Config;
use q2proto::Usercmd;
use tokio::net::UdpSocket;
use tokio::time;

#[derive(Parser)]
#[command(
    name = "qbots",
    about = "External Quake 2 bot clients that connect to a real server over UDP"
)]
struct Cli {
    /// Config file (server address + Q2 paths).
    #[arg(long, default_value = "config.yaml", global = true)]
    config: String,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Connect a single bot to a server and keep it alive.
    ConnectOne {
        /// Server address (defaults to config's server, e.g. `noir.lan:27910`).
        #[arg(long)]
        addr: Option<String>,
        /// Bot display name (userinfo `name`).
        #[arg(long)]
        name: Option<String>,
        /// Client qport (defaults to a per-process value; must be unique across bots).
        #[arg(long)]
        qport: Option<u16>,
    },
    /// Print the loaded config (server + paths) and exit.
    Config,
    /// Load + dump a BSP (planes/nodes/leafs/brushes counts) from the configured baseq2.
    BspInfo { map: String },
    /// Build the collision model for a map and fire test rays from its center.
    Trace { map: String },
    /// Show PVS info for a map (cluster at the center + how many clusters it sees).
    Pvs { map: String },
    /// Generate the nav graph for a map and find a corner-to-corner path.
    Nav { map: String },
}

/// A per-process default qport (distinct across concurrent bot processes).
fn default_qport() -> u16 {
    (std::process::id() & 0xFFFF) as u16
}

/// Squared 3D distance (for nearest-waypoint comparisons).
fn dist2(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// Resolve `host[:port]` to a socket address via DNS lookup. Hostnames (e.g.
/// `noir.lan`), `IP:port`, and bare IPs (defaulting port to 27910) all work.
async fn resolve_addr(addr: &str) -> Result<SocketAddr, String> {
    let target = if addr.contains(':') {
        addr.to_string()
    } else {
        format!("{addr}:27910")
    };
    // Pass `target` by value so the lookup future owns it (avoids a borrow that would
    // otherwise be extended across the await).
    match tokio::net::lookup_host(target).await {
        Ok(mut it) => it
            .next()
            .ok_or_else(|| format!("no addresses found for '{addr}'")),
        Err(e) => Err(format!("can't resolve '{addr}': {e}")),
    }
}

/// Connect a single bot and drive it with the full brain (FSM + nav + combat).
async fn run_brain_bot(
    addr: SocketAddr,
    name: &str,
    qport: u16,
    cfg: &Config,
) -> std::io::Result<()> {
    use brain::fsm::BehaviorState;
    use brain::perception::Worldview;
    use brain::{
        BotSkill, CombatDriver, MovementController, MovementIntent, NavGoal, NavigationDriver,
    };
    use client::{Conn, ConnState};

    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    sock.connect(addr).await?;
    let mut conn = Conn::new(addr, name, qport);

    if let Some(pkt) = conn.start() {
        sock.send(&pkt).await?;
    }

    let mut buf = vec![0u8; 4096];
    let mut ticker = time::interval(Duration::from_millis(100));
    let mut ticks: u32 = 0;

    let mut fsm = BehaviorState::Roam;
    let mut combat = CombatDriver::new();
    let mut move_ctrl = MovementController::new();
    let skill = BotSkill::default();
    let mut nav_driver: Option<NavigationDriver> = None;
    let mut roam_nodes: Vec<usize> = Vec::new();
    let mut roam_idx: usize = 0;
    let mut map_loaded = false;

    loop {
        tokio::select! {
            res = sock.recv(&mut buf) => {
                let n = res?;
                if let Some(pkt) = conn.on_recv(&buf[..n]) {
                    let _ = sock.send(&pkt).await;
                }
                if conn.state() == ConnState::Disconnected {
                    break;
                }

            }

            _ = ticker.tick() => {
                ticks = ticks.wrapping_add(1);

                // Clone what we need before any mutable borrow.
                let (frame_opt, cs) = (conn.frame.clone(), conn.configstrings().clone());
                let state = conn.state();
                let playernum = conn.serverdata.as_ref().map(|sd| sd.playernum).unwrap_or(0);

                // Lazy-load nav graph from CS_MODELS+1 (index 33 = "maps/q2dm7.bsp").
                // CS_NAME in svc_serverdata is the display name, not the filename.
                // Configstrings arrive over multiple packets so we retry each tick.
                if !map_loaded && state == ConnState::Active {
                    if let Some(bsp_path) = cs.get(33) {
                        if !bsp_path.is_empty() {
                            let bsp_path = bsp_path.to_owned();
                            let map = bsp_path
                                .strip_prefix("maps/")
                                .unwrap_or(&bsp_path)
                                .strip_suffix(".bsp")
                                .unwrap_or(&bsp_path)
                                .to_owned();
                            map_loaded = true;
                            eprintln!("qbots: BSP={bsp_path}  building nav graph for '{map}'…");
                            match world::Bsp::load(&cfg.paths.baseq2, &map) {
                                Ok(bsp) => {
                                    let cm = world::CollisionModel::from_bsp(&bsp);
                                    let m = bsp.models.first().expect("bsp has models");
                                    let t0 = std::time::Instant::now();
                                    let g =
                                        world::NavGraph::generate(&cm, (m.mins, m.maxs), 64.0);
                                    let comps = g.components();
                                    let largest = comps.into_iter().next().unwrap_or_default();
                                    eprintln!(
                                        "qbots: nav ready: {} nodes / {} edges  largest={} ({} ms)",
                                        g.node_count(),
                                        g.edge_count(),
                                        largest.len(),
                                        t0.elapsed().as_millis()
                                    );
                                    roam_nodes = largest;
                                    nav_driver = Some(NavigationDriver::new(Arc::new(g)));
                                }
                                Err(e) => {
                                    eprintln!("qbots: nav load failed: {e}  (no nav)");
                                }
                            }
                        }
                    }
                }

                let cmd = if state == ConnState::Active {
                    if let Some(frame) = frame_opt {
                        let view = Worldview::from_frame(&frame, &cs, playernum);
                        let fsm_intent = fsm.tick(&view);
                        let jitter = (ticks as f32) * 0.1;
                        let combat_dec =
                            combat.evaluate(&view, skill.aim_jitter_factor(), jitter);

                        let mut mv = MovementIntent::new();

                        // Aim + fire when we have a target.
                        if combat_dec.should_fire {
                            mv.look_at(combat_dec.aim_yaw, combat_dec.aim_pitch);
                            mv.attack();
                        }

                        // Navigation: set FSM goal or cycle roam waypoints.
                        let pos = view.self_state().origin;
                        if let Some(nav) = nav_driver.as_mut() {
                            nav.update(pos);

                            let goal = if let Some(g) = fsm_intent.nav_goal {
                                g
                            } else if !roam_nodes.is_empty() {
                                if ticks.is_multiple_of(50) {
                                    // Advance to a well-spread roam target (skip ~1/7 of nodes).
                                    roam_idx = (roam_idx + roam_nodes.len() / 7 + 1)
                                        % roam_nodes.len();
                                }
                                NavGoal::Waypoint(roam_nodes[roam_idx])
                            } else {
                                NavGoal::Position(pos)
                            };
                            nav.set_goal(goal, pos);

                            if let Some(dir) = nav.next_waypoint_direction(pos) {
                                if !combat_dec.should_fire {
                                    let yaw = dir.y.atan2(dir.x).to_degrees();
                                    let pitch =
                                        (-dir.z).atan2(dir.x.hypot(dir.y)).to_degrees();
                                    mv.look_at(yaw, pitch);
                                }
                                mv.move_forward(400.0);
                            }

                            if nav.is_stuck() {
                                mv.jump();
                            }
                        } else if !combat_dec.should_fire {
                            mv.move_forward(200.0);
                            if ticks.is_multiple_of(20) {
                                mv.jump();
                            }
                        }

                        let mut cmd = move_ctrl.build_cmd(mv);
                        cmd.impulse = combat_dec.impulse.unwrap_or(0);
                        cmd
                    } else {
                        Usercmd::default()
                    }
                } else {
                    Usercmd::default()
                };

                if let Some(pkt) = conn.transmit_cmd(&cmd) {
                    let _ = sock.send(&pkt).await;
                }

                if ticks.is_multiple_of(10) {
                    match conn.frame.as_ref() {
                        Some(f) => {
                            let o = f.playerstate.pmove.origin_f32();
                            eprintln!(
                                "qbots: {:?} frame={} ents={} origin=({:.1},{:.1},{:.1}) fsm={fsm:?}",
                                conn.state(),
                                f.serverframe,
                                f.entities.len(),
                                o[0],
                                o[1],
                                o[2],
                            );
                        }
                        None => eprintln!("qbots: {:?} (no frame yet)", conn.state()),
                    }
                }
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    let cfg = match Config::load(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("qbots: config: {e}");
            return ExitCode::FAILURE;
        }
    };

    match cli.cmd {
        Cmd::Config => {
            println!("server      : {}", cfg.server_addr());
            println!("server_cfg  : {}", cfg.paths.server_cfg.display());
            println!("baseq2      : {}", cfg.paths.baseq2.display());
            let maps_dir = cfg.paths.baseq2.join("maps");
            match std::fs::read_dir(&maps_dir) {
                Ok(entries) => {
                    let n = entries
                        .filter_map(Result::ok)
                        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("bsp"))
                        .count();
                    println!("maps        : {n} .bsp files in {}", maps_dir.display());
                }
                Err(e) => println!("maps        : can't read {}: {e}", maps_dir.display()),
            }
            let q2dm1 = cfg.map_bsp("q2dm1");
            let exists = q2dm1.exists();
            println!(
                "q2dm1.bsp   : {} ({})",
                q2dm1.display(),
                if exists { "found" } else { "MISSING" }
            );
            ExitCode::SUCCESS
        }
        Cmd::Trace { map } => match world::Bsp::load(&cfg.paths.baseq2, &map) {
            Ok(bsp) => {
                let cm = world::CollisionModel::from_bsp(&bsp);
                let m = bsp.models.first().expect("bsp has models");
                let center = [
                    (m.mins[0] + m.maxs[0]) * 0.5,
                    (m.mins[1] + m.maxs[1]) * 0.5,
                    (m.mins[2] + m.maxs[2]) * 0.5,
                ];
                println!(
                    "{}: bounds [{:.0},{:.0},{:.0}]..[{:.0},{:.0},{:.0}]  center=({:.0},{:.0},{:.0})",
                    map,
                    m.mins[0],
                    m.mins[1],
                    m.mins[2],
                    m.maxs[0],
                    m.maxs[1],
                    m.maxs[2],
                    center[0],
                    center[1],
                    center[2]
                );
                println!(
                    "  point_contents(center) = {:#x}  is_solid={}",
                    cm.point_contents(&center),
                    cm.is_solid(&center)
                );
                // 8 horizontal rays, 4096 units each, from the center.
                const RAY: f32 = 4096.0;
                let dirs = [
                    [1.0f32, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [-1.0, 0.0, 0.0],
                    [0.0, -1.0, 0.0],
                    [1.0, 1.0, 0.0],
                    [-1.0, -1.0, 0.0],
                    [1.0, -1.0, 0.0],
                    [-1.0, 1.0, 0.0],
                ];
                for d in dirs {
                    let n = (d[0] * d[0] + d[1] * d[1]).sqrt();
                    let dir = [d[0] / n, d[1] / n, 0.0];
                    let end = [
                        center[0] + dir[0] * RAY,
                        center[1] + dir[1] * RAY,
                        center[2],
                    ];
                    let t = cm.trace(&center, &end, &[0.0; 3], &[0.0; 3], world::MASK_SOLID);
                    println!(
                        "  dir ({:+.1},{:+.1}): frac={:.3}  hit at {:.0} units  {}",
                        dir[0],
                        dir[1],
                        t.fraction,
                        t.fraction * RAY,
                        if t.fraction < 1.0 { "WALL" } else { "clear" }
                    );
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("qbots: {e}");
                ExitCode::FAILURE
            }
        },
        Cmd::Pvs { map } => match world::Bsp::load(&cfg.paths.baseq2, &map) {
            Ok(bsp) => {
                let cm = world::CollisionModel::from_bsp(&bsp);
                let pvs = world::Pvs::from_lump(bsp.vis.clone());
                match &pvs {
                    Some(p) => println!("{}: {} clusters", map, p.numclusters()),
                    None => println!("{}: no PVS lump", map),
                }
                let m = bsp.models.first().expect("bsp has models");
                let center = [
                    (m.mins[0] + m.maxs[0]) * 0.5,
                    (m.mins[1] + m.maxs[1]) * 0.5,
                    (m.mins[2] + m.maxs[2]) * 0.5,
                ];
                let cluster = cm.point_cluster(&center);
                println!(
                    "  center ({:.0},{:.0},{:.0}) → cluster {}",
                    center[0], center[1], center[2], cluster
                );
                if let Some(p) = &pvs {
                    println!(
                        "  clusters visible from {}: {}",
                        cluster,
                        p.count_visible(cluster)
                    );
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("qbots: {e}");
                ExitCode::FAILURE
            }
        },
        Cmd::Nav { map } => match world::Bsp::load(&cfg.paths.baseq2, &map) {
            Ok(bsp) => {
                let cm = world::CollisionModel::from_bsp(&bsp);
                let m = bsp.models.first().expect("bsp has models");
                let bounds = (m.mins, m.maxs);

                let t0 = std::time::Instant::now();
                let g = world::NavGraph::generate(&cm, bounds, 64.0);
                println!(
                    "{}: nav graph  {} nodes / {} edges  (spacing 64, {} ms)",
                    map,
                    g.node_count(),
                    g.edge_count(),
                    t0.elapsed().as_millis(),
                );

                // Diagnose connectivity, then find a path inside the largest component.
                let cz = (m.mins[2] + m.maxs[2]) * 0.5;
                let start = g.nearest(&[m.mins[0] + 200.0, m.mins[1] + 200.0, cz]);
                let comps = g.components();

                // Guard against empty nav graph
                let largest = comps
                    .first()
                    .expect("nav graph must have at least one component");
                if largest.is_empty() {
                    println!("  no walkable nodes in nav graph");
                    return ExitCode::SUCCESS;
                }

                println!(
                    "  {} components; largest = {} nodes",
                    comps.len(),
                    largest.len()
                );

                // Pick a start node from the largest component
                let s = if let Some(start) = start {
                    if largest.contains(&start) {
                        start
                    } else {
                        largest[0]
                    }
                } else {
                    largest[0]
                };

                // Find the farthest node in the largest component
                let farthest = largest
                    .iter()
                    .copied()
                    .max_by(|&x, &y| {
                        dist2(&g.nodes[x], &g.nodes[s]).total_cmp(&dist2(&g.nodes[y], &g.nodes[s]))
                    })
                    .expect("largest component must have nodes");

                if s == farthest {
                    println!("  only one node in largest component");
                    return ExitCode::SUCCESS;
                }

                let t0 = std::time::Instant::now();
                match g.path(s, farthest) {
                    Some(path) => {
                        let len: f32 = path
                            .windows(2)
                            .map(|w| {
                                let a = g.nodes[w[0]];
                                let b = g.nodes[w[1]];
                                ((a[0] - b[0]).powi(2)
                                    + (a[1] - b[1]).powi(2)
                                    + (a[2] - b[2]).powi(2))
                                .sqrt()
                            })
                            .sum();
                        println!(
                            "  path (in largest): {}→{}: {} hops / {} nodes, {:.0} units  ({} ms)",
                            s,
                            farthest,
                            path.len() - 1,
                            path.len(),
                            len,
                            t0.elapsed().as_millis(),
                        );
                    }
                    None => println!("  no path in largest component (this is a bug!)"),
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("qbots: {e}");
                ExitCode::FAILURE
            }
        },
        Cmd::BspInfo { map } => match world::Bsp::load(&cfg.paths.baseq2, &map) {
            Ok(bsp) => {
                println!(
                    "{}: v{} | {} planes, {} nodes, {} leafs, {} brushes, {} brushsides, {} leafbrushes, {} models",
                    map,
                    bsp.version,
                    bsp.planes.len(),
                    bsp.nodes.len(),
                    bsp.leafs.len(),
                    bsp.brushes.len(),
                    bsp.brushsides.len(),
                    bsp.leafbrushes.len(),
                    bsp.models.len(),
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("qbots: {e}");
                ExitCode::FAILURE
            }
        },
        Cmd::ConnectOne { addr, name, qport } => {
            let name = name.unwrap_or_else(|| "qbots".to_string());
            let qport = qport.unwrap_or_else(default_qport);
            let addr_str = addr.unwrap_or_else(|| cfg.server_addr());
            let addr = match resolve_addr(&addr_str).await {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("qbots: {e}");
                    return ExitCode::FAILURE;
                }
            };
            println!("qbots: connecting '{name}' to {addr} (qport {qport})…  Ctrl-C to stop.");
            match run_brain_bot(addr, &name, qport, &cfg).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("qbots: {e}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
