# Hybrid Navigation Modes — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-18
- Deliverable: 4 `hybrid-*` `--mode` backends + shared factory + accessors

## Resume Instructions
Each mode is a `Navigator` impl in `crates/brain/src/hybrid/` owning a `NavigationDriver`
+ `NavmeshDriver`. Wire both dispatch sites (`main.rs:615`, `scenario.rs:261`) through a
factory in the qbots crate. Verify with the Plan 10 movement scenarios per mode. Commit
at every task boundary (`task(TN): …`).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: planner accessors | `nav.rs`, `navmesh_driver.rs` | pending | `planned_cost`/`jump_count`/`path`/`planned_len` |
| 2 | T2: hybrid scaffold | `hybrid/mod.rs`, `lib.rs` | pending | `Sub` + `goal_to_pos` |
| 3 | T3: hybrid-fallback | `hybrid/fallback.rs`, dispatch | pending | A* primary, navmesh on stuck |
| 4 | T4: hybrid-race | `hybrid/race.rs`, dispatch | pending | plan both, run winner |
| 5 | T5: hybrid-hier | `hybrid/hier.rs`, dispatch | pending | navmesh corridor + A* local |
| 6 | T6: hybrid-segment | `hybrid/segment.rs`, dispatch | pending | navmesh open + A* jump links |
| 7 | T7: docs + close | `distilled.md`, `pitfalls.md`, `SERIES.md` | pending | move plan to `completed/` |
