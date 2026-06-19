# Water Nav Graph — Tracker

## Overview
- Status: ~95% complete (T1–T7 done; T8 close-out folded into Plan 40)
- Start date: 2026-06-19
- Goal metric: A* `path(spawn → railgun [240,-384,464])` on q2dm1 returns `Some`
  (today: `NO PATH` — confirmed via `navinspect navpath`).

## Resume Instructions
1. Re-read `context/plans/RULES.md` and `39_water_nav_graph.md` in full.
2. The two water-exclusion sites are `navgraph.rs:1200` (this plan) and
   `heightfield.rs:258` (navmesh — out of scope).
3. Mirror the existing `jump_edges` machinery for `swim_edges` (search `jump_edges`,
   `current_edge_is_jump`, `EdgeKind::Jump` across `world` + `brain`).
4. Offline verification needs the pak at `vendor/baseq2` and the live build path
   (`world::generate_map_nav`, or `QBOTS_LIVE=1 navinspect`).
5. Commit after every task; bump the mapcache fingerprint (T5) before relying on caches.

## Baseline (2026-06-19, offline, q2dm1)
- Railgun entity origin: `240 -384 464` (q2dm1 `weapon_railgun`).
- `navinspect navpath 544 352 482 240 -384 464` → **NO PATH (disconnected)**.
- `navinspect navpath 1488 -48 664 240 -384 464` → **NO PATH (disconnected)**.
- Dry railgun-room floor nodes DO exist (z≈468–472) — only the water bridge is missing.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `EdgeKind::Swim` + bookkeeping + `current_edge_is_swim` | `navgraph.rs`, `nav.rs`, `nav_mode.rs` | done | mirrors `jump_edges`; trait + all Navigator impls |
| 2 | T2: water node sampling (submerged + surface) | `navgraph.rs` | done | `SWIM_SPACING`=32u; `water_waypoints_multi` |
| 3 | T3: swim↔swim 3-D edges (bypass STEP) | `navgraph.rs` | done | `SWIM_COST_FACTOR`=2.0; `try_swim_edge`; `WATER_VLINK`=48 |
| 4 | T4: dry↔water entry/exit edges + bridging | `navgraph.rs` | done | exit edge fuses railgun room; prune-protected |
| 5 | T5: cache fingerprint bump + serialization | `mapcache.rs` | done | v13, +2 fp fields, swim/water serialized |
| 6 | T6: `navinspect contents`/`watermap` modes | `navinspect.rs` | done | + `gpath` (A* graph path) |
| 7 | T7: offline proof + regression tests | `world/tests/`, `navgraph.rs` | done | synthetic channel + q2dm1 railgun (gated) |
| 8 | T8: distilled + pitfalls + move to completed | `context/*`, `SERIES.md` | pending | folded into Plan 40 close-out (Rule C) |

## Result (2026-06-19)
- **Offline proof PASSES**: `gpath 1488 -48 664 → 240 -384 464` on q2dm1 returns a path
  (33 nodes, 8 swim edges, 2660u); the other baseline spawn `544 352 482` also reaches
  (16 nodes, 8 swim edges). Both were `NO PATH` before. The railgun room joins the main
  component via the water tunnel: dive at z≈297 → swim up → surface z≈425 → exit onto the
  z≈468 railgun ledge.
- Water nav also fixed **q2dm5** connectivity (now passes the gate; was a Plan 35 failure).
- q2dm2/3/6/7 still fail the gate — the **pre-existing Plan 35 regression**, unrelated.
