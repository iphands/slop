# world — Reconstructed Map Model

**The world a gamecode bot gets for free via `gi.trace()` — built from scratch by an
external client.**

> **The Hard Part:** Classic Q2 bots run *inside* the server as `gamex86.dll` plugins
> with perfect, omniscient knowledge of geometry, entities, and BSP. qbots is *external*
> — it sees only UDP packets. `world` rebuilds the map model from the `.bsp` file.

---

## What This Is

`world` parses a Quake 2 `.bsp` (loose or from a `.pak`) and constructs:

- **Collision Model** — brush-based trace against solid geometry (the `gi.trace()`
  replacement).
- **PVS (Potentially-Visible-Set)** — cluster visibility from the BSP's visibility
  lumps.
- **Nav Graph** — waypoint graph sampled from walkable surfaces, with edges for
  stairs, jumps, and elevators.
- **Navmesh** — walkable polygon mesh with funnel pathing (Recast-style, optional).

**Built once per map, cached to disk, shared read-only across all bots.**

---

## Quick Start

```rust
use world::{Bsp, CollisionModel, NavGraph};

// Load a BSP from disk or a .pak
let bsp = Bsp::load("baseq2/maps/q2dm1.bsp")?;

// Build the collision model
let cm = CollisionModel::from_bsp(&bsp);

// Generate a nav graph (grid-sampled waypoints)
let graph = NavGraph::generate(&bsp, &cm, 24.0)?;

// Cache it for later
world::mapcache::save(&graph, "data/mapcache/24/q2dm1.qnav")?;
```

See [`src/bsp.rs`](src/bsp.rs), [`src/collision.rs`](src/collision.rs), and
[`src/navgraph.rs`](src/navgraph.rs) for the full API.

**Quick Troubleshooting:**

| Error | Fix |
|-------|-----|
| `map not found` | Check `baseq2` path in config; ensure `.bsp` exists |
| `cache not found` | Run `qbots generate-map-cache --map <name>` |
| `nav graph disconnected` | Run `qbots nav-debug <map>` to diagnose |

---

## Core Concepts

### BSP Parsing

The `.bsp` format is a binary file with **lumps** (arrays of structures):

- **Planes** — infinite planes for BSP node splits.
- **Nodes** — binary space partition tree.
- **Leafs** — faceless volumes with PVS data.
- **Brushes** — convex hulls of planes (collision geometry).
- **Entities** — key/value pairs (spawn points, weapons, items).

See [`src/bsp.rs`](src/bsp.rs) for the lump structures.

### Collision Trace

The collision model is a **BSP traversal** that tests a hull against brushes:

```rust
let trace = cm.trace(start, end, MASK_SOLID);
if trace.fraction < 1.0 {
    // Hit something — trace.endpos is the impact point
}
```

Used for:
- **Nav graph generation** — testing if edges are walkable.
- **LOS checks** — determining if two points see each other.
- **Wall probes** — detecting when a bot is stuck.

### Nav Graph vs. Navmesh

Two navigation backends, both built from the same BSP:

### Nav Graph (A* over waypoints)

A **waypoint graph** sampled at regular intervals (default 24 units):

- **Nodes** — walkable positions (ground-checked, not in solid).
- **Edges** — walkable connections (direct trace, stairs, jumps).
- **Ride info** — elevator/lift edges (bots can ride moving platforms).

**Use when:** You want fast generation, simple paths. Use `--navmode astar`.

### Navmesh (A* over polygons)

An alternative **polygon-based** navigation:

- **Heightfield** — voxel representation of walkable surfaces.
- **Contour** — outer boundaries of walkable areas.
- **PolyMesh** — walkable polygons with connection info.
- **Funnel** — string-pull algorithm to straighten paths.

**Use when:** You want smoother paths, better cornering. Use `--navmode navmesh`.

See [`src/navgraph.rs`](src/navgraph.rs) and [`src/navmesh/`](src/navmesh/) for
the implementations.

---

## Map Caching

Nav graphs are **expensive to generate** (seconds per map). Cache them:

```bash
qbots generate-map-cache --map q2dm1 --spacing 24
qbots spawn-to-spawn --map q2dm1 --spacing 24  # Loads cache, doesn't regenerate
```

Caches live in `data/mapcache/<spacing>/` and are **gitignored**.

---

## Tunable Parameters

These constants control nav graph generation. Change them only if you understand
the trade-offs:

```rust
// Nav graph spacing (units) — finer = more nodes, slower pathfinding
pub const GRID_SPACING: f32 = 24.0;

// Stair height threshold (units) — max step height for walkable edges
pub const STAIR_MAX: f32 = 24.0;

// Jump spacing (units) — for gap-crossing nodes
pub const JUMP_SPACING: f32 = 48.0;

// Elevator penalty (A* cost bias) — discourages lift usage (temporary hack)
pub const ELEVATOR_PENALTY: f32 = 5000.0;
```

**Note:** To change the spacing, regenerate the cache with `--spacing <n>`.

---

## Testing

```bash
cargo test  # BSP loader, collision trace, nav graph connectivity
```

The BSP loader has **fuzz-tested** against all 8 `q2dm*` maps and several SP maps.

---

## Sources

| Feature | yquake2 Source |
|---------|----------------|
| BSP format | `common/filesystem.c`, `refresh/r_light.c` |
| BSP lumps | `doc/` (protocol/BSP docs) |
| Collision | `client/cl_main.c` (gi.trace implementation) |
| PVS | `server/sv_phs.c` (Potentially-Visible-Set) |
| Nav graph | Inspired by 3ZB2 `.chn` / Eraser `.rt2` route files |

---

## What This Is NOT

- **No runtime learning** — nav graphs are generated once, not updated dynamically
  (Eraser-style learning is not implemented).
- **No entity tracking** — that's `client/` + `brain/`'s job.
- **No path execution** — the brain decides where to go; `world/` just provides
  the map and navigation.

---

## License

MIT / Apache-2.0 (same as the rest of qbots).
