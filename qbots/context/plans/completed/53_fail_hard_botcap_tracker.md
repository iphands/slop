# Fail Hard When Roster Can't Fully Join — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-07-11
- Completed: 2026-07-11
- Scope: 4 source files, 6 tasks. Default-strict fail-hard on join failure + `--loose-botcap`.

## Resume Instructions
Done. Tasks ran T1→T6 with a commit each. Verification (T6) was live: no-regression
(24 bots joined & fought under the server's `maxclients=64`), plus strict (exit 1,
`fleet join failed`) and loose (exit 0, `dropping this bot`) exercised end-to-end by
pointing `--config` at a copy with `connect_timeout_ms: 0` against the real server.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: FSM reject classification | `crates/client/src/conn.rs` | done | `ConnState::Rejected` + `reject_reason` + 3 tests |
| 2 | T2: connect_timeout_ms config | `crates/qbots/src/config.rs` | done | default 10_000 |
| 3 | T3: bot_task reject/timeout returns | `crates/qbots/src/main.rs` | done | ConnectionRefused / TimedOut |
| 4 | T4: fleet fatal signal + hard fail | `crates/qbots/src/supervisor.rs` | done | join_failure + loose_botcap + `fleet_join_result` |
| 5 | T5: --loose-botcap CLI flag | `crates/qbots/src/main.rs` | done | Competition + Run |
| 6 | T6: verify + pitfalls + close | `context/` | done | live-verified strict/loose/no-regression |

## Diagnostic note
Live server reported `maxclients=64` (not the assumed 18) and accepted all 24 bots — the
original 18-cap was not reproducible against the current server state. The value delivered
is **visibility**: any future join refusal now names its reason and fails loudly (or warns
under `--loose-botcap`) instead of silently short-counting.
