# base64 Stranded Spawn: Rescue Jump Pass + Teleporter Edges — Tracker

## Overview
- Status: 100% complete — closed 2026-07-11
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
| 2 | T2: teleporter edges | `world/build.rs`, `world/navgraph.rs` | done | One-way Teleport edge, prune-protected, TELEPORT_COST=32; base64 logs 1 edge (pit → t163); unit tests: one-way A*, missing-dest ignored |
| 3 | T3: brain Teleport legs | `brain/nav.rs`, `brains/zb2.rs` | done | No executor machinery needed — both followers steer INTO the pad center while pad-side (trigger ~16×16u < reach radius); snap = instant progress, watchdogs see arrival. Unit test drives route→pad→snap→advance |
| 4 | T4: cache VERSION + regen | `world/mapcache.rs` | done | v24 + teleport section (round-trip tested); base64 ok=1 err=0, q2dm1–8 8/8, zero rescue firings. 'No fresh cache' mystery = stale prebuilt tool binaries |
| 5 | T5: end-to-end verification | — | done | Offline + graph-side live: `generate-map-cache` base64 **ok=1 err=0 (46/46)** — the plan's original failure; q2dm1–q2dm8 sweep 8/8, zero rescue firings (no-op on stock maps, as designed); `nav-debug` spawn[26] comp=0; live scenario preflight loaded the v24 cache and confirmed all 46 spawns in component 0; A* reaches the GL room from **every** spawn; pit→dest routes carry `teleport=1` in the edge-kind composition. **Conditional item (blocked)**: observing a live bot physically drop into the room / cross the teleporter — the server rotated to q2dm3 mid-session (later runs drove base64 routes against q2dm3 geometry → identical nonsense wedge positions; `spawn-to-point --map` override skips the map-match preflight, worth a follow-up guard). Re-run `spawn-to-point --map base64 -- -2160 904 -856` once the server is back on base64. Also noted: base64 has `func_wall` brush entities live in DM that model-0 collision doesn't include — a movement-quality follow-up, not a connectivity one. |
| 6 | T6: knowledge capture + close | `context/*` | done | pitfalls: V-groove floor probe entry; distilled: base64 geometry + diagnosis fast-path + teleporter/rescue notes; SERIES done; moved to completed/ |
