# Plan 43 — Ride behavior + q2dm3 reach proof & navmode ranking — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-19
- Goal: brain rides q2dm3 `func_train` + railgun `func_plat`; `spawn-to-item quaddamage` and
  `spawn-to-weapon railgun --instance 1` reach; 6-navmode ranking recorded.

## Resume Instructions
Plan 40's swim execution (`brains/main.rs:372-426`, `brain::water`, `current_edge_is_swim`) is
the exact structural template — copy its shape for `Ride`. Validate live against the q2dm3
server with the user's commands (use `--lift-penalty 0` so lifts are used). Suspend recovery
during ride/wait (the `swim_active` gate → `ride_active`). Brain-notes append is mandatory (T6).

## Validation commands (user-provided)
```bash
cargo run --release --bin qbots -- spawn-to-item quaddamage --count 4 --max-secs 150 --navmode <mode> --lift-penalty 0
cargo run --release --bin qbots -- spawn-to-weapon railgun --instance 1 --count 4 --max-secs 150 --navmode <mode> --lift-penalty 0
```

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `brain::ride` live platform tracking | `brain/src/ride.rs` | pending | |
| 2 | T2: `current_edge_is_ride`/`current_ride_info` | `nav.rs`, `nav_mode.rs`, `hybrid/*` | pending | |
| 3 | T3: ride execution in `MainBrain` | `brains/main.rs` | pending | suspend recovery |
| 4 | T4: recorder `P` flag + runtester parity | `recorder.rs` | pending | |
| 5 | T5: live q2dm3 reach proof | (live) | pending | astar first |
| 6 | T6: navmode ranking + brain_notes + pitfalls | `context/*.md` | pending | mandatory notes |
