# Plan 10 — Movement Test Harness — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-06-15
- Goal: measurable movement quality (`spawn-to-spawn` / `spawn-to-weapon` → structured log)

## Baseline (fill in at T5)
Run against the **current** steering code (pre-Plans 11–14). These numbers are what
11–14 must beat. Measured live 2026-06-15 on `noir.lan:27910` (q2dm1, 2 other
clients present; scenario combat disabled so the run is pure navigation). `max_speed`
≤ 300 throughout — no overspeed / no cheating confirmed.

| Scenario | Map | reached | elapsed (s) | distance | mean_speed | max_speed | bumps | wrong_turns | hindered_fr | log |
|----------|-----|---------|-------------|----------|------------|-----------|-------|-------------|-------------|-----|
| spawn-to-spawn | q2dm1 | false | 29.92 | 1002 | 33 | 300 | 18 | 26 | 196 | `logs/spawn-to-spawn/1781559241.qb0.log` |
| spawn-to-weapon (RL) | q2dm1 | false | 29.91 | 322 | 11 | 276 | 62 | 1 | 239 | `logs/spawn-to-weapon/1781559291.qb1.log` |

**Headline**: both runs **failed to reach** the goal in 30 s. `spawn-to-spawn` made
~1000 u of progress (mean 33 u/s vs. a 300 u/s envelope) with 196/300 frames
hindered; `spawn-to-weapon` barely moved (322 u) and ground a wall near spawn
(62 bumps). This is exactly the orbit/grind/wrong-way-facing pathology Plans 11–13
target. The log files themselves are gitignored (`/logs/`).

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
| 6 | T5: baseline runs + `.gitignore /logs/` + docs | `.gitignore`, `qbots/CLAUDE.md` | done | both baselines measured live on q2dm1; numbers in Baseline table; `/logs/` gitignored; CLAUDE.md "Movement testing" section added |
