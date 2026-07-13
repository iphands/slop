# Scoreboard K/D-first ranking + health/armor pickup metric — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-07-13
- Scope: competition scoreboard sort + new hp/ap measured axis

## Resume Instructions
Plan file: `67_scoreboard_kd_sort_and_pickup_metric.md`. Tasks are independent-ish but land
in order T1→T4 (T4 needs T2's fields). The pickup hook goes in the Plan 51 delta block
(`main.rs` ~1176), NOT the older Active-branch block (~1372) — see plan "Pre-identified trap".
One commit per task (`task(TN): …`), fmt/clippy/test green before each.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: K/D-first scoreboard ranking | `crates/qbots/src/supervisor.rs` | pending | |
| 2 | T2: BotTally pickup fields + recorders | `crates/qbots/src/stats.rs` | pending | |
| 3 | T3: pickup detection in bot_task | `crates/qbots/src/main.rs` | pending | |
| 4 | T4: scoreboard + final-stats columns | `crates/qbots/src/supervisor.rs` | pending | |
