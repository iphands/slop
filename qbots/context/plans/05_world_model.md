# Plan 05 — World Model (`world`)

> **Status**: pending
> **Created**: 2026-06-14
> **Depends on**: Plan 04
> **Goal**: Reconstruct the map **without the game DLL** — parse the `.bsp` file into a
> collision tree (trace + point-contents), a PVS query, and an auto-generated navigation
> graph. This is the subsystem gamecode bots get for free (`gi.trace()`, `g_edicts`) and
> qbots must build entirely itself.
> **Agent**: implementation agent (ralph-loop) | sub-agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Build `crates/world`: a Q2 BSP loader + collision/visibility queries (ported from
`common/collision.c` + `common/header/files.h`) + a nav-graph generator, exposed to the
brain through a clean trait. Shared read-only across all bots via `Arc`.

**Deliverables**:
1. `.bsp` parser: read `dheader_t` + all collision-relevant lumps.
2. Collision tree: `trace(A→B, bbox)` and `point_contents(p)` — qbots' replacement for `gi.trace()`.
3. PVS query: "can leaf A potentially see leaf B?" — matches server entity visibility.
4. Nav-graph generator: walkable waypoints + LOS-checked edges; A* pathfinding over it.
5. Map discovery: read map name from the client, locate `<map>.bsp` on local disk, cache.
6. Verification: load `q2dm1.bsp`, trace known rays, pathfind spawn→weapon.

**Estimated effort**: Large (3 days)

---

## Context

### The novel hard part

A gamecode bot calls `gi.trace()` and reads the BSP through the engine. qbots has **no
engine** — only a UDP socket and, hopefully, the map file on disk. So the entire collision
world + navigation must be reconstructed. This crate is what makes the bot-AI *ideas* from
the archive actually runnable externally (AGENTS.md §"The One Thing That Makes This Hard").

### Key Facts (confirmed against `vendor/yquake2/src/`)

- **BSP lump layout**: `common/header/files.h:273` — `LUMP_ENTITIES`(0), `LUMP_PLANES`(1),
  `LUMP_VERTEXES`(2), `LUMP_VISIBILITY`(3), `LUMP_NODES`(4), `LUMP_TEXINFO`(5), `LUMP_FACES`(6),
  `LUMP_LIGHTING`(7), `LUMP_LEAFS`(8), `LUMP_LEAFFACES`(9), `LUMP_LEAFBRUSHES`(10),
  `LUMP_EDGES`(11), `LUMP_SURFEDGES`(12), `LUMP_MODELS`(13), `LUMP_BRUSHES`(14),
  `LUMP_BRUSHSIDES`(15), `LUMP_POP`(16), `LUMP_AREAS`(17), `LUMP_AREAPORTALS`(18);
  `dheader_t` at `files.h:299`. **Collision-relevant subset**: PLANES, NODES, LEAFS,
  LEAFBRUSHES, BRUSHES, BRUSHSIDES, VISIBILITY, MODELS, AREAS, AREAPORTALS.
- **Reference implementation**: `common/collision.c` — `CM_LoadMap`, the BSP→collision-tree
  builder, plus `CM_TransformedBoxTrace` (trace), `CM_TestBoxInBrush`,
  `CM_BoxLeafnums`/`CM_BoxLeafnums_headnode` (leaf lookup), and **`CM_HeadnodeVisible`
  (`collision.c:282`) — the PVS query**. Port these; do not invent new geometry math.
- **Map source**: the `.bsp` is **never sent over the wire**. qbots must read it from disk
  — the server's `baseq2/maps/<map>.bsp` (we run alongside/qctrl) or a configured local path.
  The map name comes from configstring `CS_MODELS` (index 1 = `maps/<name>.bsp`), parsed in
  Plan 03 T4. **If the file is missing, the bot can't navigate** — degrade to a reactive
  mode (move toward visible items) and log loudly.
- **Nav formats to borrow (not the code)**: 3ZB2 `.chn`, Eraser `.rt2` are authored/learned
  waypoint graphs. We auto-generate ours from BSP; their *structure* (waypoint + reachability)
  is the inspiration, not the binary format.

---

## Step-by-Step Tasks

> **RULES.md Rule A/B**: zero warnings, clippy clean, fmt applied, tests green — **commit**
> `task(TN): <desc>` at each boundary.

### T1: BSP loader

**Files**: `crates/world/Cargo.toml` (deps: `glam`, `bytes`, `bytemuck`), `crates/world/src/bsp.rs`, `crates/world/src/lumps.rs`

**What to do**: Port the `d*_t` lump structs from `common/header/files.h` (`dheader_t`,
`dplane_t`, `dnode_t`, `dleaf_t`, `dbrush_t`, `dbrushside_t`, `dmodel_t`, `dareaportal_t`).
Read a `.bsp`: validate the header (`IDBSPHEADER` + version), slice each lump by
`fileofs`/`filelen`. Store the collision-relevant lumps. Reject malformed files with `Err`.

**Commit**: `task(T1): parse BSP header and collision-relevant lumps`

### T2: Collision tree — trace + point_contents

**Files**: `crates/world/src/collision.rs`

