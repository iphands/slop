# `runtester` Scenario Brain — Tracker

## Overview
- Status: CODE-COMPLETE (T1–T4 done; T5 skipped; T6 live sweep pending a server).
- Start date: 2026-06-18
- Contract: verbatim lift of the inline scenario tick → `RuntesterBrain`; CI gates green; the
  live 6-navmode acceptance sweep (≥ `mode_perf.md` baseline − 2/16) is the one outstanding item,
  to run when a server is reachable (`noir40.lan` was down this session).
- Closes: Plan 22 T4 (scenario.rs onto Brain) + retires Plan 15 duplication.

## Resume Instructions
Plans 24 + 25 must be `done` (`MainBrain` relocated; `--brain`/`--navmode` wired). T2 is a
verbatim lift — do not "improve" it, or parity/sweep results drift. T6 is the acceptance gate
(live, 16 bots × 180 s × 6 navmodes × 2 scenarios); the plan only closes when every navmode
clears the gate. If interrupted, the Progress table's last `done` row + `cargo build` show where.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `BrainContext.goal_override` | `crates/brain/src/brains/core.rs` | done | dropped `BrainConfig.goal_override` |
| 2 | T2: `RuntesterBrain` verbatim lift | `crates/brain/src/brains/runtester.rs` | done | + `BrainKind::Runtester` + `intent_forward` |
| 3 | T3: determinism unit tests | `crates/brain/src/brains/runtester.rs` | done | 6 tests; stub Navigator + open CM |
| 4 | T4: migrate `scenario.rs` | `scenario.rs`, `main.rs` | done | default `--brain runtester`; inline block deleted |
| 5 | T5: `mode-perf-report` (optional) | `crates/tools/` | skipped | no logs without a server |
| 6 | T6: 6-navmode acceptance sweep | `mode_perf.md`, `brain_notes.md`, `SERIES.md` | pending | live sweep — server down; CI gates green |

## Baseline to beat (`context/mode_perf.md`, q2dm1, 16 bots, 180 s)
| navmode | s2s | s2w(RL) |
|---|:--:|:--:|
| astar | 16/16 | 12/16 |
| navmesh | 5/16 | 15/16 |
| hybrid-fallback | 14/16 | 12/16 |
| hybrid-race | 15/16 | 16/16 |
| hybrid-hier | 11/16 | 1/16 |
| hybrid-segment | 13/16 | 4/16 |

## Post-refactor sweep (fill in at T6)
| navmode | s2s | s2w(RL) | pass? |
|---|:--:|:--:|:--:|
| astar | | | |
| navmesh | | | |
| hybrid-fallback | | | |
| hybrid-race | | | |
| hybrid-hier | | | |
| hybrid-segment | | | |
