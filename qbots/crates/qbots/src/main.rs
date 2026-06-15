//! # qbots — external Quake 2 bot client fleet
//!
//! CLI entry point. `connect-one` connects a single bot and keeps it alive; the fleet
//! runner lands in Plan 07. Server address and on-disk Q2 paths come from `config.yaml`.

mod config;

use std::net::SocketAddr;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use config::Config;

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
                let largest = comps.first().expect("nav graph must have at least one component");
                if largest.is_empty() {
                    println!("  no walkable nodes in nav graph");
                    return ExitCode::SUCCESS;
                }
                
                println!("  {} components; largest = {} nodes", comps.len(), largest.len());

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
                        dist2(&g.nodes[x], &g.nodes[s])
                            .total_cmp(&dist2(&g.nodes[y], &g.nodes[s]))
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
            match client::run(addr, &name, qport).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("qbots: {e}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
