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
| 2 | T2: heightfield deadly-floor span rejection | `world/src/navmesh/heightfield.rs` | pending | port `floor_is_deadly`; log poly deltas; commit |
| 3 | T3: drop-link landing validation | `heightfield.rs` / `polymesh.rs` | pending | `landing_strip_deadly` semantics; commit |
| 4 | T4: driver fallback guard | `brain/src/navmesh_driver.rs` | pending | no unvalidated funnel vertex; commit |
| 5 | T5: xon stale-key hazard veto | `brain/src/brains/xon/mod.rs` | pending | re-key on hazard; commit |
| 6 | T6: live A/B + docs + closeout | context docs, SERIES | pending | meet gate; `git mv` to completed/; commit |
