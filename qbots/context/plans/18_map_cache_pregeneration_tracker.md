# Plan 18 — Ahead-of-Time Map Cache — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-16
- Depends on Plan 17 landing first (STEP fix baked into cache fingerprint).

## Resume Instructions
Read `context/plans/18_map_cache_pregeneration.md` for full task details.
Run `cargo build && cargo clippy -- -D warnings && cargo test && cargo fmt` after each task.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: consolidate generate_map_nav | `world/src/build.rs`, `qbots/scenario.rs`, `qbots/supervisor.rs` | pending | |
| 2 | T2: binary cache format | `world/src/mapcache.rs` | pending | |
| 3 | T3: generate-map-cache CLI | `qbots/main.rs` | pending | |
| 4 | T4: rayon parallel generation | `world/navgraph.rs`, `world/Cargo.toml` | pending | |
| 5 | T5: wire cache lookup | `qbots/supervisor.rs`, `qbots/scenario.rs` | pending | |
| 6 | T6: gitignore | `qbots/.gitignore` | pending | |
| 7 | T7: live verification | local + live server | pending | |

## Timing results (T7)

| Scenario | Without cache | With cache |
|---|---|---|
| `generate-map-cache --map 'q2dm*' --jobs 1` | n/a | |
| `generate-map-cache --map 'q2dm*' --jobs 4` | n/a | |
| `spawn-to-spawn --count 8` wall clock to all-bots-moving | | |
