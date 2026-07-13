# Serverframe Beacon — Tracker

## Overview
- Status: 100% complete (T1–T6 in commit `48274a8b1`; T7 verified live 2026-07-13)
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
| 1 | T1: `frames.rs` parse + identity guard | `crates/api/src/frames.rs` | done | 12 tests; golden line parses qbots' `encode_pins_the_wire_format` output verbatim |
| 2 | T2: `clock.rs` `observe_frame` + anti-jitter | `crates/api/src/clock.rs` | done | 13 new tests; **all 13 pre-existing tests pass unmodified** |
| 3 | T3: `status_cache::apply_beacon` | `crates/api/src/status_cache.rs` | done | 2 tests, incl. `apply_beacon_does_not_fake_server_online` |
| 4 | T4: `FramesConfig` (off by default) | `crates/api/src/config.rs` | done | 3 tests |
| 5 | T5: `spawn_frame_listener` + guarded spawn | `crates/api/src/main.rs` | done | reconnect loop verified live (qbots started *after* qctrl) |
| 6 | T6: frontend optional `MapClock` fields | `frontend/src/lib/api.ts` | done | `tsc -b`, 21 vitest, `vite build` all green on node 22 |
| 7 | T7: live end-to-end | — | done | see Live Results below |

## Live Results (2026-07-13, noir.lan q2dm1, 8-bot fleet)

| Check | Result |
|---|---|
| **Feature-off regression** | No `frames:` block ⇒ `anchor=unknown, source=none`, no task spawned, socket untouched. Byte-identical to pre-Plan-13. |
| **Beacon absent but configured** | Warns once, falls back to edge inference, retries in background. `anchor=unknown`. |
| **Cold start mid-map (THE fix)** | `unknown/null` → **`exact / server_frame / 139s`**, `server_frame=1397` ÷ 10 = 139.7s. The case this module called "genuinely unknowable". |
| **Reconnect** | qbots started *after* qctrl; reader connected on its own with no restart. |
| **Coalescing** | 8 bots × 10 Hz = 80 potential reports/s. `seq` advanced **120 over 120 frames, not 960** ⇒ the fold collapsed it to one per server tick; the wire then carried **1.03 msg/sec**. Bot count never multiplied it. |
| **Same-map restart** | `map q2dm1` while already on q2dm1: elapsed **202s → 12s**, serverframe 2013 → 121. No name edge existed; today's clock would have kept counting to 214. `source` stayed `server_frame`, i.e. the beacon superseded the `own_map_command` anchor (which fires at rcon-send, before the server has loaded). |
| **Fleet stopped** | `elapsed` kept advancing 50 → 65 → 80 (+15/15s), `beacon_age` grew 12 → 27 → 42, `anchor` stayed `exact`. The countdown did not blank. |

Incidental confirmations: `servercount` came back as **1232907478** — a large random value, confirming `svs.spawncount = randk()` and vindicating the never-compare-with-`<`/`>` rule. And `serverframe` kept advancing through intermission, as the plan claimed.

## Notes / Deviations

- The wire format has **no shared crate** with qbots, by design: a shared crate would be a hard
  build dependency between the two workspaces and would defeat "optional coupling". The contract
  is pinned by a golden-line test on each side; **they must be edited together**.
- `sv_uptime` turned out to be a **ghost cvar** — yquake2 has no such cvar, and `Cvar_Set` merely
  created one that nothing reads. `manage_sv_uptime` / `ensure_sv_uptime` / `saw_uptime_key` are
  left in place as the beacon-less fallback path; retiring them is a separate change.

- **The verification run crashed the live Q2 server**, and it is worth recording why, because it
  was entirely self-inflicted and is a trap anyone repeating this will fall into. The fleet was
  started with **qctrl not running**, so nothing had pushed `sv_maplist`. The server was parked in
  intermission (28 min into a 10-min map — nothing owns intermission without qctrl's rotator); the
  joining bots correctly pressed ATTACK to advance the level; the changelevel read an empty
  `sv_maplist` and `Com_Error`'d with `Couldn't load maps/.bsp`. That is Plan 12's founding
  incident, reproduced exactly. **Always start qctrl before any fleet.** Written up in
  `../../context/pitfalls.md`. Note `q2ded` does *not* exit — it drops to a no-map state and stops
  answering UDP while looking alive, and one rcon `map <name>` revives it.
