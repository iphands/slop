# Plan 31 — Elevator de-conflict + remove lift penalty — Tracker

## Overview
- Status: 0% complete
- Start date: —
- Goal: multi-bot lift etiquette (wait-clear, prompt step-off, back-off/retry) →
  DELETE `ELEVATOR_PENALTY`/`--lift-penalty` everywhere; close `context/elevator_todo.md`.

## Resume Instructions
1. Read `31_elevator_deconflict_remove_penalty.md` + `context/elevator_todo.md` (the
   acceptance criteria) + `pitfalls.md` "func_plat elevator deadlock".
2. Blocked on Plan 46 for T2/T3 (the shared traversal executor hosts the ride machine).
3. Penalty sites: `world/{build,lib,mapcache}.rs`, `qbots/{supervisor,scenario,main}.rs`,
   `tools/navinspect.rs` (grep `lift_penalty|ELEVATOR_PENALTY|elevator-hack`).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: occupancy/at-bottom predicates | `ride.rs` | pending | pure + unit-tested |
| 2 | T2: wait-clear + prompt step-off | `traverse.rs` | pending | zero pad-dwell at top |
| 3 | T3: back-off/retry de-conflict | `traverse.rs` | pending | jittered waits |
| 4 | T4: delete the hack + VERSION bump | world/qbots/tools + docs | pending | one commit |
| 5 | T5: 10-min 8-bot soak + notes | live, `brain_notes.md` | pending | no lift deadlock |
