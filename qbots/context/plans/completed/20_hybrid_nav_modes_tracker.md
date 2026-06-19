# Hybrid Navigation Modes — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-06-18
- Completed: 2026-06-18
- Deliverable: 4 `hybrid-*` `--mode` backends + shared factory + accessors

## Resume Instructions
Each mode is a `Navigator` impl in `crates/brain/src/hybrid/` owning a `NavigationDriver`
+ `NavmeshDriver`. Wire both dispatch sites (`main.rs:615`, `scenario.rs:261`) through a
factory in the qbots crate. Verify with the Plan 10 movement scenarios per mode. Commit
at every task boundary (`task(TN): …`).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: planner accessors | `nav.rs`, `navmesh_driver.rs` | done | `planned_cost`/`jump_count`/`path`/`planned_len` (+ `NavMesh::empty()`) |
| 2 | T2: hybrid scaffold | `hybrid/mod.rs`, `lib.rs` | done | `Sub` + `goal_to_pos`/`goal_key`; landed with T3 |
| 3 | T3: hybrid-fallback | `hybrid/fallback.rs`, dispatch | done | A* primary, navmesh on stuck; factory wires both dispatch sites |
| 4 | T4: hybrid-race | `hybrid/race.rs`, dispatch | done | plan both, run winner; `pick_backend` unit-tested |
| 5 | T5: hybrid-hier | `hybrid/hier.rs`, dispatch | done | navmesh corridor + A* sliding sub-goal |
| 6 | T6: hybrid-segment | `hybrid/segment.rs`, dispatch | done | navmesh open + A* jump links |
| 7 | T7: docs + close | `distilled.md`, `SERIES.md` | done | distilled.md note added; plan moved to `completed/` |

## Notes
- All six modes exposed via `--mode` (clap): `astar, navmesh, hybrid-fallback, hybrid-race,
  hybrid-hier, hybrid-segment`. `cargo clippy --all-targets --all-features -D warnings` and
  `cargo test` green (107 + hybrid tests).
- **Live A/B verification still pending**: run `spawn-to-spawn` / `spawn-to-weapon` per mode on
  a matching-map server and compare each `# SUMMARY` to the Plan 10 baselines. Code complete;
  the harness needs a running q2 server (not available in this environment).
