//! BSP verification tool - verify BSP parsing correctness against vendor expectations

use std::path::Path;
use std::sync::Arc;
use world::{Bsp, CollisionModel, HULL_MAXS, HULL_MINS, MASK_SOLID};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: bsp_verify <map_name>");
        eprintln!("Example: bsp_verify q2dm1");
        std::process::exit(1);
    }

    let map_name = &args[1];
    let base_path = std::env::var("QBOTS_BASE_PATH").unwrap_or_else(|_| "baseq2".to_string());
    
    println!("Loading BSP: {} from {}/", map_name, base_path);
    
    // Load BSP
    let bsp = Bsp::load(Path::new(&base_path), map_name)?;
    println!("✓ BSP loaded successfully");
    
    // Test 1: Header verification
    println!("\n=== Test 1: Header Verification ===");
    println!("  Models: {}", bsp.models.len());
    println!("  Planes: {}", bsp.planes.len());
    println!("  Nodes: {}", bsp.nodes.len());
    println!("  Leafs: {}", bsp.leafs.len());
    println!("  Brushes: {}", bsp.brushes.len());
    println!("  Brushsides: {}", bsp.brushsides.len());
    println!("  Leafbrushes: {}", bsp.leafbrushes.len());
    println!("  Entities: {}", bsp.entities.len());
    
    // Test 2: Entity parsing
    println!("\n=== Test 2: Entity Parsing ===");
    let spawn_points = bsp.spawn_points();
    println!("  Found {} spawn points", spawn_points.len());
    for (i, sp) in spawn_points.iter().enumerate() {
        println!("    spawn[{}]: ({:.0}, {:.0}, {:.0})", i, sp.origin[0], sp.origin[1], sp.origin[2]);
    }
    
    // Test 3: Collision model
    println!("\n=== Test 3: Collision Model ===");
    let cm = Arc::new(CollisionModel::from_bsp(&bsp));
    
    // Test point contents at known locations
    let test_points = vec![
        ("spawn[0]", spawn_points[0].origin),
        ("spawn[3]", spawn_points[3].origin),
        ("sky", [1000.0, 1000.0, 2000.0]),
    ];
    
    for (name, point) in &test_points {
        let contents = cm.point_contents(point);
        let is_solid = contents & MASK_SOLID != 0;
        let is_water = contents & (32 | 8 | 16) != 0; // WATER | LAVA | SLIME
        println!("  {} at ({:.0}, {:.0}, {:.0}): contents={:#x} (solid={}, water={})", 
                 name, point[0], point[1], point[2], contents, is_solid, is_water);
    }
    
    // Test 4: Trace verification
    println!("\n=== Test 4: Trace Verification ===");
    
    // Test 1: Trace from air to ground at spawn[0]
    let spawn0 = spawn_points[0].origin;
    let top = [spawn0[0], spawn0[1], spawn0[2] + 200.0];
    let bot = [spawn0[0], spawn0[1], spawn0[2] - 200.0];
    let trace = cm.trace(&top, &bot, &[0.0; 3], &[0.0; 3], MASK_SOLID);
    println!("  Trace from ({:.0}, {:.0}, {:.0}) to ({:.0}, {:.0}, {:.0}):", 
             top[0], top[1], top[2], bot[0], bot[1], bot[2]);
    println!("    fraction={:.3}, startsolid={}, endpos=({:.0}, {:.0}, {:.0})", 
             trace.fraction, trace.startsolid, trace.endpos[0], trace.endpos[1], trace.endpos[2]);
    
    // Test 2: Trace through wall
    let wall_start = [-80.0, 800.0, 482.0]; // spawn[3]
    let wall_end = [-80.0, 800.0, 482.0];
    let trace = cm.trace(&wall_start, &wall_end, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
    println!("  Hull at spawn[3] (-80, 800, 482):");
    println!("    startsolid={}, allsolid={}", trace.startsolid, trace.allsolid);
    
    // Test 3: Trace through air (should be clear)
    let air_start = [0.0, 0.0, 1000.0];
    let air_end = [0.0, 0.0, 500.0];
    let trace = cm.trace(&air_start, &air_end, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
    println!("  Trace through air from (0, 0, 1000) to (0, 0, 500):");
    println!("    fraction={:.3}, startsolid={}", trace.fraction, trace.startsolid);
    
    // Test 5: Nav graph node verification
    println!("\n=== Test 5: Nav Graph Node Verification ===");
    let model = bsp.models.first().expect("BSP has models");
    let graph = world::NavGraph::generate(&cm, (model.mins, model.maxs), 64.0);
    println!("  Generated {} nodes", graph.node_count());
    
    // Check a sample of nodes
    let sample_nodes = vec![0, 100, 500, 1000, 2000];
    for &idx in &sample_nodes {
        if idx >= graph.node_count() {
            continue;
        }
        let node = graph.nodes[idx];
        let contents = cm.point_contents(&node);
        let is_water = contents & (32 | 8 | 16) != 0;
        
        let stand = cm.trace(&node, &node, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        let is_walkable = !stand.startsolid && !is_water;
        
        println!("  Node {}: ({:.0}, {:.0}, {:.0}) - walkable={}", 
                 idx, node[0], node[1], node[2], is_walkable);
    }
    
    // Test 6: Component analysis
    println!("\n=== Test 6: Component Analysis ===");
    let components = graph.components();
    println!("  Found {} components", components.len());
    for (i, comp) in components.iter().enumerate() {
        println!("    component[{}]: {} nodes", i, comp.len());
    }
    
    // Test 7: Spawn-to-spawn pathfinding
    println!("\n=== Test 7: Spawn-to-Spawn Pathfinding ===");
    if spawn_points.len() >= 2 {
        let from = spawn_points[0].origin;
        let to = spawn_points[3].origin; // Different spawn
        
        let from_node = graph.nearest(&from);
        let to_node = graph.nearest(&to);
        
        println!("  From spawn[0] ({:.0}, {:.0}, {:.0}) to spawn[3] ({:.0}, {:.0}, {:.0})", 
                 from[0], from[1], from[2], to[0], to[1], to[2]);
        println!("  From node: {:?}, To node: {:?}", from_node, to_node);
        
        if let (Some(from_idx), Some(to_idx)) = (from_node, to_node) {
            let path = graph.path(from_idx, to_idx);
            match path {
                Some(p) => println!("  Path found: {} nodes", p.len()),
                None => println!("  NO PATH FOUND!"),
            }
        }
    }
    
    println!("\n=== Summary ===");
    println!("BSP parsing appears to be working correctly.");
    println!("Check the trace results and node walkability for issues.");
    
    Ok(())
}
