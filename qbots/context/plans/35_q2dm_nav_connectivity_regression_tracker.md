# q2dm nav connectivity: hull-valid routes + residual gaps — Tracker

## Overview
- Status: ~55% complete — T1 root-caused 2026-07-09 (in progress)
- Start date: 2026-06-18 (revised scope 2026-07-09)
- Per-map now: q2dm1/2/4/5/8 full; q2dm3 7/7; q2dm6 7/8; q2dm7 4/6

## T1 root cause (2026-07-09, via navinspect PATH + edge-kind)
Enhanced `navinspect` PATH mode to print each edge's `EdgeKind` + mark BAD *Walk* (blocked hull)
vs OK ride/swim/jump. Ran q2dm3 all-spawns → quad:
- 2 spawns route cleanly; the far-spawn 528u BLOCKED edge is a **Ride** (`*10` train over lava) —
  correct, NOT a bug (the old "354u Walk" note conflated ride edges with walk edges).
- The REAL bug: **4/6 spawns route through `402 (-553,199,232) → 395 (-553,31,216)` — a 168u
  `dz=-16` hull-BLOCKED *Walk* edge.** It survives because `walkable_stair` bases its step count
  only on VERTICAL distance (`steps = ceil(total_dz/STEP)`): a low-dz long edge is `steps=1`, so it
  steps up once and does ONE long horizontal trace, checking floor existence **only at the
  destination** — never mid-span. A 168u/528u edge over a floor gap passes vacuously (same root as
  the `total_dz≈0 → steps=0` return-true-with-no-checks bug).
- **First fix attempt (drive steps by `max(dz, hd)/STEP`) — REVERTED, too aggressive.** Measured
  cross-map (scratch cache, non-destructive): it disconnected maps that were FULL (q2dm8 full→5/6,
  etc.). Reason: `walkable_stair`'s per-step model is "step UP to the elevated z, then trace
  horizontally at that height" — which correctly clears a **≤STEP lip** (a legit step-over edge).
  The 168u `dz=-16` edge is very likely a **real step-over-a-16u-lip** (the direct trace blocks at
  the lip @z216, but at z232 the horizontal path is clear + floor is 16u below). So it is NOT
  obviously false — and sub-stepping the floor check rejects thousands of legit step-over edges.
- **Revised T1 approach (next):** distinguishing a valid step-over-lip from a false gap/wall edge
  needs (a) verify the blocking lip is actually ≤STEP tall (trace the obstacle height, not just
  "elevated trace clear"), AND (b) floor continuity ONLY where the span crosses a real void — per-
  edge live validation via `spawn-to-point (-553,199,232)` from `(-553,31,216)`. This is genuine
  iterative nav work (the plan's "Large" estimate). Kept: the `navinspect` PATH edge-kind
  diagnostic (marks BAD Walk vs ok ride/swim/jump) — committed standalone.
- **Also confirmed:** the far-spawn 528u BLOCKED edge is a legit **Ride** (`*10` train), so the
  far-spawn quad *route* may already be fine — the T6 0/1 quad result is more likely the **`*10`
  ride control-feasibility wall** (noted in mode_perf.md) than a nav-graph bug. Re-verify with a
  clean per-spawn `spawn-to-point` to the board before assuming a graph fix is even needed.

## Resume Instructions
1. Read `35_q2dm_nav_connectivity_regression.md` (revised 2026-07-09) — the bisect-era tasks
   are gone; scope is now hull-valid bridges (T1/T2) + q2dm6/7 residuals (T3) + regen (T4).
2. Known-bad reference edge: q2dm3 `(-121,-161,216) → (191,-329,216)` — 354u "Walk" bridge,
   hull trace fraction 0.07, point trace clear (see `context/brain_notes.md` 2026-06-19 tail).
3. Diagnostics: `navinspect <map> compgaps|gpath`, `spawn-to-point <x> <y> <z>`,
   `QBOTS_NO_PRUNE=1`, `QBOTS_OBSERVE_MOVERS=1`.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 0 | Connector mechanisms (ladders, rides, jump bridges) | `world/build.rs`, `navgraph.rs` | done | shipped 2026-06-19; q2dm3 3/7→7/7 |
| 1 | T1: hull-validate bridge/seed edges + regression test | `navgraph.rs`, `world/tests/` | pending | |
| 2 | T2: split long bridges / resample q2dm3 upper level | `navgraph.rs`, `build.rs` | pending | far-spawn quad ≥3/4 is the gate |
| 3 | T3: q2dm6 (7/8) + q2dm7 (4/6) residuals | per diagnosis | pending | q2dm7 target ≥5/6 |
| 4 | T4: regen all q2dm* + live spot-checks + notes | `mapcache.rs`, live | pending | VERSION bump |

## History
- 2026-06-19: root cause = missing connectors, not `walkable_stair`. Ladder + ride + jump-down
  bridge edges landed (see SERIES + git log P35). Quad reached from spawn3 (Plan 43).
  Far-spawn route reliability deferred by user decision.
- 2026-07-09: user directive (human-like map navigation from anywhere) re-opens far-spawn
  scope; plan revised around hull-valid routes.
