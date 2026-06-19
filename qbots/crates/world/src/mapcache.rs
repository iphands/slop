//! Ahead-of-time nav graph disk cache (Plan 18 T2).
//!
//! Format (little-endian throughout):
//! ```text
//! [0..7]   magic    b"QBNAVC2"
//! [7]      version  u8
//! [8..64]  fingerprint (14 × u32, see Fingerprint)
//! [64..68] node_count  u32
//! for each node:
//!   x, y, z  f32 × 3
//! for each node:
//!   edge_count  u32
//!   for each edge: neighbor u32, cost f32
//! [cont.]  jump_count  u32
//! for each jump edge: from u32, to u32, launch_yaw f32
//! [cont.]  swim_count  u32        (Plan 39)
//! for each swim edge: from u32, to u32
//! [cont.]  water_count u32        (Plan 39)
//! for each water node: index u32
//! [cont.]  ride_count  u32        (Plan 42)
//! for each ride edge: from u32, to u32, board[3] f32, far[3] f32, dismount[3] f32, model_index u32
//! ```
//!
//! A fingerprint mismatch on load returns `None` — never an error — so callers
//! transparently fall back to live generation. The fingerprint encodes the BSP's
//! structural checksums and the generation constants so any map edit or constant
//! change auto-invalidates stale caches.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::bsp::Bsp;
use crate::build::{BRIDGE_HDIST, JUMP_SPACING, PRUNE_MAX_HD};
use crate::navgraph::{NavGraph, CONNECT_RADIUS, STAIR_MAX, STEP, SWIM_COST_FACTOR, SWIM_SPACING};

const MAGIC: &[u8; 7] = b"QBNAVC2";
// Version 2: multi-floor column probing (see navgraph::floor_waypoints_multi).
// Version 3: func_plat elevator edges + component bridging (navgraph::bridge_components)
// plus a 9th fingerprint field (BRIDGE_HDIST). Older caches are auto-rejected.
// Version 4: false-edge prune (navgraph::prune_long_blocked_edges) + a 10th fingerprint
// field (PRUNE_MAX_HD), so the fingerprint is now 40 bytes. Older caches auto-rejected.
// Version 5: generate() wider neighbour connection + an 11th fingerprint field
// (CONNECT_CELLS); fingerprint is now 44 bytes. Older caches auto-rejected.
// Version 6: elevator ride edges carry ELEVATOR_PENALTY cost (A* avoids lifts).
// Version 7: lift penalty is a runtime --lift-penalty knob + a 12th fingerprint field
// (lift_penalty_bits); fingerprint is now 48 bytes. Older caches auto-rejected.
// Version 8: prune_long_blocked_edges also drops FLAT hull-blocked edges (false
// same-level wall-crossings), not just long ones. Algorithm change → invalidate caches.
// Version 13: water nav (Plan 39) — swim edges + water-node tags serialized after the
// jump edges, plus two new fingerprint fields (SWIM_SPACING, SWIM_COST_FACTOR); the
// fingerprint is now 56 bytes. Older caches auto-rejected.
// Version 14: moving-platform ride edges (Plan 42) — func_train ride edges + RideInfo
// serialized after the water-node tags. Generation change → older caches auto-rejected.
const VERSION: u8 = 14;

