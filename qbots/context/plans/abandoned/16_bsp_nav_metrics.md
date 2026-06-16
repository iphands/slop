> **ABANDONED 2026-06-16** — Metrics/baselines cited here (32u grid spacing, specific node-count targets) are stale vs current code (24u spacing in navgraph.rs). Superseded by Plan 17's verification checklist (BSP/collision) and Plan 19's verification checklist (nav graph/path quality), which carry forward the useful acceptance-criteria ideas with current numbers.

---

# Plan 16 — BSP Parsing & Nav Graph Correctness Metrics

> **Status**: pending
> **Created**: 2026-06-16
> **Depends on**: Plan 05 (world model), Plan 10 (movement test harness)
> **Goal**: Define clear, measurable metrics for BSP parsing correctness, collision model accuracy, nav graph quality, and path traversability.
> **Agent**: implementation agent (ralph-loop)

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Define measurable acceptance criteria for BSP parsing, collision model, nav graph, and path traversability.

**Deliverables**:
1. Documented metrics for BSP parsing correctness
2. Testable collision model validation criteria
3. Nav graph quality benchmarks
4. Path traversability verification procedures

**Estimated effort**: Small–Medium (half day)

---

## Context

The world model (`world/` crate) is the foundation for all bot navigation and combat. Errors in BSP parsing, collision detection, or nav graph generation cascade into:
- Bots spawning inside geometry
- Traces returning incorrect results
- Unreachable goals
- Pathfinding failures

We need **clear, measurable metrics** to verify correctness before integrating with higher-level brain logic.

### Key Facts from Implementation

1. **BSP Parsing** (`crates/world/src/bsp.rs`):
   - Parses IBSP version 38 (Yamagi Q2 / q2pro compatible)
   - Extracts entities (spawns, weapons, items) from LUMP_ENTITIES
   - Parses geometry lumps: planes, nodes, leafs, brushes, brushsides, models
   - Verified counts: q2dm1 = 2408 planes, 2250 leafs, 960 brushes

2. **Collision Model** (`crates/world/src/collision.rs`):
   - Implements `CM_RecursiveHullCheck`, `CM_ClipBoxToBrush`, `CM_PointContents`
   - Ported from `common/collision.c` with `DIST_EPSILON = 0.03125`
   - Verified on q2dm1: bounds [-256,-464,-256]..[2240,1808,1920], 8-direction wall traces hit at 288–800 units

3. **Nav Graph** (`crates/world/src/navgraph.rs`):
   - Grid sampling at 32-unit spacing over map bounds
   - Hull-trace connectivity checks (player hull: `[-16,-16,-24]..[16,16,32]`)
   - Jump-edge detection for ledge drops (Plan 14 T2)
   - Spawn seeding (`seed_spawns`), component bridging (`connect_components`)

4. **Movement Recorder** (`crates/brain/src/recorder.rs`):
   - Per-frame telemetry: position, velocity, speed, wall bumps, wrong turns, hindered frames
   - SUMMARY metrics: `reached`, `elapsed_secs`, `mean_speed`, `path_efficiency`
   - Baseline (pre-fix): mean_speed 33/11 u/s, 196/239 hindered frames, reached=0

---

## Step-by-Step Tasks

### T1: BSP Parsing Correctness Metrics

**File**: `crates/world/src/bsp.rs`

**What to do**: Define measurable criteria for BSP parsing correctness.

**Metrics**:

1. **Lump Integrity**:
   - ✅ All required lumps present: LUMP_PLANES, LUMP_NODES, LUMP_LEAFS, LUMP_BRUSHES, LUMP_BRUSHSIDES, LUMP_MODELS, LUMP_ENTITIES
   - ✅ Lump sizes match expected struct sizes:
     - `dplane_t`: 20 bytes (3×float + float + int)
     - `dnode_t`: 28 bytes (planenum + children + mins + maxs)
     - `dleaf_t`: 28 bytes (contents + cluster + area + mins + maxs + brushes)
     - `dbrush_t`: 12 bytes (firstside + numsides + contents)
     - `dbrushside_t`: 4 bytes (planenum + texinfo)
     - `dmodel_t`: 48 bytes (mins + maxs + origin + headnode + faces)

