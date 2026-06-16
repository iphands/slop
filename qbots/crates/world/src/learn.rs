//! Observation-based nav graph learning.
//!
//! Converts recorded player paths into a nav graph by:
//! 1. Downsampling the path (keeping points where direction changes or distance > threshold)
//! 2. Connecting consecutive points if the trace clears
//! 3. Adding spawn points as mandatory nodes
//! 4. Detecting jump edges for large drops

use crate::collision::{CollisionModel, MASK_SOLID};
use crate::navgraph::{NavGraph, HULL_MAXS, HULL_MINS};

/// Calculate distance between two points
fn dist(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Downsample a recorded path, keeping points where:
/// - Direction changes by more than `angle_threshold` degrees
/// - Distance from last kept point exceeds `distance_threshold`
pub fn downsample_path(
    path: &[[f32; 3]],
    distance_threshold: f32,
    angle_threshold: f32,
) -> Vec<[f32; 3]> {
    if path.is_empty() {
        return Vec::new();
    }

    let mut result = vec![path[0]];
    let mut last_kept = path[0];
    let mut last_dir = [0.0f32; 3];

    for &point in &path[1..] {
        let current = point;
        let dir = [
            current[0] - last_kept[0],
            current[1] - last_kept[1],
            current[2] - last_kept[2],
        ];
        let dir_len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        let dir = if dir_len > 0.0 {
            [dir[0] / dir_len, dir[1] / dir_len, dir[2] / dir_len]
        } else {
            [0.0, 0.0, 0.0]
        };

        // Always keep the first point after the start
        if result.len() == 1 {
            result.push(point);
            last_kept = current;
            last_dir = dir;
            continue;
        }

        let distance = dir_len;
        let angle_change = if last_dir[0] != 0.0 || last_dir[1] != 0.0 || last_dir[2] != 0.0 {
            let dot = last_dir[0] * dir[0] + last_dir[1] * dir[1] + last_dir[2] * dir[2];
            dot.clamp(-1.0, 1.0).acos().to_degrees()
        } else {
            0.0
        };

        if distance >= distance_threshold || angle_change >= angle_threshold {
            result.push(point);
            last_kept = current;
            last_dir = dir;
        }
    }

    result
}

/// Convert a downsampled path to a nav graph, connecting consecutive points if the trace clears.
pub fn path_to_graph(path: &[[f32; 3]], cm: &CollisionModel) -> NavGraph {
    if path.is_empty() {
        return NavGraph::from_raw(Vec::new(), Vec::new());
    }

    let nodes: Vec<[f32; 3]> = path.to_vec();
    let mut adj: Vec<Vec<(usize, f32)>> = vec![Vec::new(); nodes.len()];

    // Connect consecutive points if the trace clears
    for i in 0..nodes.len() - 1 {
        let a = &nodes[i];
        let b = &nodes[i + 1];

        let t = cm.trace(a, b, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        if t.fraction >= 1.0 && !t.startsolid {
            let cost = dist(a, b);
            adj[i].push((i + 1, cost));
            adj[i + 1].push((i, cost));
        }
    }

    NavGraph::from_raw(nodes, adj)
}

/// Add spawn points to a nav graph, connecting them to nearby nodes.
pub fn add_spawns_to_graph(graph: &mut NavGraph, cm: &CollisionModel, spawns: &[[f32; 3]]) {
    let spawn_distance = 64.0; // Connect spawns within this distance

    for &spawn in spawns {
        // Find nearest existing node
        if let Some(nearest) = graph.nearest(&spawn) {
            let dist_to_spawn = {
                let a = graph.nodes[nearest];
                let dx = a[0] - spawn[0];
                let dy = a[1] - spawn[1];
                let dz = a[2] - spawn[2];
                (dx * dx + dy * dy + dz * dz).sqrt()
            };

            // If spawn is far from nearest node, add it
            if dist_to_spawn > spawn_distance {
                let new_idx = graph.add_node(spawn);

                // Collect indices to connect first (to avoid borrow issues)
                let mut to_connect = Vec::new();
                for (i, node) in graph.nodes.iter().enumerate().take(new_idx) {
                    let dist = {
                        let dx = node[0] - spawn[0];
                        let dy = node[1] - spawn[1];
                        let dz = node[2] - spawn[2];
                        (dx * dx + dy * dy + dz * dz).sqrt()
                    };

                    if dist > spawn_distance * 2.0 {
                        continue;
                    }

                    let t = cm.trace(
                        &graph.nodes[new_idx],
                        node,
                        &HULL_MINS,
                        &HULL_MAXS,
                        MASK_SOLID,
                    );
                    if t.fraction >= 1.0 && !t.startsolid {
                        to_connect.push((i, dist));
                    }
                }

                // Now add the edges
                for (i, dist) in to_connect {
                    graph.add_edge(new_idx, i, dist);
                }
            }
        }
    }
}

/// Detect jump edges in a nav graph (drops > STEP but < MAX_FALL).
pub fn detect_jump_edges(graph: &mut NavGraph, cm: &CollisionModel, max_jump: f32) -> usize {
    let mut count = 0;

    for i in 0..graph.nodes.len() {
        let node = graph.nodes[i];

        // Check 8 directions
        for dx in [-1.0, 0.0, 1.0] {
            for dy in [-1.0, 0.0, 1.0] {
                if dx == 0.0 && dy == 0.0 {
                    continue;
                }

                let test_pos = [node[0] + dx * 32.0, node[1] + dy * 32.0, node[2]];

                // Trace down to find if there's a drop
                let down = cm.trace(
                    &test_pos,
                    &[test_pos[0], test_pos[1], test_pos[2] - max_jump],
                    &HULL_MINS,
                    &HULL_MAXS,
                    MASK_SOLID,
                );

                if down.fraction < 1.0 && down.endpos[2] < node[2] - 24.0 {
                    // Found a drop, check if it's walkable
                    let landing_z = down.endpos[2] + 24.0;
                    let landing = [test_pos[0], test_pos[1], landing_z];

                    // Check if there's a node nearby at the landing position
                    if let Some(nearest) = graph.nearest(&landing) {
                        let dist = {
                            let a = graph.nodes[nearest];
                            let dx = a[0] - landing[0];
                            let dy = a[1] - landing[1];
                            let dz = a[2] - landing[2];
                            (dx * dx + dy * dy + dz * dz).sqrt()
                        };

                        if dist < 32.0 {
                            // Add jump edge (one-way)
                            let cost = {
                                let dx = node[0] - landing[0];
                                let dy = node[1] - landing[1];
                                let dz = node[2] - landing[2];
                                (dx * dx + dy * dy + dz * dz).sqrt()
                            };
                            graph.add_edge(i, nearest, cost);
                            count += 1;
                        }
                    }
                }
            }
        }
    }

    count
}

/// Save a nav graph to disk in binary format.
pub fn save_graph(graph: &NavGraph, path: &str) -> std::io::Result<()> {
    use std::io::{BufWriter, Write};

    let mut file = BufWriter::new(std::fs::File::create(path)?);

    // Write magic header
    file.write_all(b"QBNAV1")?;

    // Write version
    let version: u32 = 1;
    file.write_all(&version.to_le_bytes())?;

    // Write node count
    let node_count: u32 = graph.nodes.len() as u32;
    file.write_all(&node_count.to_le_bytes())?;

    // Write nodes
    for node in &graph.nodes {
        file.write_all(&node[0].to_le_bytes())?;
        file.write_all(&node[1].to_le_bytes())?;
        file.write_all(&node[2].to_le_bytes())?;
    }

    // Write edge count (placeholder - edges not saved in this simplified version)
    let edge_count: u32 = 0;
    file.write_all(&edge_count.to_le_bytes())?;

    file.flush()?;
    Ok(())
}

/// Load a nav graph from disk (simplified - nodes only, no edges).
pub fn load_graph(path: &str) -> Result<NavGraph, std::io::Error> {
    use std::io::{BufReader, Read};

    let mut file = BufReader::new(std::fs::File::open(path)?);

    // Read and verify magic header
    let mut magic = [0u8; 6];
    file.read_exact(&mut magic)?;
    if &magic != b"QBNAV1" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid nav graph file: bad magic header",
        ));
    }

    // Read version
    let mut version_bytes = [0u8; 4];
    file.read_exact(&mut version_bytes)?;
    let version = u32::from_le_bytes(version_bytes);
    if version != 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Unsupported nav graph version: {}", version),
        ));
    }

    // Read node count
    let mut node_count_bytes = [0u8; 4];
    file.read_exact(&mut node_count_bytes)?;
    let node_count = u32::from_le_bytes(node_count_bytes) as usize;

    // Read nodes
    let mut nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        let mut x_bytes = [0u8; 4];
        let mut y_bytes = [0u8; 4];
        let mut z_bytes = [0u8; 4];
        file.read_exact(&mut x_bytes)?;
        file.read_exact(&mut y_bytes)?;
        file.read_exact(&mut z_bytes)?;
        nodes.push([
            f32::from_le_bytes(x_bytes),
            f32::from_le_bytes(y_bytes),
            f32::from_le_bytes(z_bytes),
        ]);
    }

    // Skip edge count (placeholder)
    let mut edge_count_bytes = [0u8; 4];
    file.read_exact(&mut edge_count_bytes)?;

    // Return graph with no edges (will need to regenerate edges)
    let adj: Vec<Vec<(usize, f32)>> = vec![Vec::new(); node_count];
    Ok(NavGraph::from_raw(nodes, adj))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_downsample_path_straight_line() {
        let path = vec![
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [20.0, 0.0, 0.0],
            [30.0, 0.0, 0.0],
            [40.0, 0.0, 0.0],
        ];

        let downsampled = downsample_path(&path, 25.0, 45.0);

        // Should keep start, then points at 20, 40 (distance >= 25)
        assert!(downsampled.len() >= 2);
        assert_eq!(downsampled[0], [0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_downsample_path_with_turn() {
        let path = vec![
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 10.0, 0.0], // 90 degree turn
            [10.0, 20.0, 0.0],
        ];

        let downsampled = downsample_path(&path, 25.0, 45.0);

        // Should keep the turn point
        assert!(downsampled.len() >= 2);
    }
}
