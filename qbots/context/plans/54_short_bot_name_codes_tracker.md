# Short Bot-Name Codes — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-07-11
- Scope: 2 source files. Short codes for brain(3)/mode(2)/char(3) so competition names ≤15.

## Resume Instructions
Ordered T1→T5. Each task: implement → `cargo fmt` → `cargo clippy -- -D warnings` →
`cargo test` → commit `task(TN): …`. T5 does live verify + closes the plan.

Code scheme: brain mai/sen/run/q3/zb2; mode as/nm/fb/rc/hr/sg; char gru/maj/sar/cam.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: code fns + group_tag | `crates/qbots/src/supervisor.rs` | pending | mode_code/brain_code/char_code |
| 2 | T2: tests | `crates/qbots/src/supervisor.rs` | pending | fix group_tag test + length-bound test |
| 3 | T3: help-text examples | `crates/qbots/src/main.rs` | pending | mai_as_1 / q3_rc_1 / q3_rc_gru_1 |
| 4 | T4: scoreboard legend | `crates/qbots/src/supervisor.rs` | pending | optional code→full legend |
| 5 | T5: live verify + close | `context/` | pending | status shows ≤15 names; move to completed/ |