2. **Entity Extraction**:
   - ✅ `info_player_deathmatch` entities parsed with origin (fallback to `info_player_start`)
   - ✅ `weapon_*` entities parsed with origin
   - ✅ Entity fields (classname, origin, angle, spawnflags) correctly extracted
   - ✅ Garbage between entity groups ignored (verified by `parse_entities_ignores_garbage_between_groups`)

3. **Geometry Counts** (per-map baselines):
   ```
   q2dm1: 2408 planes, 2246 nodes, 2250 leafs, 960 brushes, 6802 brushsides, 3745 leafbrushes, 3 models
   base1: 8558 planes (stock SP map)
   ```
   - ✅ Non-zero counts for all geometry arrays
   - ✅ Consistent cross-references (node children point to valid nodes/leafs)

4. **Spawn Points**:
   - ✅ At least one spawn point found (DM or SP fallback)
   - ✅ Spawn origins parseable as valid 3D coordinates
   - ✅ Spawn positions are within map bounds (not in void)

**Test Plan**:
```rust
#[test]
fn bsp_load_q2dm1_geometry_counts() {
    let bsp = Bsp::load(baseq2, "q2dm1").unwrap();
    assert_eq!(bsp.planes.len(), 2408);
    assert_eq!(bsp.nodes.len(), 2246);
    assert_eq!(bsp.leafs.len(), 2250);
    assert_eq!(bsp.brushes.len(), 960);
    assert_eq!(bsp.spawn_points().len(), "expected_spawn_count");
}

#[test]
fn bsp_parse_entities_extract_spawns_and_weapons() {
    // Verified by existing: parse_entities_round_trips_spawns_and_weapons
}
```

---

### T2: Collision Model Correctness Metrics

**File**: `crates/world/src/collision.rs`

**What to do**: Define measurable criteria for trace and point_contents accuracy.

**Metrics**:

1. **Point Contents**:
   - ✅ Points inside brushes return `CONTENTS_SOLID`
   - ✅ Points in open space return `0` (empty)
   - ✅ Points in water/slime/lava return respective content flags

2. **Trace Accuracy**:
   - ✅ Point trace (mins=maxs=[0;0;0]) returns `fraction=1.0` for unobstructed paths
   - ✅ Point trace returns `fraction < 1.0` when blocked, with `endpos` at impact
   - ✅ Box trace (player hull) respects brush boundaries
   - ✅ `startsolid=true` when origin is inside solid geometry
   - ✅ `allsolid=true` when entire sweep is blocked

3. **Half-Space Validation** (for LOS tests):
   - ✅ Half-space at plane `(normal, dist)` blocks rays crossing into the back side
   - ✅ Clear rays on the front side return `fraction=1.0`
   - ✅ Verified by `half_space_blocks_across_the_plane`:
     ```
     Clear: (50,0,0)→(100,0,0) → fraction=1.0
     Blocked: (50,0,0)→(-50,0,0) → fraction≈0.5 (stops at plane)
     ```

4. **Real-Map Verification** (q2dm1):
   - ✅ Bounds: [-256,-464,-256]..[2240,1808,1920]
   - ✅ Center (992,672,832) is `is_solid=false`
   - ✅ 8 horizontal rays from center hit walls at 288–800 units (verified live)

**Test Plan**:
```rust
#[test]
fn collision_model_q2dm1_verification() {
    let bsp = Bsp::load(baseq2, "q2dm1").unwrap();
    let cm = CollisionModel::from_bsp(&bsp);
    
    // Bounds check
    assert!(!cm.is_solid(&[992.0, 672.0, 832.0]));
    
    // 8-direction wall traces
    let directions = [
        [1.0, 0.0, 0.0], [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0], [0.0, -1.0, 0.0],
        [0.707, 0.707, 0.0], [0.707, -0.707, 0.0],
        [-0.707, 0.707, 0.0], [-0.707, -0.707, 0.0],
    ];
    for dir in directions {
        let end = [
            992.0 + dir[0] * 1000.0,
            672.0 + dir[1] * 1000.0,
            832.0 + dir[2] * 1000.0,
        ];
        let trace = cm.trace(&[992.0, 672.0, 832.0], &end, &[0.0; 3], &[0.0; 3], MASK_SOLID);
        assert!(trace.fraction < 1.0, "ray should hit a wall");
        assert!(trace.fraction > 0.2, "ray should travel at least 200 units");
    }
}
```

