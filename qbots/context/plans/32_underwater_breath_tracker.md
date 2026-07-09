# Plan 32 — Underwater breath — Tracker

## Overview
- Status: 0% complete
- Start date: —
- Goal: client-side air clock (Q2's 12s rule), surface-seek override in the traversal
  executor, air-budget dive gating; zero drownings.

## Resume Instructions
1. Read `32_underwater_breath.md`. Swim works (Plan 40); there is NO air model today.
2. Server truth: `vendor/yquake2/src/game/p_client.c` `P_WorldEffects` (12s air, 2/s
   escalating drown damage at waterlevel 3).
3. Blocked on Plan 46 for T2 (executor hosts the override; priority: drown-surface > ride
   > ladder > swim).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `AirClock` (pure) | `water.rs` | pending | 12s − 2s margin |
| 2 | T2: surface-seek override | `traverse.rs` | pending | path-aware surfacing |
| 3 | T3: dive gating + post-drown heal hook | `brains/main.rs`, `items.rs` | pending | |
| 4 | T4: live proof (railgun swim + forced loiter) | live, `brain_notes.md` | pending | health flat in loiter |
