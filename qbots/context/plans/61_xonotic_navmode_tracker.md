# Xonotic goal-stack navmode (`xg`) — Tracker

## Overview
- Status: 0% complete
- Start date: —
- Deliverable: `--navmode xg` — A*-wrapping Navigator with travel-time costs, PVS danger field, chase cutover, progress watchdog; reach parity with `as` proven by spawn-to-* + competition A/B.
- Blocked by: Plan 59 must be in `completed/` first (uses flood/rating primitives). Independent of Plan 60 — can run in parallel with it.

## Resume Instructions
1. Read `context/plans/RULES.md`, Plan 61, `context/distilled/xonotic.md` §2/§3/§8, and `completed/20_hybrid_nav_modes.md` (the wrapping-Navigator precedent).
2. Non-negotiables: delegate to `NavigationDriver` (never reimplement A*); compose with `set_risk_overlay` (sum, never overwrite); preserve swim/ride/jump edge flags (traversal executor reads them); suspend the watchdog during traverse waits (Plan 31 lifts).
3. Runtime pricing only — NO mapcache VERSION bump.
4. Live sweep (T6) needs a q2 server per map; mark `blocked` if unavailable.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: passthrough scaffold + parity | `crates/brain/src/xonnav.rs` | pending | |
| 2 | T2: travel-time costs | `xonnav.rs` (+navgraph if needed) | pending | swim ×1/0.7, fall = free-fall time |
| 3 | T3: danger field | `xonnav.rs`, `nav_mode.rs` | pending | note_dangers defaulted; 0.25 s refresh |
| 4 | T4: cutover + watchdog | `xonnav.rs` | pending | 700 u + clear trace + hazard probe |
| 5 | T5: wiring | `main.rs`, `supervisor.rs` | pending | code `xg` |
| 6 | T6: live sweep + docs + close | `mode_perf.md`, brain_notes, SERIES | pending | parity vs `as`; git mv to completed/ |

## Verification

- [ ] Parity/cost/danger/cutover/watchdog unit tests green
- [ ] spawn-to-* reach parity vs `as` (s2s q2dm1, swim q2dm1, ride q2dm3 ×2) — SUMMARYs recorded below
- [ ] competition `--brains q3 --navmodes as,xg` within noise floor
- [ ] mode_perf.md xg section + brain_notes entry
- [ ] Zero warnings, clippy clean, fmt, tests green at every commit (Rule A/B)

## Results (fill during T6)

| Scenario / Run | Map | Result | SUMMARY / K/D |
|---|---|---|---|
| | | | |
