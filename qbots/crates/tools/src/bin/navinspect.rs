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
    // Need at least `<baseq2> <map> <mode-or-x>`. The keyword modes (heightfield, scan) parse
    // their own remaining args; the default coordinate-dump mode is length-checked below.
    if args.len() < 4 {
        eprintln!(
            "usage: navinspect <baseq2> <map> <x> <y> <z> [radius]\n\
             modes: navinspect <baseq2> <map> heightfield [cell_size]\n\
             \x20      navinspect <baseq2> <map> scan <x0> <y0> <x1> <y1> <zq> <step> <tz> [band]\n\
             e.g.   navinspect /srv/q2/baseq2 q2dm1 1519 567 472 160"
        );
        std::process::exit(2);
    }
    let baseq2 = Path::new(&args[1]);
    let map = &args[2];

    let cache = Path::new("data/mapcache");
    // Inspect a non-default grid via `QBOTS_SPACING=12 navinspect ...` (loads that
    // spacing's cache subdir), so the tool matches whatever spacing the run used.
    let spacing: f32 = std::env::var("QBOTS_SPACING")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(world::GRID_SPACING);
    // `QBOTS_LIVE=1` builds the nav graph live (`generate_map_nav`) instead of loading the
    // disk cache. Essential for inspecting a map that FAILS the connectivity gate: such a map
    // has no cache (generation refuses to write a broken graph), so the default cache-load
    // path can't open it at all. Live build skips the cache + the gate, so we can see why.
    let live = std::env::var("QBOTS_LIVE").is_ok_and(|v| v != "0" && !v.is_empty());
    let built = if live {
        eprintln!("[navinspect] QBOTS_LIVE set — building nav live (bypassing cache + gate)");
        world::generate_map_nav(baseq2, map, world::ELEVATOR_PENALTY, spacing)?
    } else {
        cached_map_nav(baseq2, map, Some(cache), world::ELEVATOR_PENALTY, spacing)?
    };
    let g = &built.graph;
    let cm = &built.cm;

    // LINETRACE mode: `navinspect <baseq2> <map> linetrace <x0> <y0> <z0> <x1> <y1> <z1>`
    // Hull-traces between two points and reports clear/blocked — to tell a real walkable
    // transition from a funnel corner-cut into a wall.
    if args[3] == "linetrace" {
        let a = [args[4].parse::<f32>()?, args[5].parse()?, args[6].parse()?];
        let b = [args[7].parse::<f32>()?, args[8].parse()?, args[9].parse()?];
        let t = cm.trace(&a, &b, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        println!(
            "hull trace ({:.0},{:.0},{:.0})→({:.0},{:.0},{:.0}): startsolid={} fraction={:.2} endpos=({:.0},{:.0},{:.0})",
            a[0], a[1], a[2], b[0], b[1], b[2], t.startsolid, t.fraction, t.endpos[0], t.endpos[1], t.endpos[2]
        );
        // Also a point trace (ignores hull width) for comparison.
        let z = [0.0f32; 3];
        let p = cm.trace(&a, &b, &z, &z, MASK_SOLID);
        println!(
            "point trace: startsolid={} fraction={:.2}",
            p.startsolid, p.fraction
        );
        return Ok(());
    }

    // HEIGHTFIELD mode: `navinspect <baseq2> <map> heightfield [cell_size]`
    // Voxelizes the collision model and prints walkable-span stats + a top-down ASCII
    // coverage map (downsampled), to eyeball that the navmesh heightfield covers the play
    // area. '#' = the cell-block has at least one walkable span.
    if args[3] == "heightfield" {
        let cell: f32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(8.0);
        let model = &built.bsp.models[0];
        let bounds = (model.mins, model.maxs);
        let params = world::VoxelParams {
            cell_size: cell,
            ..Default::default()
        };
        let t = std::time::Instant::now();
        let hf = world::Heightfield::build(cm, bounds, params);
        let ms = t.elapsed().as_millis();
        println!(
            "heightfield map={map} cell={cell} grid={}x{} build={ms}ms",
            hf.nx, hf.ny
        );
        println!(
            "  walkable spans={}  walkable columns={}/{}",
            hf.walkable_span_count(),
            hf.walkable_column_count(),
            hf.nx * hf.ny
        );
        let maxw = 120usize;
        let blk = hf.nx.div_ceil(maxw).max(1); // cells per output char (square blocks)
        let out_nx = hf.nx.div_ceil(blk);
        let out_ny = hf.ny.div_ceil(blk);
        println!("  top-down coverage ('#'=walkable; {blk}x{blk} cells/char):\n");
        for oy in (0..out_ny).rev() {
            let mut row = String::new();
            for ox in 0..out_nx {
                let mut any = false;
                'blk: for dy in 0..blk {
                    for dx in 0..blk {
                        let ix = ox * blk + dx;
                        let iy = oy * blk + dy;
                        if ix < hf.nx && iy < hf.ny && !hf.columns[iy * hf.nx + ix].is_empty() {
                            any = true;
                            break 'blk;
                        }
                    }
                }
                row.push(if any { '#' } else { ' ' });
            }
            println!("{row}");
        }
        return Ok(());
    }

    // NAVMESH mode: `navinspect <baseq2> <map> navmesh [cell_size]`
    // Builds heightfield → polygon navmesh and reports poly/portal counts, component count,
    // and whether all DM spawns land in ONE component (the navmesh analog of
    // check_spawn_connectivity). The spike's Phase 2 validation.
    if args[3] == "navmesh" {
        let cell: f32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(16.0);
        let model = &built.bsp.models[0];
        let bounds = (model.mins, model.maxs);
        let params = world::VoxelParams {
            cell_size: cell,
            ..Default::default()
        };
        let erode: u32 = std::env::var("QBOTS_ERODE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let t = std::time::Instant::now();
        let mut hf = world::Heightfield::build(cm, bounds, params);
        let drops = hf.find_drops(cm);
        hf.erode(erode);
        let mut mesh = world::NavMesh::build(&hf, params.walkable_climb, Some(cm));
        mesh.add_drops(&drops);
        let ms = t.elapsed().as_millis();
        let edges: usize = mesh.adj.iter().map(Vec::len).sum();
        let comps = mesh.components();
        println!(
            "navmesh map={map} cell={cell} polys={} portals={} components={} build={ms}ms",
            mesh.polys.len(),
            edges / 2,
            comps.len()
        );
        // Spawn connectivity: map each DM spawn to its nearest poly + component.
        let mut comp_of = vec![usize::MAX; mesh.polys.len()];
        for (ci, c) in comps.iter().enumerate() {
            for &p in c {
                comp_of[p] = ci;
            }
        }
        let mut spawn_comps = std::collections::BTreeMap::new();
        let mut unmapped = 0;
        for (si, sp) in built.spawn_origins.iter().enumerate() {
            match mesh.nearest_poly(*sp) {
                Some(p) => {
                    let c = comp_of[p];
                    *spawn_comps.entry(c).or_insert(0) += 1;
                    println!(
                        "  spawn[{si}] ({:.0},{:.0},{:.0}) → poly {p} comp {c} (size {})",
                        sp[0],
                        sp[1],
                        sp[2],
                        comps[c].len()
                    );
                }
                None => unmapped += 1,
            }
        }
        println!(
            "  {} DM spawns → components {:?} (unmapped={unmapped})",
            built.spawn_origins.len(),
            spawn_comps
        );
        let biggest = comps.iter().map(Vec::len).max().unwrap_or(0);
        println!(
            "  largest component = {biggest} polys ({:.0}% of mesh)",
            100.0 * biggest as f32 / mesh.polys.len().max(1) as f32
        );
        if spawn_comps.len() == 1 && unmapped == 0 {
            println!("  OK: all spawns in ONE component");
        } else {
            println!(
                "  WARN: spawns split across {} components",
                spawn_comps.len()
            );
        }
        return Ok(());
    }

    // NAVQUERY mode: `navinspect <baseq2> <map> navquery <x> <y> <z> [cell]`
    // Builds the navmesh and reports what nearest_poly returns for the point + every poly whose
    // rectangle covers that cell column (their heights + components), so a z-level mismatch
    // between where a bot stands and where the mesh routes it is visible.
    if args[3] == "navquery" {
        let q = [args[4].parse::<f32>()?, args[5].parse()?, args[6].parse()?];
        let cell: f32 = args.get(7).and_then(|s| s.parse().ok()).unwrap_or(16.0);
        let model = &built.bsp.models[0];
        let params = world::VoxelParams {
            cell_size: cell,
            ..Default::default()
        };
        let hf = world::Heightfield::build(cm, (model.mins, model.maxs), params);
        let mesh = world::NavMesh::build(&hf, params.walkable_climb, Some(cm));
        let comps = mesh.components();
        let mut comp_of = vec![usize::MAX; mesh.polys.len()];
        for (ci, c) in comps.iter().enumerate() {
            for &p in c {
                comp_of[p] = ci;
            }
        }
        println!(
            "navquery @ ({:.0},{:.0},{:.0}) cell={cell}",
            q[0], q[1], q[2]
        );
        match mesh.nearest_poly(q) {
            Some(p) => {
                let c = mesh.poly_center(p);
                println!(
                    "  nearest_poly = {p} center=({:.0},{:.0},{:.0}) Δz={:.0} comp={} (size {})",
                    c[0],
                    c[1],
                    c[2],
                    c[2] - q[2],
                    comp_of[p],
                    comps[comp_of[p]].len()
                );
            }
            None => println!("  nearest_poly = NONE (off-mesh)"),
        }
        // All heightfield spans in this column (the raw walkable Z-levels).
        let ix = ((q[0] - hf.min[0]) / hf.cell_size).floor() as i64;
        let iy = ((q[1] - hf.min[1]) / hf.cell_size).floor() as i64;
        if ix >= 0 && iy >= 0 && (ix as usize) < hf.nx && (iy as usize) < hf.ny {
            let spans = &hf.columns[iy as usize * hf.nx + ix as usize];
            println!("  heightfield spans (origin Z) at this cell: {spans:?}");
        }
        return Ok(());
    }

    // NAVPATH mode: `navinspect <baseq2> <map> navpath <sx> <sy> <sz> <gx> <gy> <gz> [cell]`
    // Builds the navmesh, plans a funnel path start→goal, and prints the polyline + per-segment
    // hull-clearance check. Validates Phase 3 (and e.g. RL reachability).
    if args[3] == "navpath" {
        let s = [args[4].parse()?, args[5].parse()?, args[6].parse()?];
        let goal = [args[7].parse()?, args[8].parse()?, args[9].parse()?];
        let cell: f32 = args.get(10).and_then(|s| s.parse().ok()).unwrap_or(16.0);
        let radius: f32 = args.get(11).and_then(|s| s.parse().ok()).unwrap_or(16.0);
        let model = &built.bsp.models[0];
        let params = world::VoxelParams {
            cell_size: cell,
            ..Default::default()
        };
        let hf = world::Heightfield::build(cm, (model.mins, model.maxs), params);
        let mesh = world::NavMesh::build(&hf, params.walkable_climb, Some(cm));
        match mesh.path(s, goal, radius) {
            Some(path) => {
                let mut total = 0.0;
                let mut clear = true;
                for w in path.windows(2) {
                    let d = ((w[1][0] - w[0][0]).powi(2)
                        + (w[1][1] - w[0][1]).powi(2)
                        + (w[1][2] - w[0][2]).powi(2))
                    .sqrt();
                    total += d;
                    let t = cm.trace(&w[0], &w[1], &HULL_MINS, &HULL_MAXS, MASK_SOLID);
                    if t.startsolid || t.fraction < 1.0 {
                        clear = false;
                    }
                }
                println!(
                    "navpath OK: {} points, length {total:.0}u, all-segments-hull-clear={clear}",
                    path.len()
                );
                for p in &path {
                    println!("  ({:.0},{:.0},{:.0})", p[0], p[1], p[2]);
                }
            }
            None => println!("navpath: NO PATH (start or goal off-mesh, or disconnected)"),
        }
        return Ok(());
    }

    // SCAN mode: `navinspect <baseq2> <map> scan <x0> <y0> <x1> <y1> <zq> <step> <tz> <band>`
    // Floor-probes a grid and prints a heatmap of where walkable floor near `tz` exists and
    // whether it is SAMPLED (a nav node within `step`). 'X' = floor in band + node nearby;
    // 'o' = floor in band but NO node (an under-sampled walkable surface — e.g. the RL ledge);
    // '·' = floor exists but at a different level; ' ' = void. Used to map narrow ledges.
    if args[3] == "scan" {
        let x0: f32 = args[4].parse()?;
        let y0: f32 = args[5].parse()?;
        let x1: f32 = args[6].parse()?;
        let y1: f32 = args[7].parse()?;
        let zq: f32 = args[8].parse()?;
        let step: f32 = args[9].parse()?;
        let tz: f32 = args[10].parse()?;
        let band: f32 = args.get(11).and_then(|s| s.parse().ok()).unwrap_or(24.0);
        println!("SCAN x[{x0}..{x1}] y[{y0}..{y1}] zq={zq} step={step} target_z={tz}±{band}");
        println!("  X=floor-in-band+node  o=floor-in-band NO node(unsampled)  ·=other floor  (space)=void\n");
        let mut y = y1;
        while y >= y0 {
            let mut row = format!("y={y:>5} ");
            let mut x = x0;
            while x <= x1 {
                let top = [x, y, zq + 64.0];
                let bot = [x, y, zq - 600.0];
                let d = cm.trace(&top, &bot, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
                let ch = if d.fraction >= 1.0 && !d.startsolid {
                    ' ' // void
                } else {
                    let fz = d.endpos[2];
                    if (fz - tz).abs() <= band {
                        // floor at target level — is there a nav node within `step`?
                        let near = g.nearest(&[x, y, fz]).is_some_and(|ni| {
                            let np = g.nodes[ni];
                            (np[0] - x).powi(2) + (np[1] - y).powi(2) <= step * step
                                && (np[2] - fz).abs() <= band
                        });
                        if near {
                            'X'
                        } else {
                            'o'
                        }
                    } else {
                        '·'
                    }
                };
                row.push(ch);
                x += step;
            }
            println!("{row}");
            y -= step;
        }
        return Ok(());
    }

    // Default coordinate-dump mode needs <x> <y> <z>.
    if args.len() < 6 {
        eprintln!("usage: navinspect <baseq2> <map> <x> <y> <z> [radius]  (or a keyword mode: heightfield | scan)");
        std::process::exit(2);
    }
    let qx: f32 = args[3].parse()?;
    let qy: f32 = args[4].parse()?;
    let qz: f32 = args[5].parse()?;
    let radius: f32 = args.get(6).map(|s| s.parse()).transpose()?.unwrap_or(128.0);

    println!(
        "map={map} nodes={} query=({qx},{qy},{qz}) radius={radius}",
        g.node_count()
    );

    // Path mode: if a goal point (gx gy gz) is given after the radius, print the A*
    // path from the query point to it (node coords + dz/hd + live hull/stair re-check),
    // so a route that funnels bots through a false/unfollowable edge is visible.
    if args.len() >= 10 {
        let gx: f32 = args[7].parse()?;
        let gy: f32 = args[8].parse()?;
        let gz: f32 = args[9].parse()?;
        let s = g.nearest(&[qx, qy, qz]).ok_or("no start node")?;
        let t = g.nearest(&[gx, gy, gz]).ok_or("no goal node")?;
        println!(
            "PATH from node {s} {:?} to node {t} {:?}",
            g.nodes[s].map(|v| v as i32),
            g.nodes[t].map(|v| v as i32)
        );
        match g.path(s, t) {
            None => println!("  NO PATH"),
            Some(p) => {
                for w in p.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    let pa = g.nodes[a];
                    let pb = g.nodes[b];
                    let dz = pb[2] - pa[2];
                    let hd = ((pb[0] - pa[0]).powi(2) + (pb[1] - pa[1]).powi(2)).sqrt();
                    let tr = cm.trace(&pa, &pb, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
                    let hull = if tr.startsolid || tr.fraction < 0.999 {
                        "BLOCKED"
                    } else {
                        "clear"
                    };
                    let stair = if dz.abs() > STEP {
                        let (lo, hi) = if dz > 0.0 { (pa, pb) } else { (pb, pa) };
                        if walkable_stair(cm, lo, hi) {
                            " stair=OK"
                        } else {
                            " stair=NO"
                        }
                    } else {
                        ""
                    };
                    let mark = if hull == "BLOCKED" { "  <<<" } else { "" };
                    println!(
                        "  {a} ({:.0},{:.0},{:.0}) -> {b} ({:.0},{:.0},{:.0}) dz={dz:+.0} hd={hd:.0} {hull}{stair}{mark}",
                        pa[0], pa[1], pa[2], pb[0], pb[1], pb[2]
                    );
                }
            }
        }
        return Ok(());
    }

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

    // Floor-walkability probe at the query column: trace straight down to find the floor,
    // then test whether a player hull can stand there (not startsolid). This distinguishes
    // "the grid just didn't sample this walkable surface" (floor present + hull fits) from
    // "there is no walkable floor here" (collision-model / BSP gap).
    {
        let top = [qx, qy, qz + 64.0];
        let bot = [qx, qy, qz - 512.0];
        let down = cm.trace(&top, &bot, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        if down.fraction >= 1.0 && !down.startsolid {
            println!("FLOOR PROBE @ ({qx},{qy},{qz}): NO floor within 512u below — void/wall");
        } else if down.startsolid {
            println!("FLOOR PROBE @ ({qx},{qy},{qz}): startsolid (query point inside geometry)");
        } else {
            let fz = down.endpos[2];
            let stand = [qx, qy, fz];
            let st = cm.trace(&stand, &stand, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
            let walk = if st.startsolid {
                "HULL BLOCKED"
            } else {
                "hull fits (WALKABLE)"
            };
            println!("FLOOR PROBE @ ({qx},{qy},{qz}): floor at z={fz:.0} ({walk})");
        }
    }

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
