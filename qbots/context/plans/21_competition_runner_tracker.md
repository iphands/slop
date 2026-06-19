# Competition Runner — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-18
- Deliverable: `qbots competition` subcommand (N bots/mode, skin/mode, per-mode scoreboard)

## Resume Instructions
In-process runner reusing the fleet supervisor with a per-bot `mode`. Build one shared
`NavCache`; spawn the mode×count cross-product; group `FleetStats::snapshot()` by name prefix for
the scoreboard. Commit at each task boundary (`task(TN): …`).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: mode per-bot | `supervisor.rs` | pending | drop FleetShared.mode; param on bot_supervisor_loop |
| 2 | T2: skins::distinct | `skins.rs` | pending | n distinct skins, no dupes |
| 3 | T3: run_competition | `supervisor.rs` | pending | shared cache, cross-product, mode_tag, scoreboard |
| 4 | T4: competition subcmd | `main.rs` | pending | --count per mode, --modes list |
| 5 | T5: docs + close | `distilled.md`, `SERIES.md` | pending | move to completed/ |
