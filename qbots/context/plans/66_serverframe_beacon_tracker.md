# Serverframe Beacon — Tracker

## Overview
- Status: 83% complete (T1–T5 committed as `62b003c83`; T6 needs the live server)
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
| 6 | T6: live verification (socat, 32 bots) | — | pending | needs the live server |

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
