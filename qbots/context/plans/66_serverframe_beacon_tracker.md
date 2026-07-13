# Serverframe Beacon — Tracker

## Overview
- Status: 0% complete
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
| 1 | T1: pure core — `fold` / `encode` / `should_write` | `crates/qbots/src/beacon.rs` | pending | |
| 2 | T2: `Beacon` handle (watch + `ActiveBot` RAII) | `crates/qbots/src/beacon.rs` | pending | |
| 3 | T3: `BeaconCfg` (off by default) | `crates/qbots/src/config.rs` | pending | |
| 4 | T4: `beacon::serve` (bind / fanout / accept / unlink) | `crates/qbots/src/beacon.rs` | pending | |
| 5 | T5: wire into fleet + the `:1053` frame hook | `supervisor.rs`, `main.rs` | pending | |
| 6 | T6: live verification (socat, 32 bots) | — | pending | needs the live server |

## Notes / Deviations

- The wire format has **no shared crate** with qctrl by design (a shared crate would make the
  coupling mandatory). The contract is pinned by a golden-line test in each repo; they must be
  edited together.
