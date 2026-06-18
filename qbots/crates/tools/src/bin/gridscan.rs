//! gridscan — measure nav-graph fragmentation as a function of grid spacing,
//! BEFORE any bridging. Decides whether "bridges" are compensating for a too-coarse
//! sampling grid: if a finer grid drops the component count toward 1, the bridge
//! pass (and its dangerous long-range BRIDGE_HDIST) is a workaround we could shrink
//! or remove rather than a fundamental necessity.
//!
//! For each spacing it builds ONLY `NavGraph::generate` (no seed_spawns, no
//! add_elevator_edges, no bridge_components, no prune) and reports node count,
//! component count, largest component size, and how many DM spawns land in the
//! largest component.
//!
//! Usage:
//!   cargo run -p tools --bin gridscan -- <baseq2> <map> [spacing ...]
//! Example:
//!   cargo run -p tools --bin gridscan -- vendor/baseq2 q2dm1 24 16 12 8

use std::path::Path;
use std::time::Instant;
use world::{Bsp, CollisionModel, NavGraph};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: gridscan <baseq2> <map> [spacing ...]   (default spacings: 24 16 12)");
        std::process::exit(2);
    }
    let baseq2 = Path::new(&args[1]);
    let map = &args[2];
    let spacings: Vec<f32> = if args.len() > 3 {
        args[3..].iter().filter_map(|s| s.parse().ok()).collect()
    } else {
        vec![24.0, 16.0, 12.0]
    };

    let bsp = Bsp::load(baseq2, map)?;
    let cm = CollisionModel::from_bsp(&bsp);
    let model = bsp.models.first().ok_or("BSP has no models")?;
    let bounds = (model.mins, model.maxs);
    let spawns: Vec<[f32; 3]> = bsp.spawn_points().iter().map(|s| s.origin).collect();

    println!(
        "map={map} spawns={} (generate-only, pre-bridge)\n",
        spawns.len()
    );
    println!(
        "{:>7}  {:>7}  {:>6}  {:>9}  {:>10}  {:>8}",
        "spacing", "nodes", "comps", "largest", "spawns/lg", "gen_ms"
    );

    for &sp in &spacings {
        let t0 = Instant::now();
        let g = NavGraph::generate(&cm, bounds, sp);
        let gen_ms = t0.elapsed().as_millis();
        let comps = g.components();
        let largest = comps.iter().map(|c| c.len()).max().unwrap_or(0);
        let (in_lg, total) = g.spawns_in_largest_component(&spawns);
        println!(
            "{sp:>7.0}  {:>7}  {:>6}  {:>9}  {:>7}/{:<2}  {gen_ms:>8}",
            g.node_count(),
            comps.len(),
            largest,
            in_lg,
            total
        );
    }
    Ok(())
}
