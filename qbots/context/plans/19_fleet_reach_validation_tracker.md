# Plan 19 — Nav Graph Quality & 8-Bot Fleet Reach Validation — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-16
- Depends on Plan 17 + Plan 18 landing first.

## Resume Instructions
Read `context/plans/19_fleet_reach_validation.md` for full task details.
Run `cargo build && cargo clippy -- -D warnings && cargo test && cargo fmt` after each task.
T6 requires a live q2dm1 server.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: STEP-adjacent constant audit | `brain/nav.rs`, `brain/recover.rs`, `world/navgraph.rs` | pending | |
| 2 | T2: seed scenario goal position | `qbots/scenario.rs` | pending | |
| 3 | T3: --max-secs flag | `qbots/main.rs` | pending | |
| 4 | T4: --count on spawn-to-weapon | `qbots/main.rs` | pending | |
| 5 | T5: per-bot summary | `qbots/main.rs` | pending | |
| 6 | T6: live verification (8/8 both commands) | live server q2dm1 | pending | the acceptance test |

## Live verification results (T6)

| Run | Command | Reached | Notes |
|-----|---------|---------|-------|
| | `spawn-to-spawn --count 8 --max-secs 60` | _/8 | |
| | `spawn-to-weapon rocketlauncher --count 8 --max-secs 60` | _/8 | |
