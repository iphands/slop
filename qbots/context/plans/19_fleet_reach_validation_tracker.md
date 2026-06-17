# Plan 19 — Nav Graph Quality & 8-Bot Fleet Reach Validation — Tracker

## Overview
- Status: 83% complete (T1-T5 done; T6 live verification pending)
- Start date: 2026-06-16
- Depends on Plan 17 + Plan 18 landing first.

## Resume Instructions
Read `context/plans/19_fleet_reach_validation.md` for full task details.
Run `cargo build && cargo clippy -- -D warnings && cargo test && cargo fmt` after each task.
T6 requires a live q2dm1 server.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: STEP-adjacent constant audit | `brain/nav.rs`, `brain/recover.rs`, `world/navgraph.rs` | done | comments added distinguishing step-climb (18) vs arrival-tolerance vs trace-lift |
| 2 | T2: seed scenario goal position | `qbots/scenario.rs` | done | weapon origin seeded as exact nav node before graph wrapped in Arc |
| 3 | T3: --max-secs flag | `qbots/main.rs` | done | on both SpawnToSpawn + SpawnToWeapon (default 30.0); DEFAULT_MAX_SECS removed |
| 4 | T4: --count on spawn-to-weapon | `qbots/main.rs` | done | mirrors SpawnToSpawn; no longer hardcoded 1 |
| 5 | T5: per-bot summary | `qbots/main.rs` | done | handles Vec carries (String, JoinHandle); per-bot reached=true/false + "N/total bots reached" |
| 6 | T6: live verification (8/8 both commands) | live server q2dm1 | pending | the acceptance test |

## Live verification results (T6)

| Run | Command | Reached | Notes |
|-----|---------|---------|-------|
| baseline (before fixes) | `spawn-to-spawn --count 8 --max-secs 60` | 1/8 | bots stuck, DEADBAND=4, no blacklisting |
| baseline (before fixes) | `spawn-to-weapon rocketlauncher --count 8 --max-secs 60` | 0/8 | path_efficiency ~0.2, high wrong_turns |
| after blacklist+DEADBAND+backoff fixes | `spawn-to-spawn --count 8 --max-secs 60` | 2/8 | GOAL_GIVEUP=30, BackOff timer 8 ticks, blacklisting |
| after blacklist+DEADBAND+backoff fixes | `spawn-to-weapon rocketlauncher --count 8 --max-secs 60` | 0/8 | mean_speed 67-114, wrong_turns 17-71, still not reaching |

## Bugs found and fixed during T6

| Bug | Symptom | Fix | Commit |
|-----|---------|-----|--------|
| BackOffThenRepath cancelled | scenario.rs: nav fwd block AFTER recovery match overwrites -0.5 backward | added `backoff_ticks` counter (8 ticks ≈ 0.8s) so backward motion persists | 4edb5415d |
| GOAL_GIVEUP infinite loop | bot blacklists waypoint, force_replan clears blacklist → same path → loop | force_replan() does NOT clear blacklist; only goal-reached clears it | 4edb5415d |
| DEADBAND too small | bots oscillating 9-10 u/s at walls not triggering stuck detection | raised DEADBAND 4→16 | 4edb5415d |
| STAIR_MAX too small | q2dm3 3/7 spawns reachable (144u staircase pairs skipped at 128 limit) | raised STAIR_MAX 128→160 | 29c782201 |

## Remaining issues
- spawn-to-spawn 2/8: 6 bots still fail with high wrong_turns (25-93), high bumps (57-120)
- spawn-to-weapon 0/8: fast bots (mean_speed 78-114) but path_efficiency low (~0.2)
- Root cause candidates: wrong_turns = circuitous path from overcrowded server bots blocking corridors; high hindered frames from geometry near some spawn points