/// Generation-constant + BSP-structural snapshot for cache invalidation.
#[derive(Debug, Clone, PartialEq)]
pub struct Fingerprint {
    plane_count: u32,
    leaf_count: u32,
    brush_count: u32,
    plane_hash: u32,
    grid_spacing_bits: u32,
    step_bits: u32,
    jump_spacing_bits: u32,
    /// `STAIR_MAX` encoded as f32 bits — changes to the stair-climb logic invalidate
    /// caches. Previously `_reserved: u32 = 0`; any cached file with the old zero
    /// will be a fingerprint mismatch and regenerated automatically.
    stair_max_bits: u32,
    /// `BRIDGE_HDIST` encoded as f32 bits — changing the component-bridge radius
    /// (`navgraph::bridge_components`) alters the generated graph, so it must
    /// invalidate stale caches.
    bridge_hdist_bits: u32,
    /// `PRUNE_MAX_HD` encoded as f32 bits — the false-edge prune threshold
    /// (`navgraph::prune_long_blocked_edges`) alters the generated graph, so changing
    /// it must invalidate stale caches.
    prune_max_hd_bits: u32,
    /// `CONNECT_RADIUS` (f32 bits) — generate()'s world-unit connection radius (the
    /// changes which edges generate adds, so it must invalidate stale caches.
    connect_radius_bits: u32,
    /// `lift_penalty` (f32 bits) — extra A* cost baked into elevator ride edges. A
    /// runtime knob (`--lift-penalty`), so it must be part of the cache key.
    /// TODO(elevator-hack): remove with the penalty once real lift behaviour exists.
    lift_penalty_bits: u32,
    /// `SWIM_SPACING` (f32 bits) — submerged swim-node vertical spacing (Plan 39). Changing
    /// it alters which water nodes/edges are generated, so it must invalidate stale caches.
    swim_spacing_bits: u32,
    /// `SWIM_COST_FACTOR` (f32 bits) — swim-edge cost multiplier (Plan 39). Part of the
    /// generated graph's edge costs, so changing it must invalidate stale caches.
    swim_cost_factor_bits: u32,
}

impl Fingerprint {
    /// Derive the fingerprint from a loaded BSP and the current generation constants.
    /// `lift_penalty` and `spacing` are the runtime `--lift-penalty` / `--spacing` values
    /// (part of the cache key, so different runtime params never share a cache file).
    pub fn from_bsp(bsp: &Bsp, lift_penalty: f32, spacing: f32) -> Self {
        // FNV-1a over the first min(256, plane_count) planes' normal+dist bytes
        // (16 bytes each). Any structural BSP change flips this.
        let sample_count = bsp.planes.len().min(256);
        let mut hash: u32 = 0x811c9dc5;
        for p in bsp.planes.iter().take(sample_count) {
            for &b in p.normal[0]
                .to_le_bytes()
                .iter()
                .chain(p.normal[1].to_le_bytes().iter())
                .chain(p.normal[2].to_le_bytes().iter())
                .chain(p.dist.to_le_bytes().iter())
            {
                hash ^= b as u32;
                hash = hash.wrapping_mul(0x01000193);
            }
        }
        Self {
            plane_count: bsp.planes.len() as u32,
            leaf_count: bsp.leafs.len() as u32,
            brush_count: bsp.brushes.len() as u32,
            plane_hash: hash,
            grid_spacing_bits: spacing.to_bits(),
            step_bits: STEP.to_bits(),
            jump_spacing_bits: JUMP_SPACING.to_bits(),
            stair_max_bits: STAIR_MAX.to_bits(),
            bridge_hdist_bits: BRIDGE_HDIST.to_bits(),
            prune_max_hd_bits: PRUNE_MAX_HD.to_bits(),
            connect_radius_bits: CONNECT_RADIUS.to_bits(),
            lift_penalty_bits: lift_penalty.to_bits(),
            swim_spacing_bits: SWIM_SPACING.to_bits(),
            swim_cost_factor_bits: SWIM_COST_FACTOR.to_bits(),
        }
    }

