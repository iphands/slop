# Plan 41 — `spawn-to-item` + targeting — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-06-19
- Goal: `spawn-to-item` exists; item aliases work; multi-instance targets are selectable.

## Resume Instructions
Start at T1 (alias table + `ScenarioGoal::Item`). Each task is small; commit per task. Validate
T4 against q2dm3 (quad `(192,320,216)`, railgun instances `(-368,-64,352)` / `(768,816,208)`).
No nav/brain changes — pure CLI + goal resolution.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: alias table + `Item` variant | `scenario.rs` | done | |
| 2 | T2: instance-aware resolver + log all candidates | `scenario.rs` | done | |
| 3 | T3: `SpawnToItem` cmd + `--instance` | `main.rs` | done | |
| 4 | T4: thread `instance` through scenario | `scenario.rs`, `main.rs` | done | |
| 5 | T5: docs + help | `main.rs`, `CLAUDE.md` | done | |
