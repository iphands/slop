# Fleet Durability: Active-State Frame-Stall Watchdog — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-07-13
- Target: full roster survives ≥4 consecutive map cycles (2 min timelimit server)

## Resume Instructions
Plan 64 shipped map-change survival but left a hole: no watchdog while `Active`
(`crates/qbots/src/main.rs:1062` gates the deadline to `state != Active`). T1 adds the
frame-stall watchdog, T2 resets the supervisor reconnect budget, T3 is the live
multi-cycle run. If resuming mid-T3: the competition command + poll procedure are in the
plan; compare `qbots status` player counts against the initial roster size.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: frame-stall watchdog + `stall_timeout_ms` | `main.rs`, `config.rs` | pending | |
| 2 | T2: reset reconnect budget on success | `supervisor.rs` | pending | |
| 3 | T3: live ≥4-map-cycle durability run | — | pending | |
