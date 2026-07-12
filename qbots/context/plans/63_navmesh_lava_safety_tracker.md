# Navmesh lava safety (q2dm6) — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-07-12
- Baseline (T1, to fill): lava env_suicides = ?, share of deaths = ?, per brain/navmode
- Gate: lava+slime env_suicides ≤ 2 per bot per 5 min AND < 5% of deaths

## Resume Instructions
Read the plan (`63_navmesh_lava_safety.md`) — B1–B4 carry exact file:line anchors from the
2026-07-12 audit. Order matters: T1 (baseline + red tests) BEFORE any fix (Plan 50/51 lesson).
Measurement tooling already exists: `EVT env_suicide` (WARN) + FleetStats env tallies +
scoreboard `env=` column (commit `2ec2e3ef4`). Baseline command (user's repro):
`RUST_LOG=info cargo run --release --bin qbots -- competition --brains q3,xon --count 2 --navmodes nm,sg --chars grunt,major,sarge,camper --xonchars rus,shp,trt,nob`
(server must be on q2dm6).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: baseline soak + red navmesh-lava tests | `world/tests/lava_navmesh_q2dm6.rs` | pending | commit before marking done |
| 2 | T2: `world::deadly` extraction + heightfield span rejection | `world/src/deadly.rs`, `heightfield.rs` | pending | pure refactor first (tests unchanged), then B1 fix; log poly deltas; commit |
| 3 | T3: drop-link landing validation | `heightfield.rs` (`find_drops`) | pending | `landing_strip_deadly`; polymesh untouched; commit |
| 4 | T4: shared `steer_line_safe` + navmesh fallback + xg cutover | `pursuit.rs`, `nav.rs`, `navmesh_driver.rs`, `xonnav.rs` | pending | A* behavior-identical; navmesh `None` terminal; B5; commit |
| 5 | T5: xon stale-key hazard veto | `brain/src/brains/xon/mod.rs` | pending | re-key on hazard; commit |
| 6 | T6: live A/B + docs + closeout | context docs, SERIES | pending | meet gate; `git mv` to completed/; commit |

## Audit note (2026-07-12 second pass)
All-navmode audit answered "do all navmodes avoid lava?": `as`/`hier`/`xg`(non-cutover)/zb2-route/traverse
are safe; `nm` + hybrids `fb`/`race`/`sg` inherit the navmesh gaps (one driver fix covers all);
xg's cutover was a fifth bug (B5). Sharing decision recorded in the plan's Context: share
primitives (`world::deadly`), keep fallback policy per-driver, do NOT default-impl
`pursue_target_safe` on the trait.
