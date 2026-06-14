# Fleet (qbots binary) — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-14
- Plan: `07_fleet.md`
- Depends on: Plan 06 (brain) — a single bot must work before fanning out
- Exit criterion: 8–16 bots run several minutes, all connected, frags accumulating, no kicks.

## Resume Instructions
1. Confirm Plans 03–06 done — one bot connects, perceives, navigates, fights.
2. The shared `Arc<World>` from Plan 05 is the fleet's cheap multiplier; load once, share read-only.
3. Stagger connects + backoff (T2/T4) before going wide — a connectionless flood gets the IP banned.
4. Use qctrl's RCON `status` as the fleet verification lens.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: roster config | `qbots/src/config.rs` | pending | TOML + example |
| 2 | T2: supervisor | `qbots/src/supervisor.rs` | pending | stagger/backoff/shutdown |
| 3 | T3: logging + status | `qbots/src/logging.rs` | pending | tracing; qctrl optional |
| 4 | T4: rate pacing | `qbots/src/pacing.rs` | pending | qports, rate, maxclients |
| 5 | T5: CLI | `qbots/src/main.rs` | pending | run/connect-one/status |
| 6 | T6: verify fleet | — | pending | 8–16 bots, minutes-long |
