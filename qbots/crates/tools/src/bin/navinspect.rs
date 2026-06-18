//! navinspect — dump nav-graph nodes and edges near a query point.
//!
//! Reusable diagnostic for investigating navigation dead-ends: list every node
//! within a radius of a coordinate, its outgoing edges (with target coords, dz,
//! horizontal distance, and a live hull-trace walkability re-check), so false
//! edges (graph edge present but hull-blocked in the live collision model) and
//! missing edges at stair/transition zones are easy to spot.
//!
//! Usage:
//!   cargo run -p tools --bin navinspect -- <baseq2> <map> <x> <y> <z> [radius]
//!
//! Example (the q2dm1 z=472→567 transition that blocks reaching spawn[3]):
//!   cargo run -p tools --bin navinspect -- /srv/q2/baseq2 q2dm1 1519 567 472 160

use std::path::Path;
use world::navgraph::{walkable_stair, HULL_MAXS, HULL_MINS};
use world::{cached_map_nav, MASK_SOLID, STEP};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 6 {
        eprintln!(
            "usage: navinspect <baseq2> <map> <x> <y> <z> [radius]\n\
             e.g.   navinspect /srv/q2/baseq2 q2dm1 1519 567 472 160"
        );
        std::process::exit(2);
    }
    let baseq2 = Path::new(&args[1]);
    let map = &args[2];
    let qx: f32 = args[3].parse()?;
    let qy: f32 = args[4].parse()?;
    let qz: f32 = args[5].parse()?;
    let radius: f32 = args.get(6).map(|s| s.parse()).transpose()?.unwrap_or(128.0);

    let cache = Path::new("data/mapcache");
    let built = cached_map_nav(baseq2, map, Some(cache))?;
    let g = &built.graph;
    let cm = &built.cm;

    println!(
        "map={map} nodes={} query=({qx},{qy},{qz}) radius={radius}",
        g.node_count()
    );

    let q = [qx, qy, qz];
    let mut near: Vec<(usize, f32)> = (0..g.node_count())
        .filter_map(|i| {
            let p = g.nodes[i];
            let dx = p[0] - q[0];
            let dy = p[1] - q[1];
            let dz = p[2] - q[2];
            let d = (dx * dx + dy * dy + dz * dz).sqrt();
            (d <= radius).then_some((i, d))
        })
        .collect();
    near.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    println!("{} nodes within radius:\n", near.len());
    for (i, d) in &near {
        let p = g.nodes[*i];
        println!(
            "node {i} ({:.0},{:.0},{:.0}) dist={d:.0} edges={}",
            p[0],
            p[1],
            p[2],
            g.adj_count(*i)
        );
        for &(nb, cost) in g.neighbors(*i) {
            let np = g.nodes[nb];
            let dz = np[2] - p[2];
            let hd = ((np[0] - p[0]).powi(2) + (np[1] - p[1]).powi(2)).sqrt();
            // Live hull-trace re-check: is this graph edge actually walkable now?
            let t = cm.trace(&p, &np, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
            let hull = if t.startsolid {
                "STARTSOLID"
            } else if t.fraction < 0.999 {
                "BLOCKED"
            } else {
                "clear"
            };
            // Stair-check re-run: for dz>STEP edges the straight hull trace is expected
            // to fail; walkable_stair is the proper test. "stair=NO" on an edge means it
            // is false by BOTH tests (neither a clear walk nor a climbable stair).
            let stair = if dz.abs() > STEP {
                let (lower, upper) = if dz > 0.0 { (p, np) } else { (np, p) };
                if walkable_stair(cm, lower, upper) {
                    "stair=OK"
                } else {
                    "stair=NO"
                }
            } else {
                "flat"
            };
            println!(
                "    -> {nb} ({:.0},{:.0},{:.0}) dz={dz:+.0} hd={hd:.0} cost={cost:.0} hull={hull} {stair}",
                np[0], np[1], np[2]
            );
        }
    }
    Ok(())
}
