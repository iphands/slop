# Plan 10 — Movement Test Harness — Tracker

## Overview
- Status: 80% complete
- Start date: 2026-06-15
- Goal: measurable movement quality (`spawn-to-spawn` / `spawn-to-weapon` → structured log)

## Baseline (fill in at T5)
Run against the **current** steering code (pre-Plans 11–14). These numbers are what
11–14 must beat.

| Scenario | Map | reached | elapsed (s) | distance | mean_speed | max_speed | bumps | wrong_turns | hindered_fr | log |
|----------|-----|---------|-------------|----------|------------|-----------|-------|-------------|-------------|-----|
| spawn-to-spawn | | | | | | | | | | |
| spawn-to-weapon (RL) | | | | | | | | | | |

## Resume Instructions
1. T1 (BSP entities) and T2 (recorder) are independent — do them in parallel, then T3/T4.
2. T4 needs T1 (spawn points / weapon origins) and T2 (recorder) both landed.
3. If `connect-one`'s handshake is hard to factor out, duplicate it into `scenario.rs`
   (Open Q6) — unblock the baseline first.
4. After T5, the baseline numbers above are the contract for Plans 11–14.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: parse `LUMP_ENTITIES` + `spawn_points()` + `find_class()` | `world/src/bsp.rs`, `world/src/lib.rs` | done | 4 new tests green; clippy clean |
| 2 | T2: `MovementRecorder` + detectors + unit tests | `brain/src/recorder.rs`, `brain/src/lib.rs`, `brain/tests/recorder.rs` | done | 7 tests green; clippy clean; detectors via `WallProbe` trait (prod `CmWallProbe`, test stubs) |
| 3 | T3: log format + `dump()` + schema doc | `brain/src/recorder.rs` | done | schema doc block + `dump_matches_documented_schema` test (16 cols + SUMMARY keys) |
| 4 | T4: `SpawnToSpawn` / `SpawnToWeapon` CLI + `scenario.rs` + `scenario_mode` in `bot_task` | `qbots/src/main.rs`, `qbots/src/scenario.rs` | done | wrote a dedicated `scenario.rs` loop (mirrors connect+tick scaffolding, reuses brain primitives, NO `bot_task` munge). build+clippy clean; CLI help shows both subcommands. live run is T5. |
| 5 | T4b: factor (or duplicate) connect handshake for reuse | `qbots/src/supervisor.rs` | done | chose the duplicate path (Open Q6) — `scenario.rs` mirrors the ~15-line `Conn` handshake + `tokio::select!` recv/tick loop; brain logic is shared, not duplicated. |
| 6 | T5: baseline runs + `.gitignore /logs/` + docs | `.gitignore`, `qbots/CLAUDE.md` | pending | `/logs/` already gitignored; live baseline needs a server — fill Baseline table on next live run |
