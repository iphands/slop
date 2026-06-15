# Fleet (qbots binary) — Tracker

## Overview
- Status: ~85% — T1-T5 done; T6 verified at 3-bot scale (live), full 8-16 run deferred.
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
| 1 | T1: roster config | `qbots/src/config.rs` | done | `fleet` section (count/prefix/qport/stagger/reconnect/max_bots) |
| 2 | T2: supervisor | `qbots/src/supervisor.rs` | done | shared `NavCache`, stagger, reconnect w/ backoff, graceful shutdown |
| 3 | T3: logging + status | `qbots/src/{main,supervisor}.rs` | done | per-bot `bot` span + 30s heartbeat; span-scope display on combat lines is a follow-up |
| 4 | T4: rate pacing | `qbots/src/{supervisor,config}.rs` | done | distinct qports, staggered connects, `max_bots` maxclients guard |
| 5 | T5: CLI | `qbots/src/main.rs` | done | `run` (fleet) + `connect-one`; SIGINT/SIGTERM shutdown |
| 6 | T6: verify fleet | — | partial | **3-bot fleet verified live**: shared nav, staggered connect, 80 shots, full death/respawn lifecycle, no kicks. Full 8-16 multi-minute run deferred (live shared server). |
