# Serverframe Beacon — Tracker

## Overview
- Status: 100% complete (T1–T5 committed as `62b003c83`; T6 verified live 2026-07-13)
- Start date: 2026-07-13
- Paired plan: qctrl Plan 13 (`../qctrl/context/plans/13_serverframe_beacon.md`) — the consumer.
  qbots T1–T5 must land before qctrl T5/T7 can be verified live.

## Resume Instructions

Read Plan 66 in full first — the **Pre-Identified Bug** section is the one thing you must not
get wrong: `svs.spawncount = randk()` (`vendor/yquake2/src/server/sv_init.c:495`) means
servercount is **random per server process**. `servercount != previous` ⇒ new level instance.
**Never compare servercounts with `<` or `>`.** There is a test (`a_lower_servercount_is_still_a_level_change`)
whose entire job is to fail if someone "optimises" this into a numeric comparison.

The hard requirement is the 32-bots-one-message property. It lives in `fold()` — a free
function, deliberately, so it is testable without tokio. If you change `fold`, re-run
`thirty_two_bots_reporting_one_frame_produce_one_message` and the `#[tokio::test]` wakeup-count
test together; they guard the same property at two levels.

Tests first (Red → Green → Refactor). `just all` and a commit at **every** task boundary.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: pure core — `fold` / `encode` / `should_write` | `crates/qbots/src/beacon.rs` | done | 19 unit tests; commit `62b003c83` |
| 2 | T2: `Beacon` handle (watch + `ActiveBot` RAII) | `crates/qbots/src/beacon.rs` | done | `#[tokio::test]` proves ≤10 wakeups for 320 bot reports |
| 3 | T3: `BeaconCfg` (off by default) | `crates/qbots/src/config.rs` | done | 2 config tests |
| 4 | T4: `beacon::serve` (bind / fanout / accept / unlink) | `crates/qbots/src/beacon.rs` | done | real-socket test + stale-reclaim + never-steal-a-live-socket |
| 5 | T5: wire into fleet + the `:1053` frame hook | `supervisor.rs`, `main.rs` | done | `start_beacon()` shared by `run_fleet` + `run_competition`; `connect-one` passes `None` |
| 6 | T6: live verification (socat, 32 bots) | — | done | see Live Results below |

## Live Results (2026-07-13, noir.lan q2dm1, 8-bot fleet)

**The coalescing guarantee, measured on real traffic:**

- 8 bots × 10 Hz = **80 potential reports/sec** into `fold`.
- `seq` advanced **120 over 120 server frames — not 960**. So `fold` collapsed the 8 bots' reports
  to exactly one per distinct frame: the 7-of-8 no-op path is real, and the message count follows
  server ticks, not bot count.
- The socket then carried **1.03 msg/sec** (the 1 Hz heartbeat on top of the fold).
- `serverframe` advanced 9.5/sec ≈ the server's 10 Hz, confirming the tick rate the whole design
  rests on.

Consumer side (qctrl Plan 13): a cold-start qctrl went from `anchor=unknown, elapsed=null` to
`anchor=exact, source=server_frame, elapsed=139s` with `serverframe=1397` — i.e. 139.7s, matching.
A same-map restart re-anchored 202s → 12s with no map-name edge in sight.

**`servercount` came back as `1232907478`** — a large random value, exactly as `svs.spawncount =
randk()` (`sv_init.c:495`) predicts. This is the live vindication of the never-compare-with-`<`/`>`
rule and of `a_lower_servercount_is_still_a_level_change`.

`serverframe` also kept advancing through intermission, as designed.

## Notes / Deviations

- **T1–T5 landed as one commit** (`62b003c83`), not five. The module is dead code until T5
  wires it in, so `cargo clippy -D warnings` (RULES Rule A) fails on any partial commit —
  there is no ordering that makes each task independently clean. RULES Rule B's "unless they
  are inseparable" clause. Tests are still per-task and were written first.
- The wire format has **no shared crate** with qctrl by design (a shared crate would make the
  coupling mandatory). The contract is pinned by a golden-line test in each repo; they must be
  edited together.
- `run_competition` gets the beacon too (via the shared `start_beacon`), since it is also a
  real fleet on a real server. `connect-one` does not — it is a single-bot dev tool.

- **The first verification attempt crashed the live Q2 server**, and the cause is a trap worth
  knowing: the fleet was started with **qctrl not running**, so nothing had pushed `sv_maplist`.
  The server was parked in intermission; the joining bots did the right thing and pressed ATTACK to
  advance the level; the changelevel read an empty `sv_maplist` and died on
  `Couldn't load maps/.bsp`. This is qctrl Plan 12's founding incident, reproduced exactly.
  **Always start qctrl before a fleet.** Written up in `../../context/pitfalls.md`.

  Diagnostic worth remembering: the beacon showed a **frozen `serverframe` with a climbing
  `age_ms`** — which is precisely the "wedged server" signal the heartbeat was designed to expose,
  and it is how the intermission-park was spotted. Silence would have looked like a dead socket.