---

### T3: Nav Graph Correctness Metrics

**File**: `crates/world/src/navgraph.rs`

**What to do**: Define measurable criteria for nav graph quality.

**Metrics**:

1. **Node Coverage**:
   - ✅ Nodes sampled on walkable floor (not in solid/void/water)
   - ✅ Grid spacing: 32 units (configurable)
   - ✅ Node count correlates with map size (q2dm1: ~1000–2000 nodes expected)

2. **Connectivity**:
   - ✅ Adjacent nodes connected if hull-trace clear and height diff ≤ STEP (24 units)
   - ✅ Jump edges added for ledge drops (STEP < drop ≤ MAX_FALL=256)
   - ✅ Spawn points seeded and connected (`seed_spawns`)
   - ✅ Disconnected components bridged (`connect_components`)

3. **Component Analysis**:
   - ✅ Single connected component for most DM maps (q2dm1 should be fully connected)
   - ✅ Multi-level maps may have multiple components; largest component should contain most spawns
   - ✅ Warning logged if spawns are in different components than the bot's starting position

4. **Pathfinding Success**:
   - ✅ A* finds paths between reachable nodes
   - ✅ `path_weighted` respects danger/popularity overlays (Plan 08)
   - ✅ `smooth_path` collapses grid zigzag (Plan 14 T1)

**Test Plan**:
```rust
#[test]
fn nav_graph_q2dm1_connectivity() {
    let bsp = Bsp::load(baseq2, "q2dm1").unwrap();
    let cm = CollisionModel::from_bsp(&bsp);
    let mut graph = NavGraph::generate(&cm, (model.mins, model.maxs), 32.0);
    graph.seed_spawns(&cm, &spawn_origins);
    graph.detect_jump_edges(&cm, 48.0);
    let added = graph.connect_components(&cm, 512.0);
    
    let comps = graph.components();
    assert_eq!(comps.len(), 1, "q2dm1 should be fully connected");
    assert!(graph.node_count() > 1000, "expected ~1000-2000 nodes");
}

#[test]
fn nav_graph_pathfinding_success_rate() {
    // Test A* between random node pairs
    // Measure: success_rate > 95% for reachable goals
}
```

---

### T4: Path Traversability Metrics

**File**: `crates/brain/src/nav.rs`, `crates/brain/src/recorder.rs`

**What to do**: Define measurable criteria for path traversability using the movement test harness.

**Metrics**:

1. **Scenario Success Rate**:
   - ✅ `spawn-to-spawn`: `reached=1` within 30s cap
   - ✅ `spawn-to-weapon`: `reached=1` within 30s cap
   - ✅ Success rate > 90% across multiple runs

2. **Movement Quality** (baseline vs. post-fix):
   | Metric | Baseline (pre-fix) | Target (post-fix) |
   |--------|-------------------|-------------------|
   | `reached` | 0 (fail) | 1 (success) |
   | `elapsed_secs` | >30 (timeout) | <20 |
   | `mean_speed` | 33/11 u/s | >200 u/s |
   | `max_speed` | ~100 u/s | >280 u/s |
   | `bumps` | >50 | <10 |
   | `wrong_turns` | >20 | <5 |
   | `hindered_frames` | 196/239 | <50 |
   | `path_efficiency` | <0.5 | >0.8 |

3. **Path Efficiency**:
   - ✅ `path_efficiency = straight_line_dist / distance_traveled`
   - ✅ Values > 0.8 indicate minimal grid zigzag
   - ✅ Values < 0.5 indicate severe path quality issues

