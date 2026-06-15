# Plan 14 — Nav-Graph & Path Quality — Tracker (DEFERRED)

## Overview
- Status: deferred (start only after Plans 10–13 are measured)
- Start date: —
- Goal: shorter, smoother routes; connected spawns; quantified path efficiency

## Before / After metrics (Plan-10 harness, post 11–13 baseline)
| Metric | Post-11–13 baseline | After Plan 14 |
|--------|---------------------|---------------|
| `path_efficiency` (straight/path_len) | | closer to 1.0 |
| `spawn-to-spawn` elapsed (s) | | lower |
| `bumps` (corner clipping) | | not worse |
| graph node count | | (prune only if churn) |
| spawns in largest component | | 100% |

## Resume Instructions
1. **Do not start until Plan 10 baseline + Plans 11–13 are landed and measured.** The recorder
   data decides whether the funnel/jump-link work is worth it.
2. T1 (funnel) is the highest-ROI sub-task; T3 (spawn connectivity) is the cheapest safety win
   and could be pulled forward independently if a map shows disconnected spawns.
3. T2 (jump links) is the riskiest — land it last, conservatively, opt-in per map.
4. T4's `path_efficiency` metric should be added to the recorder early (cheap) even before the
   smoothing work, so we have a number to beat.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 0 | Prereq: Plans 10–13 measured | — | pending | defer gate |
| 1 | T4 (early): `path_efficiency` recorder metric | `brain/src/recorder.rs` | pending | cheap, do first |
| 2 | T1: funnel/string-pull smoothing | `brain/src/nav.rs` | pending | highest ROI |
| 3 | T3: spawn-point seeding + connectivity assert | `world/src/navgraph.rs` or `supervisor.rs` | pending | cheapest safety |
| 4 | T2: jump links (`EdgeKind::Jump`) | `world/src/navgraph.rs`, `brain/src/steer.rs` | pending | riskiest |
| 5 | T4b: optional node prune (only if churn) | `world/src/navgraph.rs` | pending | measure first |
