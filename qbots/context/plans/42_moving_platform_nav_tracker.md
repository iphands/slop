# Plan 42 — Moving-platform (`func_train`) nav integration — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-19
- Goal: q2dm3 quad + loop-train railgun join the reachable nav graph via `EdgeKind::Ride`
  train edges + lift anchoring; `generate-map-cache q2dm3` succeeds.

## Resume Instructions
Read Plan 39's water-edge work as the structural template (new `EdgeKind`, side tables, prune
protection, cache `VERSION` bump). q2dm3 mover ground truth is in the plan Context. Build/inspect
live with `QBOTS_LIVE=1 ./target/release/navinspect vendor/baseq2 q2dm3 navquery <x> <y> <z>`.
Trace-guard EVERY synthesized board/dismount node — false bridges are the recurring failure.

## Key q2dm3 facts
- Quad train `*10`: t1 (143,-296,184) ↔ t2 (143,88,184); leads to quad (192,320,216) comp29.
- Loop trains `*3`,`*4`: corners t6…t15 at z=-120; lead to func_plat `*2` elevator → railgun
  (768,816,208) comp62.
- `EdgeKind` lives at `navgraph.rs:82`; lift code at `build.rs:188` (`add_elevator_edges`).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `EdgeKind::Ride` + side tables + accessors | `navgraph.rs` | pending | |
| 2 | T2: `func_train` corner parse + top heights | `build.rs` | pending | |
| 3 | T3: board/dismount synth + `add_train_edges` | `build.rs` | pending | |
| 4 | T4: lift anchoring (railgun elevator top) | `build.rs`, `navgraph.rs` | pending | |
| 5 | T5: connectivity + cache regen + VERSION bump | `mapcache.rs` | pending | coordinate w/ Plan 35 |
| 6 | T6: offline q2dm3 reachability (ride-edge) tests | `world/tests/` | pending | |
