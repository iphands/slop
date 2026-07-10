# Plan 32 — Underwater breath — Tracker

## Overview
- Status: **DONE (2026-07-10)** — T1 AirClock + T2 surface-seek shipped (unit-tested end to end);
  T4 live regression passed (q2dm1 railgun swim 3/3, zero damage → no false surfacing). T3 dive
  gating deferred (needs Navigator path introspection; the surface-seek makes an over-budget dive
  self-correcting — the bot bobs up for a breath mid-route, which is also the human behavior).
  Moved to `completed/`.
- Start date: 2026-07-10
- Goal: client-side air clock (Q2's 12s rule), surface-seek override in the traversal
  executor, air-budget dive gating; zero drownings.

## Verification notes (2026-07-10)
- **Forced-loiter live test substituted by the deterministic unit test**: the scenario harness's
  preflight *rejects* unreachable goals by design ("goal node isolated — aborting"), so pinning a
  goal inside a wall to force an underwater loiter can't run through `spawn-to-point`. The unit
  test `air_critical_overrides_swim_toward_surface` proves the same contract end to end (submerged
  past budget with a DOWNWARD path target → full up-thrust + hard up-pitch; one breathable frame
  resets → the dive resumes), and the live q2dm1 swim regression (3/3, zero damage) proves the
  clock doesn't false-fire on in-budget routes.
- Deep-water coordinates found for future live work via `navinspect watermap/contents`: q2dm1
  water column x≈192–288, y≈-100..-292, z≈220–430 (surface ~432).

## Resume Instructions
1. Read `32_underwater_breath.md`. Swim works (Plan 40); there is NO air model today.
2. Server truth: `vendor/yquake2/src/game/p_client.c` `P_WorldEffects` (12s air, 2/s
   escalating drown damage at waterlevel 3).
3. Blocked on Plan 46 for T2 (executor hosts the override; priority: drown-surface > ride
   > ladder > swim).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `AirClock` (pure) | `water.rs` | done | 12s budget − 2s margin; must_surface(tts); damage re-sync; SWIM_UP_SPEED=60 pinned from Plan 40 logs; 5 unit tests |
| 2 | T2: surface-seek override | `traverse.rs` | done | gates() ticks the clock (gains `dt`); apply_swim overrides to full-up when critical; main re-syncs on unexplained underwater damage; end-to-end unit test |
| 3 | T3: dive gating + post-drown heal hook | — | deferred | needs Navigator path introspection; surface-seek makes over-budget dives self-correcting (bob up, breathe, re-dive — the human behavior) |
| 4 | T4: live proof | live | done | q2dm1 railgun swim **3/3** (9.9/16/18.3s), zero damage; forced-loiter → deterministic unit test (see notes) |
