# Serverframe Beacon — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-07-13
- Producer: qbots Plan 66 (`../qbots/context/plans/66_serverframe_beacon.md`), landed as
  qbots commit `62b003c83`. Its `beacon:` config block is **off by default**; turn it on to
  exercise T5/T7 here.

## Resume Instructions

Read Plan 13 in full. Two things you must not get wrong:

1. **`servercount` is random per server process** (`svs.spawncount = randk()`,
   `vendor/yquake2/src/server/sv_init.c:495`). `servercount != previous` ⇒ new level instance.
   **Never compare servercounts with `<` or `>`.**
2. **The existing `clock.rs` tests are the regression gate.** All 13 must pass *unmodified*. If
   you find yourself editing one, you have changed beacon-less behaviour — which this plan
   explicitly must not do. The whole feature is opt-in; absent a `frames:` block, no task is
   spawned and every gated branch takes exactly the path it took before.

The clock is already a monotonic `Instant` anchor, so the beacon is not a new time model — it is
just a better way to compute the anchor: `map_start = received_at − serverframe×100ms − age_ms`.
Everything downstream (`elapsed_seconds`, `Overdue`, `Degraded`, `rotator::decide`) is untouched.

Tests first (Red → Green → Refactor). `just be-all` + a commit at every task boundary.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `frames.rs` parse + identity guard | `crates/api/src/frames.rs` | pending | golden line must match qbots' `encode_pins_the_wire_format` |
| 2 | T2: `clock.rs` `observe_frame` + anti-jitter | `crates/api/src/clock.rs` | pending | all 13 existing tests must pass unmodified |
| 3 | T3: `status_cache::apply_beacon` | `crates/api/src/status_cache.rs` | pending | must NOT touch `last_ok`/`consecutive_failures` |
| 4 | T4: `FramesConfig` (off by default) | `crates/api/src/config.rs` | pending | |
| 5 | T5: `spawn_frame_listener` + guarded spawn | `crates/api/src/main.rs` | pending | |
| 6 | T6: frontend optional `MapClock` fields | `frontend/src/lib/api.ts` | pending | node 22 for `just fe-build` |
| 7 | T7: live end-to-end | — | pending | needs the live server + a qbots fleet |

## Notes / Deviations

- The wire format has **no shared crate** with qbots, by design: a shared crate would be a hard
  build dependency between the two workspaces and would defeat "optional coupling". The contract
  is pinned by a golden-line test on each side; **they must be edited together**.
- `sv_uptime` turned out to be a **ghost cvar** — yquake2 has no such cvar, and `Cvar_Set` merely
  created one that nothing reads. `manage_sv_uptime` / `ensure_sv_uptime` / `saw_uptime_key` are
  left in place as the beacon-less fallback path; retiring them is a separate change.
