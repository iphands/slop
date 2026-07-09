# q2dm nav connectivity: hull-valid routes + residual gaps — Tracker

## Overview
- Status: ~50% complete (connectors shipped 2026-06-19; hull-valid routes + q2dm6/7 remain)
- Start date: 2026-06-18 (revised scope 2026-07-09)
- Per-map now: q2dm1/2/4/5/8 full; q2dm3 7/7; q2dm6 7/8; q2dm7 4/6

## Resume Instructions
1. Read `35_q2dm_nav_connectivity_regression.md` (revised 2026-07-09) — the bisect-era tasks
   are gone; scope is now hull-valid bridges (T1/T2) + q2dm6/7 residuals (T3) + regen (T4).
2. Known-bad reference edge: q2dm3 `(-121,-161,216) → (191,-329,216)` — 354u "Walk" bridge,
   hull trace fraction 0.07, point trace clear (see `context/brain_notes.md` 2026-06-19 tail).
3. Diagnostics: `navinspect <map> compgaps|gpath`, `spawn-to-point <x> <y> <z>`,
   `QBOTS_NO_PRUNE=1`, `QBOTS_OBSERVE_MOVERS=1`.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 0 | Connector mechanisms (ladders, rides, jump bridges) | `world/build.rs`, `navgraph.rs` | done | shipped 2026-06-19; q2dm3 3/7→7/7 |
| 1 | T1: hull-validate bridge/seed edges + regression test | `navgraph.rs`, `world/tests/` | pending | |
| 2 | T2: split long bridges / resample q2dm3 upper level | `navgraph.rs`, `build.rs` | pending | far-spawn quad ≥3/4 is the gate |
| 3 | T3: q2dm6 (7/8) + q2dm7 (4/6) residuals | per diagnosis | pending | q2dm7 target ≥5/6 |
| 4 | T4: regen all q2dm* + live spot-checks + notes | `mapcache.rs`, live | pending | VERSION bump |

## History
- 2026-06-19: root cause = missing connectors, not `walkable_stair`. Ladder + ride + jump-down
  bridge edges landed (see SERIES + git log P35). Quad reached from spawn3 (Plan 43).
  Far-spawn route reliability deferred by user decision.
- 2026-07-09: user directive (human-like map navigation from anywhere) re-opens far-spawn
  scope; plan revised around hull-valid routes.
