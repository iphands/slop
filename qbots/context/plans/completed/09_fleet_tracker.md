# Fleet (qbots binary) — Tracker

## Overview
- Status: 100% — T1-T6 done; 8-bot fleet verified live.
- Depends on: Plan 06 (brain) — a single bot must work before fanning out
- Exit criterion: 8–16 bots run several minutes, all connected, frags accumulating, no kicks. — MET at 8 bots.

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
| 6 | T6: verify fleet | `qbots status` | done | **8-bot fleet verified live (2026-06-15)**: observed qb0-qb7 all connected over a ~2 min run; 4 bots scored frags in a 40s window; no kicks; 10/25 maxclients. The new `qbots status` OOB query was the verification lens. (Prior session also verified a 3-bot fleet live with full death/respawn lifecycle.) |
