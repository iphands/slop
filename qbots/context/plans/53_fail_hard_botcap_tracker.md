# Fail Hard When Roster Can't Fully Join — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-07-11
- Scope: 4 source files, 6 tasks. Default-strict fail-hard on join failure + `--loose-botcap`.

## Resume Instructions
Tasks are independent-ish but ordered T1→T5 (T3 depends on T1's `Rejected` state; T4 on
T3's error kinds; T5 on T4's params). Each task: implement → `cargo fmt` →
`cargo clippy -- -D warnings` → `cargo test` → commit `task(TN): …`. Then T6 closes out.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: FSM reject classification | `crates/client/src/conn.rs` | pending | `ConnState::Rejected` + `reject_reason` + tests |
| 2 | T2: connect_timeout_ms config | `crates/qbots/src/config.rs` | pending | default 10_000 |
| 3 | T3: bot_task reject/timeout returns | `crates/qbots/src/main.rs` | pending | ConnectionRefused / TimedOut |
| 4 | T4: fleet fatal signal + hard fail | `crates/qbots/src/supervisor.rs` | pending | join_failure + loose_botcap |
| 5 | T5: --loose-botcap CLI flag | `crates/qbots/src/main.rs` | pending | Competition + Run |
| 6 | T6: verify + pitfalls + close | `context/` | pending | move to completed/, SERIES done |
