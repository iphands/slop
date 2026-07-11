# base64 Stranded Spawn: Rescue Jump Pass + Teleporter Edges — Tracker

## Overview
- Status: 0% complete
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
| 1 | T1: spawn-rescue jump pass | `world/navgraph.rs`, `world/build.rs` | pending | RESCUE_MAX_FALL=384, stranded-spawn comps only |
| 2 | T2: teleporter edges | `world/build.rs`, `world/navgraph.rs` | pending | misc_teleporter + trigger_teleport → EdgeKind::Teleport |
| 3 | T3: brain Teleport legs | `brain/traverse.rs`, `ride.rs`, `zb2.rs` | pending | snap must not trip P51 watchdog |
| 4 | T4: cache VERSION + regen | `world/mapcache.rs` | pending | base64 46/46, q2dm* unchanged |
| 5 | T5: end-to-end verification | — | pending | checklist in plan |
| 6 | T6: knowledge capture + close | `context/*` | pending | distilled, pitfalls, SERIES, git mv |
