# Map-Change Survival + Intermission Handling — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-07-12
- Live target: cosmo.lan:27910 (rcon via http://cosmo.lan:3000/api/rcon/execute), fraglimit 5

## Resume Instructions
Read Plan 64. T1 is pure `Conn` FSM work (unit-testable offline). T2/T3 are `bot_task`
in `crates/qbots/src/main.rs`. T4 needs the live server: start a competition fleet, rcon
`map q2dm2` mid-run, then let fraglimit 5 rotate the level.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: changing/reconnect stufftext in Conn | `crates/client/src/conn.rs` | pending | |
| 2 | T2: per-map reset + deadline reset | `crates/qbots/src/main.rs` | pending | |
| 3 | T3: intermission freeze + ATTACK vote | `crates/qbots/src/main.rs`, `q2proto/playerstate.rs` | pending | |
| 4 | T4: live verification | — | pending | |
