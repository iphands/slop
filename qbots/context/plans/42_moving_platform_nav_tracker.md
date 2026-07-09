# Plan 42 — Moving-platform (`func_train`) nav integration — Tracker

## Overview
- Status: 95% complete — railgun AND quad both A*-reachable from all 7 spawns; only T6
  (offline gated reachability tests) remains, then move to `completed/`
- Start date: 2026-06-19

## Update 2026-07-09 (revision pass)
- **The quad IS now A*-reachable from all 7 spawns** — the "blocked on Plan 35" note below is
  stale. Fixed by the `func_train` two-height ride search (`13b08c4ae`: `*10` deck is
  corner-level z216, not bbox-top z410) + directional ladder edges (`019f99f13`). Cache is
  **v18** (stand_offset added in `4cfdd2cb8`).
- What Plan 35 still owns is far-spawn **route quality** (hull-blocked bridges), not
  reachability.
- **Remaining here: T6 only** — a gated offline test (mirror `world/tests/water_q2dm1.rs`)
  asserting q2dm3 A* spawn→quad and spawn→railgun(instance 1) paths exist and contain ≥1
  `Ride` edge. Then `git mv` plan + tracker to `completed/`.
- Goal: q2dm3 quad + loop-train railgun join the reachable nav graph via `EdgeKind::Ride`
  train edges + lift anchoring; `generate-map-cache q2dm3` succeeds.

## Outcome (2026-06-19)
- **`EdgeKind::Ride` + `RideInfo`** (board/far/dismount/model_index/vertical/board_ent/far_ent),
  side tables, prune-protection, accessors, serialization — done (cache **v16**).
- **`func_train` ride edges** — `train_corners` (path_corner chain), `add_train_edges`. Boards
  anchored to **existing solid-ground ledges** near each corner (`nearest_ground`), not the
  platform-top (which is over the pit) — so the bot waits on ground, not air. Ride edges link
  every pair of ground-bearing corners (the train visits all).
- **Lift anchoring → vertical ride edges**: `func_plat`/`func_door` lifts are now `Ride` edges
  (`vertical:true`) so the brain rides them (Plan 43) instead of "walking" an impossible
  vertical edge. (Folds Plan 34 T4 intent + starts Plan 31.)
- **Vertical jump-down floor bridge** (`bridge_components_via_jump`): fuses stacked floors that
  connect only by a drop-off — the long drops `detect_jump_edges`' 36u probe misses.
- **Result**: q2dm3 **railgun (instance 1, `(768,816,208)`) is A\*-reachable from ALL 7 spawns**
  (verified live: `can_reach=true` ×7; path = 24 walk + 5 jump + 2 ride). `generate-map-cache
  q2dm3 --allow-failures` writes the cache.
- **Quad (`item_quad (192,320,216)`) NOT yet reachable**: it sits in the upper level (comp0,
  z152-600) which has no up-route to the spawn floors in our graph — the broad q2dm3
  fragmentation that is **Plan 35**'s scope (lifts only bridge within the lower floor; the upper
  bulk has no spawn-reachable stair/lift). The train/jump work connected the railgun area but
  not the upper level. **Quad reachability depends on Plan 35.**

## Key q2dm3 facts
- Quad train `*10`: t1 (143,-296,184) ↔ t2 (143,88,184); rides z~434 (upper). Railgun loop
  trains `*3`,`*4`: corners t6…t15 at z=-120, ride-top z~16-40, cross a pit to the railgun area.
- `func_plat *2` (z-1..193) is the railgun-area lift; rideable now.
- Components: spawns in comp1(z-16..217)/comp2(z168-232)/comp3(z360); comp0(z152-600, 3017
  nodes) has NO spawn — the upper level is cut off (Plan 35).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `EdgeKind::Ride` + side tables + accessors | `navgraph.rs` | done | + board_ent/far_ent (P43) |
| 2 | T2: `func_train` corner parse + top heights | `build.rs` | done | min-corner rule verified vs vendor |
| 3 | T3: board/dismount synth + `add_train_edges` | `build.rs` | done | ground-anchored boards |
| 4 | T4: lift anchoring (railgun elevator top) | `build.rs` | done | lifts → vertical ride edges |
| 5 | T5: connectivity + cache regen + VERSION bump | `mapcache.rs` | done | v16; railgun reachable; quad→Plan 35 |
| 6 | T6: offline q2dm3 reachability (ride-edge) tests | `world/tests/` | pending | live-verified; gated unit test TODO |
| + | jump-down floor bridge (added) | `navgraph.rs`,`build.rs` | done | `bridge_components_via_jump` |
