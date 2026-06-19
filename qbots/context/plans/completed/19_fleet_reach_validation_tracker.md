# Plan 19 — Nav Graph Quality & 8-Bot Fleet Reach Validation — Tracker

## Overview
- Status: 83% complete (T1-T5 done; T6 live verification in progress)
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
| 6 | T6: live verification (8/8 both commands) | live server q2dm1 | in progress | working toward 8/8 |

## Live verification results (T6)

| Run | Command | Reached | Commit | Notes |
|-----|---------|---------|--------|-------|
| baseline | `spawn-to-spawn --count 8 --max-secs 60` | 1/8 | (before fixes) | bots stuck, DEADBAND=4, no blacklisting |
| baseline | `spawn-to-weapon rocketlauncher --count 8 --max-secs 60` | 0/8 | (before fixes) | path_efficiency ~0.2, high wrong_turns |
| after blacklist+DEADBAND+backoff | `spawn-to-spawn --count 8 --max-secs 60` | 2/8 | 4edb5415d | GOAL_GIVEUP=30, BackOff timer 8 ticks, blacklisting |
| after blacklist+DEADBAND+backoff | `spawn-to-weapon rocketlauncher --count 8 --max-secs 60` | 0/8 | 4edb5415d | mean_speed 67-114, wrong_turns 17-71, still not reaching |
| after MAX_SMOOTH_DZ+SEED_MAX_DZ | `spawn-to-spawn --count 8 --max-secs 60` | 2/8 | 05e9c35f9 | path smoothing no longer strips staircase nodes |
| after MAX_SMOOTH_DZ+SEED_MAX_DZ | `spawn-to-weapon rocketlauncher --count 8 --max-secs 60` | 0/8 | 05e9c35f9 | min_dist=150-176 (close but not 48u); bots fall off z=792 platform targeting z=912 |

## Bugs found and fixed during T6

| Bug | Symptom | Fix | Commit |
|-----|---------|-----|--------|
| BackOffThenRepath cancelled | scenario.rs: nav fwd block AFTER recovery match overwrites -0.5 backward | added `backoff_ticks` counter (8 ticks ≈ 0.8s) so backward motion persists | 4edb5415d |
| GOAL_GIVEUP infinite loop | bot blacklists waypoint, force_replan clears blacklist → same path → loop | force_replan() does NOT clear blacklist; only goal-reached clears it | 4edb5415d |
| DEADBAND too small | bots oscillating 9-10 u/s at walls not triggering stuck detection | raised DEADBAND 4→16 | 4edb5415d |
| STAIR_MAX too small | q2dm3 3/7 spawns reachable (144u staircase pairs skipped at 128 limit) | raised STAIR_MAX 128→160 | 29c782201 |
| smooth_path strips staircase nodes | point LOS trace through open staircase interior collapses all stair waypoints | MAX_SMOOTH_DZ=48 cap in smooth_path | 05e9c35f9 |
| seed_spawns cross-floor false edge | weapon node at z=912 connects to z=792 floor node (dz=120 < STAIR_MAX=160) via open-air walkable_stair | SEED_MAX_DZ=54 in seed_spawns | 05e9c35f9 |

## Remaining root cause: bridge_components false walk edges

The core problem blocking 8/8 is **bridge_components creating false walk edges** between
floor clusters that are adjacent in the XY grid but on different floors (e.g. z=792 → z=912,
dz=120u). The `walkable_stair` trace passes through open staircase air between platforms,
never hitting a wall. The bot then navigates this edge by moving horizontally at z=792 toward
the z=912 waypoint, reaches the platform edge, and falls off. Orbit-timeout fires with
`dz=127.9, horiz=33`.

### Approaches tried and their failure modes

| Approach | Result |
|----------|--------|
| Floor probe at every walkable_stair step (STEP+2=20u) | Broke real staircase connections: startsolid when interp-z lands exactly on tread; also fails for steep/winding staircases where interp-z is above actual tread |
| Single midpoint floor probe in walkable_stair (36u range, +1u offset) | Broke winding staircase bridge edges: midpoint of straight line between far nodes is in open air between platforms |
| Midpoint probe in generate() only (not bridge) | Bridge creates same false edges; also some generate() legitimate edges broken |
| MAX_SLOPE=1.2 guard in generate()+walkable_link | Too strict: rejects legitimate steep staircase edges (dz=80/hdist=64=1.25) causing large-dz orbit-timeouts |
| SEED_MAX_DZ=54 in seed_spawns | Fixes weapon node seeding cross-floor edges; does NOT fix bridge_components edges |
| Orbit-timeout ledge blacklisting (dz > 96) | Blacklists real staircase top nodes too; bot can't reach upper levels at all (spawn-to-spawn 0/8 regression) |

### What is known about the false edge location

Waypoints 1750, 1752, 1754, 1756 (z≈464 in q2dm1) are connected via false walk edges to
z=336 floor nodes. These are created by bridge_components, not generate() (the false edge
spans more than one grid cell and connects previously disconnected components).

The actual staircase connecting z=336 to z=464 takes a WINDING PATH around the map — the
two connected nodes are close in XY but the real path is 200+ units of staircase around a
corner. bridge_components creates the direct 33u-horiz/128u-dz "shortcut" that doesn't exist.