    fn write(&self, buf: &mut Vec<u8>) {
        for &v in &[
            self.plane_count,
            self.leaf_count,
            self.brush_count,
            self.plane_hash,
            self.grid_spacing_bits,
            self.step_bits,
            self.jump_spacing_bits,
            self.stair_max_bits,
            self.bridge_hdist_bits,
            self.prune_max_hd_bits,
            self.connect_radius_bits,
            self.lift_penalty_bits,
            self.swim_spacing_bits,
            self.swim_cost_factor_bits,
        ] {
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }

    fn read(data: &[u8]) -> Option<Self> {
        if data.len() < FP_BYTES {
            return None;
        }
        let mut fields = [0u32; 14];
        for (i, f) in fields.iter_mut().enumerate() {
            *f = u32::from_le_bytes(data[i * 4..i * 4 + 4].try_into().ok()?);
        }
        Some(Self {
            plane_count: fields[0],
            leaf_count: fields[1],
            brush_count: fields[2],
            plane_hash: fields[3],
            grid_spacing_bits: fields[4],
            step_bits: fields[5],
            jump_spacing_bits: fields[6],
            stair_max_bits: fields[7],
            bridge_hdist_bits: fields[8],
            prune_max_hd_bits: fields[9],
            connect_radius_bits: fields[10],
            lift_penalty_bits: fields[11],
            swim_spacing_bits: fields[12],
            swim_cost_factor_bits: fields[13],
        })
    }
}

/// Fingerprint on-disk size in bytes (14 × u32).
const FP_BYTES: usize = 56;

/// Write a nav graph to `path`. Overwrites any existing file.
pub fn save(path: &Path, graph: &NavGraph, fingerprint: &Fingerprint) -> io::Result<()> {
    let (nodes, adj, jump_triples) = graph.raw_parts();
    let mut buf: Vec<u8> = Vec::with_capacity(
        8 + FP_BYTES + 4 + nodes.len() * 12 + nodes.len() * 4 + 4 + jump_triples.len() * 12,
    );

    // Header
    buf.extend_from_slice(MAGIC);
    buf.push(VERSION);
    fingerprint.write(&mut buf);

    // Nodes
    let nc = nodes.len() as u32;
    buf.extend_from_slice(&nc.to_le_bytes());
    for n in &nodes {
        for &f in n {
            buf.extend_from_slice(&f.to_le_bytes());
        }
    }

    // Adjacency
    for edges in &adj {
        let ec = edges.len() as u32;
        buf.extend_from_slice(&ec.to_le_bytes());
        for &(nb, cost) in edges {
            buf.extend_from_slice(&(nb as u32).to_le_bytes());
            buf.extend_from_slice(&cost.to_le_bytes());
        }
    }

    // Jump edges
    let jc = jump_triples.len() as u32;
    buf.extend_from_slice(&jc.to_le_bytes());
    for (from, to, yaw) in &jump_triples {
        buf.extend_from_slice(&(*from as u32).to_le_bytes());
        buf.extend_from_slice(&(*to as u32).to_le_bytes());
        buf.extend_from_slice(&yaw.to_le_bytes());
    }

    // Swim edges + water-node tags (Plan 39).
    let (swim, water) = graph.raw_swim_and_water();
    buf.extend_from_slice(&(swim.len() as u32).to_le_bytes());
    for (from, to) in &swim {
        buf.extend_from_slice(&(*from as u32).to_le_bytes());
        buf.extend_from_slice(&(*to as u32).to_le_bytes());
    }
    buf.extend_from_slice(&(water.len() as u32).to_le_bytes());
    for &idx in &water {
        buf.extend_from_slice(&(idx as u32).to_le_bytes());
    }

    // Ride edges (Plan 42): directed (from, to) + RideInfo (board/far/dismount, model_index).
    let rides = graph.raw_rides();
    buf.extend_from_slice(&(rides.len() as u32).to_le_bytes());
    for (from, to, info) in &rides {
        buf.extend_from_slice(&(*from as u32).to_le_bytes());
        buf.extend_from_slice(&(*to as u32).to_le_bytes());
        for v in info.board.iter().chain(&info.far).chain(&info.dismount) {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        buf.extend_from_slice(&info.model_index.to_le_bytes());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(path)?;
    file.write_all(&buf)?;
    file.flush()
}

/// Load a cached nav graph from `path`. Returns `None` if the file is absent,
/// corrupt, or the fingerprint doesn't match `expected` — never an error.
pub fn load(path: &Path, expected: &Fingerprint) -> Option<NavGraph> {
    let data = fs::read(path).ok()?;
    parse(&data, expected)
}

fn parse(data: &[u8], expected: &Fingerprint) -> Option<NavGraph> {
    let mut pos = 0;

    // Magic + version
    if data.get(pos..pos + 7)? != MAGIC {
        return None;
    }
    pos += 7;
    if *data.get(pos)? != VERSION {
        return None;
    }
    pos += 1;

    // Fingerprint
    let fp = Fingerprint::read(data.get(pos..pos + FP_BYTES)?)?;
    if fp != *expected {
        return None;
    }
    pos += FP_BYTES;

    // Nodes
    let nc = read_u32(data, &mut pos)? as usize;
    let mut nodes = Vec::with_capacity(nc);
    for _ in 0..nc {
        let x = read_f32(data, &mut pos)?;
        let y = read_f32(data, &mut pos)?;
        let z = read_f32(data, &mut pos)?;
        nodes.push([x, y, z]);
    }

    // Adjacency
    let mut adj: Vec<Vec<(usize, f32)>> = Vec::with_capacity(nc);
    for _ in 0..nc {
        let ec = read_u32(data, &mut pos)? as usize;
        let mut edges = Vec::with_capacity(ec);
        for _ in 0..ec {
            let nb = read_u32(data, &mut pos)? as usize;
            let cost = read_f32(data, &mut pos)?;
            edges.push((nb, cost));
        }
        adj.push(edges);
    }

    // Jump edges
    let jc = read_u32(data, &mut pos)? as usize;
    let mut jump_triples = Vec::with_capacity(jc);
    for _ in 0..jc {
        let from = read_u32(data, &mut pos)? as usize;
        let to = read_u32(data, &mut pos)? as usize;
        let yaw = read_f32(data, &mut pos)?;
        jump_triples.push((from, to, yaw));
    }

    // Swim edges + water-node tags (Plan 39).
    let sc = read_u32(data, &mut pos)? as usize;
    let mut swim = Vec::with_capacity(sc);
    for _ in 0..sc {
        let from = read_u32(data, &mut pos)? as usize;
        let to = read_u32(data, &mut pos)? as usize;
        swim.push((from, to));
    }
    let wc = read_u32(data, &mut pos)? as usize;
    let mut water = Vec::with_capacity(wc);
    for _ in 0..wc {
        water.push(read_u32(data, &mut pos)? as usize);
    }

    // Ride edges (Plan 42).
    let rc = read_u32(data, &mut pos)? as usize;
    let mut rides = Vec::with_capacity(rc);
    for _ in 0..rc {
        let from = read_u32(data, &mut pos)? as usize;
        let to = read_u32(data, &mut pos)? as usize;
        let mut p = [0.0f32; 9];
        for slot in p.iter_mut() {
            *slot = read_f32(data, &mut pos)?;
        }
        let model_index = read_u32(data, &mut pos)?;
        rides.push((
            from,
            to,
            crate::navgraph::RideInfo {
                board: [p[0], p[1], p[2]],
                far: [p[3], p[4], p[5]],
                dismount: [p[6], p[7], p[8]],
                model_index,
            },
        ));
    }

    let mut graph = NavGraph::from_raw_with_jumps(nodes, adj, jump_triples);
    graph.set_swim_and_water(swim, water);
    graph.set_rides(rides);
    Some(graph)
}

fn read_u32(data: &[u8], pos: &mut usize) -> Option<u32> {
    let v = u32::from_le_bytes(data.get(*pos..*pos + 4)?.try_into().ok()?);
    *pos += 4;
    Some(v)
}

fn read_f32(data: &[u8], pos: &mut usize) -> Option<f32> {
    let v = f32::from_le_bytes(data.get(*pos..*pos + 4)?.try_into().ok()?);
    *pos += 4;
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navgraph::NavGraph;

    fn simple_graph() -> NavGraph {
        let mut g = NavGraph::from_raw_with_jumps(
            vec![[0.0, 0.0, 0.0], [64.0, 0.0, 0.0], [64.0, 64.0, 0.0]],
            vec![vec![(1, 64.0)], vec![(0, 64.0), (2, 90.5)], vec![(1, 90.5)]],
            vec![(0, 2, 45.0)], // jump edge from 0→2
        );
        // Swim edge 1↔2 (both directions) + node 2 tagged as water (Plan 39).
        g.set_swim_and_water(vec![(1, 2), (2, 1)], vec![2]);
        g
    }

    fn test_fingerprint() -> Fingerprint {
        Fingerprint {
            plane_count: 42,
            leaf_count: 10,
            brush_count: 5,
            plane_hash: 0xdeadbeef,
            grid_spacing_bits: crate::build::GRID_SPACING.to_bits(),
            step_bits: STEP.to_bits(),
            jump_spacing_bits: JUMP_SPACING.to_bits(),
            stair_max_bits: STAIR_MAX.to_bits(),
            bridge_hdist_bits: BRIDGE_HDIST.to_bits(),
            prune_max_hd_bits: PRUNE_MAX_HD.to_bits(),
            connect_radius_bits: CONNECT_RADIUS.to_bits(),
            lift_penalty_bits: 5000.0_f32.to_bits(),
            swim_spacing_bits: SWIM_SPACING.to_bits(),
            swim_cost_factor_bits: SWIM_COST_FACTOR.to_bits(),
        }
    }

    #[test]
    fn round_trip_in_memory() {
        let g = simple_graph();
        let fp = test_fingerprint();

        // Serialize to a temp file, read back.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.qnav");
        save(&path, &g, &fp).expect("save failed");

        let loaded = load(&path, &fp).expect("load returned None");
        assert_eq!(loaded.node_count(), 3);
        assert_eq!(loaded.edge_count(), 4); // 1+2+1 directed edges

        // Jump edge survives the round-trip.
        assert!(matches!(
            loaded.edge_kind(0, 2),
            crate::navgraph::EdgeKind::Jump { launch_yaw }
                if (launch_yaw - 45.0).abs() < 0.001
        ));

        // Swim edge + water tag survive the round-trip (Plan 39).
        assert!(loaded.is_swim_edge(1, 2));
        assert!(loaded.is_swim_edge(2, 1));
        assert!(matches!(
            loaded.edge_kind(1, 2),
            crate::navgraph::EdgeKind::Swim
        ));
        assert!(loaded.is_water_node(2));
        assert!(!loaded.is_water_node(0));
    }

    #[test]
    fn fingerprint_mismatch_returns_none() {
        let g = simple_graph();
        let fp = test_fingerprint();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.qnav");
        save(&path, &g, &fp).expect("save failed");

        let mut wrong_fp = fp.clone();
        wrong_fp.plane_count = 99;
        assert!(
            load(&path, &wrong_fp).is_none(),
            "mismatched fp must return None"
        );
    }

    #[test]
    fn missing_file_returns_none() {
        let fp = test_fingerprint();
        assert!(load(Path::new("/nonexistent/cache.qnav"), &fp).is_none());
    }

    #[test]
    fn corrupt_magic_returns_none() {
        let fp = test_fingerprint();
        let mut data = vec![0u8; 64];
        // Wrong magic
        data[..7].copy_from_slice(b"GARBAGE");
        assert!(parse(&data, &fp).is_none());
    }

    #[test]
    fn wrong_version_returns_none() {
        let g = simple_graph();
        let fp = test_fingerprint();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.qnav");
        save(&path, &g, &fp).expect("save failed");

        // Flip the version byte (offset 7).
        let mut data = fs::read(&path).unwrap();
        data[7] = 99;
        assert!(parse(&data, &fp).is_none());
    }
}
