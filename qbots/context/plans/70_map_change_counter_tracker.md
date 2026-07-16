# Aggregated map-change count — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-07-16
- Scope: fleet-wide map-change count + sequence in the competition FINAL report

## Resume Instructions
Plan: `70_map_change_counter.md`. Dedup key is the **servercount** (BTreeMap<i32,String>) —
that collapses all 36 bots' independent detections into one count and gives the map sequence
in order. Record at bot_task's map-load block (main.rs:1313), report after the FINAL scoreboard.
One commit per task, fmt/clippy/test green each.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: FleetStats map log + queries | `crates/qbots/src/stats.rs` | pending | |
| 2 | T2: record at map load + report line | `crates/qbots/src/{main,supervisor}.rs` | pending | |
