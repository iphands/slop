//! compgaps — find where `generate()` splits the map into components that SHOULD be
//! connected. For the generate-only graph (no bridges), it computes components, then for
//! every node finds the nearest node in a DIFFERENT component within a small radius and
//! re-checks walkability (hull trace for flat, walkable_stair for steps). Pairs that are
//! close AND walkable are edges generate FAILED to add — i.e. the fragmentation is a
//! generate bug, not real map disconnection. If instead the closest inter-component pairs
//! are all far apart or genuinely blocked, the fragmentation is structural.
//!
//! Usage:
//!   cargo run -p tools --bin compgaps -- <baseq2> <map> [spacing] [radius]
//!   cargo run -p tools --bin compgaps -- vendor/baseq2 q2dm1 24 96

use std::path::Path;
use world::navgraph::{walkable_stair, HULL_MAXS, HULL_MINS};
use world::{Bsp, CollisionModel, NavGraph, MASK_SOLID, STAIR_MAX, STEP};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: compgaps <baseq2> <map> [spacing=24] [radius=96]");
        std::process::exit(2);
    }
    let baseq2 = Path::new(&args[1]);
    let map = &args[2];
    let spacing: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(24.0);
    let radius: f32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(96.0);

    // `--built` (Plan 35 T3): analyze the FULLY BUILT graph (seeds + connectors + bridges +
    // prune) instead of the raw generate-only one — the question becomes "what still separates
    // the FINAL components", answered as the nearest cross-component node pairs regardless of
    // walkability (so a structural gap needing a NEW mechanism is visible too).
    let built_mode = args.iter().any(|a| a == "--built");

    let bsp = Bsp::load(baseq2, map)?;
    let cm = CollisionModel::from_bsp(&bsp);
    let model = bsp.models.first().ok_or("BSP has no models")?;
    let bounds = (model.mins, model.maxs);

    let g = if built_mode {
        world::generate_map_nav(baseq2, map, spacing)?.graph
    } else {
        // generate-only (no seed/elevator/bridge/prune).
        NavGraph::generate(&cm, bounds, spacing)
    };
    let comps = g.components();
    println!(
        "map={map} spacing={spacing} built={built_mode} nodes={} components={} radius={radius}\n",
        g.node_count(),
        comps.len()
    );

    if built_mode {
        // Top components (node count + how many DM spawns resolve into each) + the nearest node
        // pair between each of the biggest few — the concrete junctions Plan 35 T3 must connect.
        let mut order: Vec<usize> = (0..comps.len()).collect();
        order.sort_by_key(|&c| std::cmp::Reverse(comps[c].len()));
        let spawns: Vec<[f32; 3]> = bsp.spawn_points().iter().map(|s| s.origin).collect();
        let mut comp_of = vec![usize::MAX; g.node_count()];
        for (cid, c) in comps.iter().enumerate() {
            for &n in c {
                comp_of[n] = cid;
            }
        }
        // Item pads are the playability tell: a big component with zero spawns AND zero items
        // is roof/void junk (the gate metric then mismeasures); one WITH items is a real area
        // needing a connector.
        let item_origins: Vec<[f32; 3]> = bsp
            .entities
            .iter()
            .filter(|e| {
                e.classname.starts_with("item_")
                    || e.classname.starts_with("weapon_")
                    || e.classname.starts_with("ammo_")
            })
            .filter_map(|e| e.origin())
            .collect();
        for &c in order.iter().take(4) {
            let spawn_count = spawns
                .iter()
                .filter(|s| g.nearest(s).is_some_and(|n| comp_of[n] == c))
                .count();
            let item_count = item_origins
                .iter()
                .filter(|p| g.nearest(p).is_some_and(|n| comp_of[n] == c))
                .count();
            println!(
                "component {c}: {} nodes, {spawn_count} spawns, {item_count} items",
                comps[c].len()
            );
        }
        println!();
        for a_i in 0..order.len().min(4) {
            for b_i in (a_i + 1)..order.len().min(4) {
                let (ca, cb) = (order[a_i], order[b_i]);
                let mut best: Option<(f32, usize, usize)> = None;
                for &i in &comps[ca] {
                    for &j in &comps[cb] {
                        let (p, q) = (g.nodes[i], g.nodes[j]);
                        let d =
                            ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2))
                                .sqrt();
                        if best.is_none_or(|(bd, _, _)| d < bd) {
                            best = Some((d, i, j));
                        }
                    }
                }
                if let Some((d, i, j)) = best {
                    let (p, q) = (g.nodes[i], g.nodes[j]);
                    println!(
                        "comp {ca}({}) <-> comp {cb}({}): nearest {d:.0}u  ({:.0},{:.0},{:.0}) -- ({:.0},{:.0},{:.0}) dz={:.0}",
                        comps[ca].len(),
                        comps[cb].len(),
                        p[0],
                        p[1],
                        p[2],
                        q[0],
                        q[1],
                        q[2],
                        q[2] - p[2]
                    );
                }
            }
        }
        return Ok(());
    }

    // node -> component id
    let mut comp_of = vec![usize::MAX; g.node_count()];
    for (cid, c) in comps.iter().enumerate() {
        for &n in c {
            comp_of[n] = cid;
        }
    }
    let comp_size: Vec<usize> = comps.iter().map(|c| c.len()).collect();

    let r2 = radius * radius;
    // Closest walkable inter-component pair per (small-comp) — dedup by component pair.
    let mut seen: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    let mut walkable_gaps = 0usize;
    let mut blocked_gaps = 0usize;
    let mut rows: Vec<String> = Vec::new();

    for i in 0..g.node_count() {
        let a = g.nodes[i];
        for j in (i + 1)..g.node_count() {
            if comp_of[i] == comp_of[j] {
                continue;
            }
            let b = g.nodes[j];
            let hd2 = (b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2);
            if hd2 > r2 {
                continue;
            }
            let dz = (b[2] - a[2]).abs();
            if dz > STAIR_MAX {
                continue;
            }
            // Walkability re-check. For near-flat pairs (dz <= STEP) the answer is the
            // direct hull trace alone: do NOT fall back to walkable_stair, because
            // walkable_stair with total_dz < STEP does zero step iterations and returns
            // `true` unconditionally (navgraph.rs: steps = ceil(dz/STEP) = 0) — that would
            // report wall-blocked flat pairs as "walkable" and massively over-count gaps.
            // Only real steps (dz > STEP) use the stair trace.
            let walk = if dz <= STEP {
                let t = cm.trace(&a, &b, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
                !t.startsolid && t.fraction >= 1.0
            } else {
                let (lo, hi) = if a[2] < b[2] { (a, b) } else { (b, a) };
                walkable_stair(&cm, lo, hi)
            };
            if !walk {
                blocked_gaps += 1;
                continue;
            }
            walkable_gaps += 1;
            let (ca, cb) = (comp_of[i].min(comp_of[j]), comp_of[i].max(comp_of[j]));
            if seen.insert((ca, cb)) {
                let hd = hd2.sqrt();
                rows.push(format!(
                    "  comp {ca}({}) <-> comp {cb}({}) : node {i} ({:.0},{:.0},{:.0}) -- node {j} ({:.0},{:.0},{:.0}) hd={hd:.0} dz={:.0}",
                    comp_size[ca], comp_size[cb], a[0], a[1], a[2], b[0], b[1], b[2], (b[2]-a[2])
                ));
            }
        }
    }

    println!(
        "WALKABLE inter-component pairs within {radius}u (generate MISSED these): {walkable_gaps}"
    );
    println!(
        "blocked inter-component pairs within {radius}u (genuinely separated):    {blocked_gaps}"
    );
    println!(
        "\ndistinct component-pairs that ARE walkably adjacent: {}\n",
        rows.len()
    );
    for r in rows.iter().take(40) {
        println!("{r}");
    }
    if walkable_gaps > 0 {
        println!(
            "\n=> generate() UNDER-CONNECTS: {walkable_gaps} walkable links missing. \
             Fragmentation is (at least partly) a generate bug, not map structure."
        );
    } else {
        println!("\n=> No walkable inter-component links within {radius}u: fragmentation looks structural.");
    }
    Ok(())
}