4. **Wall Bump Rate**:
   - ✅ `bumps / elapsed_secs < 5` (fewer than 5 bumps per second)
   - ✅ Sustained grinding (>10 bumps in 5s) indicates stuck behavior

5. **Hindered Frame Rate**:
   - ✅ `hindered_frames / total_frames < 20%`
   - ✅ >50% hindered frames indicates navigation failure

**Test Plan**:
```bash
# Run scenarios and capture SUMMARY metrics
cargo run -p qbots -- spawn-to-spawn --map q2dm1 --name qb_test
cargo run -p qbots -- spawn-to-weapon rocketlauncher --map q2dm1 --name qb_test

# Expected output (post-fix):
# SUMMARY reached=1 elapsed=12.45 distance=2456 mean_speed=197 max_speed=295 bumps=3 wrong_turns=1 hindered_frames=12 path_efficiency=0.87
```

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `context/plans/16_bsp_nav_metrics.md` | This plan document | P0 |
| `crates/world/src/bsp.rs` | Add integration tests for geometry counts | P1 |
| `crates/world/src/collision.rs` | Add real-map verification tests | P1 |
| `crates/world/src/navgraph.rs` | Add component analysis tests | P1 |
| `crates/brain/src/recorder.rs` | Verify SUMMARY schema matches metrics | P1 |

---

## Open Questions / Risks

1. **Q: What are the expected node counts for different maps?**
   - A: q2dm1 (DM map, ~2000x2000 units): ~1000–2000 nodes at 32-unit spacing
   - Smaller maps (base1, SP): proportionally fewer nodes

2. **Q: How do we handle multi-level maps with disconnected components?**
   - A: `connect_components` bridges gaps; if bridges fail, spawns in different components may be unreachable. Log warnings and fall back to nearest-reachable.

3. **Q: What is an acceptable path efficiency threshold?**
   - A: >0.8 is good (minimal zigzag), 0.5–0.8 is acceptable, <0.5 indicates severe grid-walking

4. **Risk**: Nav graph generation is slow on large maps (>5s for complex maps)
   - Mitigation: Cache nav graphs per map (already implemented in `NavCache`)

---

## Verification Checklist

- [ ] T1: BSP loads q2dm1 with correct geometry counts (2408 planes, 2250 leafs, 960 brushes)
- [ ] T1: Entity parsing extracts all DM spawns and weapons with valid origins
- [ ] T2: Collision model traces hit walls at expected distances (288–800u from center)
- [ ] T2: Half-space trace blocks rays crossing the plane (fraction≈0.5)
- [ ] T3: Nav graph for q2dm1 has 1000–2000 nodes and is fully connected
- [ ] T3: Spawn seeding adds nodes at DM spawn positions
- [ ] T3: Jump edges created for ledge drops (verified by `detect_jump_edges_adds_ledge_drop`)
- [ ] T4: `spawn-to-spawn` scenario reaches goal (`reached=1`) within 30s
- [ ] T4: Post-fix metrics: mean_speed >200 u/s, hindered_frames <50, path_efficiency >0.8
- [ ] All existing tests pass (`cargo test --workspace`)
- [ ] No compiler warnings (`cargo clippy -- -D warnings`)

---

## Acceptance Criteria Summary

**BSP Parsing**:
- ✅ All geometry lumps parse without errors
- ✅ Entity extraction finds all spawns and weapons
- ✅ Lump sizes match expected struct layouts

**Collision Model**:
- ✅ Point contents correctly identifies solid vs. empty space
- ✅ Traces return accurate impact points and fractions
- ✅ Real-map verification matches expected wall distances

**Nav Graph**:
- ✅ Node coverage spans walkable areas (not void/solid)
- ✅ Connectivity ensures most nodes are reachable
- ✅ Spawn seeding and component bridging work correctly

**Path Traversability**:
- ✅ Scenarios reach goals (>90% success rate)
- ✅ Movement quality metrics exceed targets (mean_speed >200, hindered <50, efficiency >0.8)
- ✅ Wall bump and wrong-turn rates are low
