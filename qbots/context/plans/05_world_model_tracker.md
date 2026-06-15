# World Model (world) — Tracker

## Overview
- Status: ~15% — T1 done & live-verified; T2–T6 (trace/PVS/nav) remain.
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
| 1 | T1: BSP loader | `world/src/{bsp,pak}.rs` | done | IBSP v38 + 7 collision lumps; **verified live** (q2dm1/base1/loose parse) |
| 2 | T2: collision trace + contents | `world/src/collision.rs` | pending | port `collision.c` CM_* |
| 3 | T3: PVS query | `world/src/vis.rs` | pending | `collision.c:282` |
| 4 | T4: nav graph + A* | `world/src/{navgraph,pathfind}.rs` | pending | auto-gen from BSP |
| 5 | T5: discovery + cache | `world/src/loader.rs` | pending | `Arc<World>`, file hash |
| 6 | T6: verify on q2dm1 | — | pending | trace/vis/nav all green |
