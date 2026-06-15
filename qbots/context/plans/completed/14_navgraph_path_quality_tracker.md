# Plan 14 — Nav-Graph & Path Quality — Tracker

## Overview
- Status: **done** (code complete 2026-06-15; live verification requires running server)
- Start date: 2026-06-15
- Goal: shorter, smoother routes; connected spawns; quantified path efficiency

## Before / After metrics (Plan-10 harness, post 11–13 baseline)
| Metric | Post-11–13 baseline | After Plan 14 |
|--------|---------------------|---------------|
| `path_efficiency` (straight/path_len) | — (metric not yet in recorder) | now recorded in SUMMARY |
| `spawn-to-spawn` elapsed (s) | fail-to-reach | string-pull reduces grid zigzag |
| `bumps` (corner clipping) | 196/239 hindered frames | expect not worse |
| graph node count | unchanged | unchanged (prune deferred) |
| spawns in largest component | unknown | warned if < 100% at build time |

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 0 | Prereq: Plans 10–13 measured | — | done | Plans 10–13 all landed |
| 1 | T4 (early): `path_efficiency` recorder metric | `brain/src/recorder.rs` | done | SUMMARY has path_efficiency={:.3} |
| 2 | T1: funnel/string-pull smoothing | `world/src/navgraph.rs`, `brain/src/nav.rs` | done | smooth_path + smooth_with_cm; 3 unit tests |
| 3 | T3: spawn-point seeding + connectivity assert | `world/src/navgraph.rs`, `supervisor.rs` | done | seed_spawns + spawns_in_largest_component + warn log; 3 unit tests |
| 4 | T2: jump links (`EdgeKind::Jump`) | `world/src/navgraph.rs`, `brain/src/nav.rs`, `main.rs` | done | detect_jump_edges (downward only, conservative); prev_waypoint + current_edge_is_jump; 1 unit test |
| 5 | T4b: optional node prune (only if churn) | `world/src/navgraph.rs` | skipped | measure first — deferred until live baseline shows churn |

## Verification Checklist
- [x] T1: funnel smoothing unit tests (straight collapse, open L-shape, short path)
- [x] T1: smooth_with_cm wired into main.rs after set_goal; cargo clippy clean
- [x] T2: detect_jump_edges_adds_ledge_drop unit test; cargo clippy clean
- [x] T3: seed_spawns_adds_and_connects + seed_spawns_skips_nearby_node + spawns_connectivity_counts_correct
- [x] T4: path_efficiency in SUMMARY; dump_matches_documented_schema updated
- [ ] Live: spawn-to-spawn elapsed time improvement (requires running server)
- [ ] Live: nav CLI jump_edges count on multi-level map
