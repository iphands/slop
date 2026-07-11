# base64 Stranded Spawn: Rescue Jump Pass + Teleporter Edges — Tracker

## Overview
- Status: 17% complete (T1 done)
- Start date: 2026-07-11
- Gate baseline: base64 45/46 (spawn[26] @ (-720,824,-520) stranded in comp 2)
- Target: base64 46/46; q2dm* sweep unchanged; bots traverse teleporters

## Resume Instructions
Read Plan 52 Context first (the diagnosis is done — do not re-derive it). Work T1→T6 in order;
T1 alone fixes the gate. `qbots nav-debug base64` is the fast verifier (spawn table at the end).
Commit at every task boundary (`task(P52-TN): …`), zero warnings, tests green.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: spawn-rescue jump pass | `world/navgraph.rs`, `world/build.rs` | done | **Deeper root cause found during impl**: the floor probe's stationary hull check startsolids on V-groove floors (point trace reaches deeper than the 32×32 hull can rest) → base64's drain duct — the room's only exit — sampled ZERO nodes, so no landing existed for ANY jump bridge. Fixed with a `hull_rest_z` fallback in `floor_waypoints_multi` (rests the hull on the slopes, like pmove). That alone flips base64 to **46/46** (nodes 45966→51781, comps 24→15). The rescue pass (RESCUE_MAX_FALL=384, stranded-spawn comps only) is shipped as designed and unit-tested; it's now a dormant safety net for future custom maps. Spot checks: q2dm1 10/10, q2dm3 7/7 (1 comp), q2dm6 8/8. |
| 2 | T2: teleporter edges | `world/build.rs`, `world/navgraph.rs` | pending | misc_teleporter + trigger_teleport → EdgeKind::Teleport |
| 3 | T3: brain Teleport legs | `brain/traverse.rs`, `ride.rs`, `zb2.rs` | pending | snap must not trip P51 watchdog |
| 4 | T4: cache VERSION + regen | `world/mapcache.rs` | pending | base64 46/46, q2dm* unchanged |
| 5 | T5: end-to-end verification | — | pending | checklist in plan |
| 6 | T6: knowledge capture + close | `context/*` | pending | distilled, pitfalls, SERIES, git mv |