**What to do**: Port `common/collision.c` into Rust: build the plane/node/leaf/brush tree
from T1's lumps (`CM_LoadMap` path). Implement `trace(start, end, mins, maxs) -> TraceHit`
(`CM_TransformedBoxTrace`) and `point_contents(p)` — qbots' `gi.trace()` replacement.
Optimize with the node-tree descent + `CM_BoxLeafnums` area gathering. Unit-test: ray
through open air = `fraction == 1.0`; ray into a wall = `fraction < 1.0` + correct plane.

**Commit**: `task(T2): port collision tree trace + point_contents`

### T3: PVS / visibility query

**Files**: `crates/world/src/vis.rs`

**What to do**: Port `CM_HeadnodeVisible` (`collision.c:282`) and the `LUMP_VISIBILITY`/
`LUMP_POP` (cluster bitset) decode. Expose `cluster_visible(from, to) -> bool` and
`leaf_cluster(leaf) -> u16`. Used to (a) sanity-check that the entities the server sends us
really are in our PVS, and (b) power brain LOS heuristics without a full trace.

**Commit**: `task(T3): port PVS cluster-visibility query`

### T4: Nav-graph generation + pathfinding

**Files**: `crates/world/src/navgraph.rs`, `crates/world/src/pathfind.rs`

**What to do**: Generate a waypoint graph from the BSP: sample walkable positions (leaf
floors / brush tops at step height), link neighbors within walk distance whose connecting
segment passes a `trace` LOS check (T2). Expose `navgraph.path(from, to) -> Vec<Vec3>`
via A* (or Dijkstra). Support dynamic waypoints dropped during play (Eraser/ACE learning
style) — append-only, keyed by map hash. This is the most original code in the project.

**Commit**: `task(T4): generate nav graph from BSP and pathfind with A*`

### T5: Map discovery + caching

**Files**: `crates/world/src/loader.rs`

**What to do**: Given a map name (from the client's configstring in Plan 03/04), resolve a
local `.bsp` path (configurable search dirs: server `baseq2/maps`, a qbots cache dir).
Parse + build the collision tree + nav graph **once**, wrap in `Arc<World>` for read-only
sharing across all bots. Cache by map name + file hash so reconnects are instant.

**Commit**: `task(T5): discover, load, and cache the world per map`

### T6: Verify on a real map

**What to do**: Load `q2dm1.bsp`. Assert: floor→sky trace is open; trace into a known wall
is blocked; PVS agrees with entities the server sent during a Plan 04 capture; nav graph
connects a spawn point to a weapon (RL/SSG) and A* returns a sane path. Record nav-gen
tuning notes (sampling density, edge distance) in `context/distilled.md`.

**Commit**: `task(T6): verify world trace/vis/nav on q2dm1`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/world/src/{bsp,lumps}.rs` | BSP loader | P0 |
| `crates/world/src/collision.rs` | trace + point_contents | P0 |
| `crates/world/src/vis.rs` | PVS query | P1 |
| `crates/world/src/navgraph.rs` + `pathfind.rs` | nav graph + A* | P0 |
| `crates/world/src/loader.rs` | discovery + `Arc<World>` cache | P0 |

---

## Open Questions / Risks

1. **BSP availability.** qbots needs the exact `.bsp` the server runs; mismatches =
   invalid traces. *Mitigation*: T5 hashes the file; if it can't find a matching map, log +
   fall back to reactive mode (visible-item chasing) — never crash.
2. **Nav-graph quality.** Auto-generation can produce bad edges (through glass, off ledges).
   *Mitigation*: T4 LOS-checks every edge with the real tracer; add step-height + drop tests.
3. **Performance.** Trace is hot (called per aim/move tick × N bots). *Mitigation*: cache the
   `Arc<World>` (T5), keep traces bbox-tight; profile in Plan 07.
4. **Collision.c port fidelity.** Subtle bugs (bevels, transformed traces) are easy.
   *Mitigation*: T2 ports verbatim with golden traces; cross-check a few against the engine
   if a buildable yquake2 is available.

---

## Verification Checklist

- [ ] T1: loads a real `.bsp`; rejects truncated/bad-header files with `Err`.
- [ ] T2: `trace` open-air vs into-wall gives correct fractions; matches `CM_*` semantics.
- [ ] T3: `cluster_visible` agrees with entities the server sent during a Plan 04 capture.
- [ ] T4: nav graph edges all pass LOS; A* returns a connected path spawn→weapon.
- [ ] T5: world loaded once into `Arc`, reused across reconnects; missing map degrades gracefully.
- [ ] T6: full check passes on `q2dm1`; tuning notes recorded in `distilled.md`.

---

> **⚠️ CRITICAL REMINDERS ⚠️**
> 
> - **COMMIT AT EVERY TASK COMPLETION** — Format: `task(TN): <description>`. DO NOT WAIT!
> - **FIX ALL WARNINGS BEFORE EACH COMMIT** — `cargo clippy -- -D warnings` must pass.
> - **RUN ALL TESTS BEFORE EACH COMMIT** — `cargo test` must pass.
> - **MOVE COMPLETED PLANS TO `completed/` IMMEDIATELY** — When 100% done, `git mv` to `completed/`.
> - **NEVER batch multiple tasks into one commit** — One task per commit, always.
> - **RE-RULES.md BEFORE EACH TASK** — Re-read RULES.md at the start of every task to stay on track.
