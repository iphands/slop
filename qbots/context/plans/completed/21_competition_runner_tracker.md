# Competition Runner — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-06-18
- Completed: 2026-06-18
- Deliverable: `qbots competition` subcommand (N bots/mode, skin/mode, per-mode scoreboard)

## Resume Instructions
In-process runner reusing the fleet supervisor with a per-bot `mode`. Build one shared
`NavCache`; spawn the mode×count cross-product; group `FleetStats::snapshot()` by name prefix for
the scoreboard. Commit at each task boundary (`task(TN): …`).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: mode per-bot | `supervisor.rs` | done | mode moved to bot_supervisor_loop param |
| 2 | T2: skins::distinct | `skins.rs` | done | `distinct_skins`; landed with T4 (sole consumer) |
| 3 | T3: run_competition | `supervisor.rs` | done | shared cache, cross-product, `mode_tag`, `mode_scoreboard`; landed with T4 |
| 4 | T4: competition subcmd | `main.rs` | done | `--count` per mode, `--modes` list, distinct skins |
| 5 | T5: docs + close | `distilled.md`, `SERIES.md` | done | plan moved to `completed/` |

## Live verification (2026-06-18, noir.lan q2dm1)
`qbots competition --count 2` (6 modes × 2 = 12 bots), ~50 s, SIGINT → graceful exit. Confirmed:
6 distinct skins, **1** `nav graph ready` + **1** `navmesh built` (shared cache works), no qport
collisions, live + FINAL per-mode K/D scoreboards printed, no panics.
