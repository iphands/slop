# Xonotic goal-stack navmode (`xg`) — Tracker

## Overview
- Status: 85% complete (T1-T5 done; T6 sweep partial — q2dm1 legs need the map back)
- Start date: 2026-07-11
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
| 1 | T1: passthrough scaffold + parity | `crates/brain/src/xonnav.rs` | done | `c0335579a` (T1-T4 one commit — struct interlocks). Parity test pins xg ≡ as when inert. |
| 2 | T2: travel-time costs | `xonnav.rs` | done | Static swim-node penalty (12 qu). DEFERRED: fall-time pricing (needs edge-kind-aware weighted API — the node overlay can't see edge kinds). |
| 3 | T3: danger field | `xonnav.rs`, `nav_mode.rs` | done | `note_dangers` + `DangerSource` (defaulted no-op); 0.25s refresh, 0.5s TTL, replan on >200 mass delta; heatmap overlay SUMMED. |
| 4 | T4: cutover + watchdog | `xonnav.rs` | done | 700u + chest-height hull trace + hazard probe; watchdog stall→replan, twice→goal_abandoned. Swim/ride flags stay live during cutover (executor compat). |
| 5 | T5: wiring | `main.rs`, `supervisor.rs` | done | `29376e8bc`. Danger feed at bot_task (rockets/grenades 300, enemies 150). |
| 6 | T6: live sweep + docs + close | `mode_perf.md`, brain_notes, SERIES | in-progress | q2dm3 DONE (see Results). q2dm1 legs (s2s, swim railgun) pending map flip back. |

## Verification

- [ ] Parity/cost/danger/cutover/watchdog unit tests green
- [ ] spawn-to-* reach parity vs `as` (s2s q2dm1, swim q2dm1, ride q2dm3 ×2) — SUMMARYs recorded below
- [ ] competition `--brains q3 --navmodes as,xg` within noise floor
- [ ] mode_perf.md xg section + brain_notes entry
- [ ] Zero warnings, clippy clean, fmt, tests green at every commit (Rule A/B)

## Results (fill during T6)

| Scenario / Run | Map | Result | SUMMARY / K/D |
|---|---|---|---|
| railgun-1 ×2 (xg, runtester) | q2dm3 | 2/2 | 18.73s/3b, 15.62s/6b — beats as-control 25.82s |
| quaddamage ×2 (xg) | q2dm3 | 1/2 | 24.10s reach — the session's ONLY quad reach (as-controls 0/3) |
| 5-min A/B q3 × {as,xg} | q2dm3 | xg ≥ as | xg 0.17 vs as 0.06; 0 drowns, 23 traverse-done, 0 panics |
| s2s + swim railgun (xg) | q2dm1 | pending | needs map back on q2dm1 |
