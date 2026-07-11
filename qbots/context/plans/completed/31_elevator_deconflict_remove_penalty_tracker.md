# Plan 31 — Elevator de-conflict + remove lift penalty — Tracker

## Overview
- Status: **DONE (2026-07-10)** — all five tasks shipped; the hack is deleted (cache v20). Moved to `completed/`.
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


## Closeout (2026-07-10)
| Task | Status | Notes |
|---|---|---|
| T1 occupancy + pad predicates | done | `shaft_occupied` + `plat_at_bottom` (lowered wire origin −travel; top is PVS-ambiguous → wait-clear default); 2 unit tests |
| T2 wait-clear + prompt step-off | done | executor vertical branch = WaitClear/Enter/BackOff machine; standoff OUTSIDE the trigger (the root-cause fix); route continuation walks riders off the pad |
| T3 back-off/retry de-conflict | done | pinned-lift detection (pad never lifted us in 5s → leave the trigger so it can descend); jittered 2–4s retry; `EVT lift_yield reason=occupied|pinned`; unit test (occupied→standoff, clear→enter) |
| T4 delete the hack | done | `ELEVATOR_PENALTY`/`--lift-penalty`/`lift_penalty_bits` gone from world/qbots/tools/README; cache v20 (52-byte fingerprint); `elevator_todo.md` retired → pitfalls.md; all 8 caches regen clean |
| T5 live soak + ride gate | done | 10-min 8-bot q2dm1 soak: frag flow 36→73 continuous (NO deadlock), 9 rides, 1 lift_yield resolved, 0 panics; q2dm3 ride flag-free **4/4 ®** (fastest 11s — A* now routes through lifts) |
