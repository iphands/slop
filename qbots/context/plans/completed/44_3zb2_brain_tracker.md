# Plan 44 — 3ZB2-Style Brain Plugin (`zb2`) — Tracker

## Overview
- Status: **DONE (2026-07-10)** — all five tasks shipped in one pass (the P23/P46 seams did the heavy lifting). Moved to `completed/`.
- Start date: —
- Goal: `Zb2Brain` plugin — committed-route following + `Search_NearlyPod` shortcuts +
  mover route-states over the shared traversal executor; competition vs `q3`/`main`.

## Resume Instructions
1. Read `44_3zb2_brain.md` (2026-07-09 rewrite) — the old T3 "port G_FindRouteLink into
   world/src/nav_generator.rs" is DROPPED (file doesn't exist; our graph is richer).
2. Template: Plan 37's `q3` wiring (`crates/brain/src/brains/q3/`, `brains/mod.rs:27-79`).
3. References: `context/distilled/brains/3zb2_brain.md`, `vendor/3zb2-zigflag/src/bot/`.
4. T1/T2 can land before Plan 46; T3 needs the shared traversal executor.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: skeleton + `BrainKind::Zb2` wiring | `brains/zb2.rs`, `brains/mod.rs`, CLI | pending | mirror Plan 37 |
| 2 | T2: committed route + `Search_NearlyPod` skip | `brains/zb2.rs` | pending | unit-test the skip |
| 3 | T3: mover states over traversal executor | `brains/zb2.rs` | pending | blocked on Plan 46 |
| 4 | T4: weapon-run item bias | `brains/zb2.rs` | pending | reuse `best_item_goal_weighted` |
| 5 | T5: live proof + competition + notes | `mode_perf.md`, `brain_notes.md` | pending | 2× 5-min runs |


## Closeout (2026-07-10)
| Task | Status | Notes |
|---|---|---|
| T1 skeleton + wiring | done | `BrainKind::Zb2` (`--brain zb2`, auto in competition); connects/roams/fights, 0 panics |
| T2 committed route + Search_NearlyPod | done | `Zb2Route` (own A* polyline, Navigator facade) + pure `nearly_pod_skip` (never across non-Walk edges, dz gate); 4 unit tests |
| T3 mover states via executor | done | facade feeds the shared TraversalExecutor — cursor frozen while carried, zero duplicated mover code. Live: q2dm1 swim **2/3**; q2dm3 ride **1/4** (one 29.4s reach — capability proven; peers hit 3/4 → follow-up: node-by-node follower needs pursue-style look-ahead on the fragmented far-spawn approach) |
| T4 weapon-run bias | done | Blaster-armed → route to a visible weapon pickup |
| T5 competitions + notes | done | q2dm3: **zb2 0.38 BEATS q3 0.20** (half the deaths — the purposeful-runner texture); main 0.82 > zb2 0.24 (post-thrash-fix main is strong). Single-run caveat per acceptance.md. mode_perf + brain_notes appended |

Deviation (documented in module header): zb2 ignores `--navmode` (always A*-graph-routed — authentic to 3ZB2's chain files).
