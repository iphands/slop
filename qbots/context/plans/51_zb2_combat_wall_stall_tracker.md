# zb2 Combat Wall-Stall — Tracker

## Overview
- Status: 25% complete (T1 done, T2 baseline soak in flight)
- Start date: 2026-07-11
- Server: config default (noir.lan:27910), map q2dm3 (verified live via `status`)
- Soak recipe: 305 s, `competition --count 3 --brains main,q3,zb2 --navmodes astar` (matches Plans 49/50 baselines)

## Resume Instructions
1. Re-read `context/plans/RULES.md` and Plan 51.
2. Check the Progress table below; the first non-`done` row is the current task.
3. Baseline/post-fix soak logs land in `logs/p51_baseline.log` / `logs/p51_postfix.log` (gitignored — numbers live HERE).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: StallMonitor + fleet wiring + zb2 probe | `brain/stall.rs`, `qbots/main.rs`, `brains/zb2.rs` | done | 5 unit tests; `EVT wall_press` carries `bot=` for per-brain attribution (span fields are dropped by the abbreviated formatter) |
| 2 | T2: baseline soak + analysis table | tracker | pending | |
| 3 | T3: fix proven root cause | `brains/zb2.rs` (+`recover.rs`?) | pending | data-dependent |
| 4 | T4: re-soak compare + notes + close | `context/brain_notes.md` | pending | |

## Baseline (T2) — to fill

| Group | wall_press episodes | total stalled s | dmg eaten stalled | episodes w/ attack>0 | died-in-episode |
|-------|--------------------:|----------------:|------------------:|---------------------:|----------------:|
| main_astar | | | | | |
| q3_astar | | | | | |
| zb2_astar | | | | | |

`EVT zb2_combat_recovery_overwrite` count: — · overlap with zb2 episodes: —

## Post-fix (T4) — to fill

(same table)
