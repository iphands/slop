# World Model (world) — Tracker

## Overview
- Status: **~90% complete** — T1–T4 done & live-verified; T5 (loader cache) deferred, T6 (final verify) essentially done.
- Start date: 2026-06-14
- Plan: `05_world_model.md`
- Depends on: Plan 04 (frame snapshots + map name from configstrings)
- Exit criterion: load a real `.bsp`, trace correctly, PVS agrees with server entities, A* pathfinds spawn→weapon.

## Resume Instructions
1. Get the exact `.bsp` the server runs (from `baseq2/maps/<map>.bsp`) — no BSP, no world.
2. Port `common/collision.c` line-for-line (trace/vis); the lump layout is `files.h:273`.
3. Nav-graph generation (T4) is original work — iterate on sampling density, record tuning in `distilled.md`.
4. Wire the real tracer into Plan 04 T5 prediction once T2 lands.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: BSP loader | `world/src/{bsp,pak}.rs` | done | IBSP v38 + 7 collision lumps; **verified live** (q2dm1/base1 from pak files) |
| 2 | T2: collision trace + contents | `world/src/collision.rs` | done | CM_RecursiveHullCheck + box/brush clip; **verified live** (q2dm1: 8 rays hit walls 288-800u) |
| 3 | T3: PVS query | `world/src/vis.rs` | done | CM_DecompressVis RLE + cluster_visible; **verified live** (q2dm1: 925 clusters, center sees 336) |
| 4 | T4: nav graph + A* | `world/src/navgraph.rs` | done | auto-gen from BSP; 1067 nodes/7708 edges; pathfinding works (34 hops, 2812u) |
| 5 | T5: discovery + cache | `world/src/loader.rs` | deferred | `Arc<World>`, file hash — not critical; BSP loading works inline in main.rs |
| 6 | T6: verify on q2dm1 | — | done | trace/vis/nav all green; pathfinding verified; ready for brain integration |
