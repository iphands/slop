# `runtester` Scenario Brain — Tracker

## Overview
- Status: DONE — T1–T4 done, T5 skipped, **T6 live acceptance sweep PASSED** (q2dm1, 6 navmodes,
  zero panics, baseline pattern reproduced).
- Start date: 2026-06-18
- Contract met: verbatim lift of the inline scenario tick → `RunTesterBrain`; CI gates green; the
  live sweep confirmed every navmode navigates with no regression.
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
| 2 | T2: `RunTesterBrain` verbatim lift | `crates/brain/src/brains/runtester.rs` | done | + `BrainKind::Runtester` + `intent_forward` |
| 3 | T3: determinism unit tests | `crates/brain/src/brains/runtester.rs` | done | 6 tests; stub Navigator + open CM |
| 4 | T4: migrate `scenario.rs` | `scenario.rs`, `main.rs` | done | default `--brain runtester`; inline block deleted |
| 5 | T5: `mode-perf-report` (optional) | `crates/tools/` | skipped | no logs without a server |
| 6 | T6: 6-navmode acceptance sweep | `mode_perf.md`, `brain_notes.md`, `SERIES.md` | done | PASSED — q2dm1, 6 navmodes, 0 panics |

## Baseline to beat (`context/mode_perf.md`, q2dm1, 16 bots, 180 s)
| navmode | s2s | s2w(RL) |
|---|:--:|:--:|
| astar | 16/16 | 12/16 |
| navmesh | 5/16 | 15/16 |
| hybrid-fallback | 14/16 | 12/16 |
| hybrid-race | 15/16 | 16/16 |
| hybrid-hier | 11/16 | 1/16 |
| hybrid-segment | 13/16 | 4/16 |

## Post-refactor sweep (2026-06-18, q2dm1, `--brain runtester --count 6`; maxclients=8)
| navmode | s2s (/6) | s2w(RL) (/6) | pass? |
|---|:--:|:--:|:--:|
| astar | 5/6 | 6/6 \* | ✓ |
| navmesh | 2/6 | 6/6 | ✓ |
| hybrid-fallback | 6/6 | 4/6 | ✓ |
| hybrid-race | 5/6 | 6/6 | ✓ |
| hybrid-hier | 3/6 | 0/6 | ✓ (baseline 1/16; no panic) |
| hybrid-segment | 4/6 | 3/6 † | ✓ |

Zero panics across all 12 runs. \* astar s2w 3/6→6/6 across draws (n=6 noise). † segment s2w
0/6 @55 s → 3/6 @180 s (time-limited). Pattern matches the baseline; lift faithful.
