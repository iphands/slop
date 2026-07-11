# Short Bot-Name Codes — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-07-11
- Completed: 2026-07-11
- Scope: 2 source files. Short codes for brain(3)/mode(2)/char(3) so competition names ≤15.

## Resume Instructions
Done. T4 (legend) was folded into T1 (it gives `mode_tag` its remaining use as the
full-name source once `group_tag` switched to codes). Live-verified: `status` showed the
short names on-server (`q3_fb_maj_8`, `zb2_fb_1`, `mai_as_5`; longest 11 chars, none
truncated) and the launch legend printed.

Code scheme: brain mai/sen/run/q3/zb2; mode as/nm/fb/rc/hr/sg; char gru/maj/sar/cam.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: code fns + group_tag (+legend) | `crates/qbots/src/supervisor.rs` | done | mode_code/brain_code/char_code + log_competition_legend |
| 2 | T2: tests | `crates/qbots/src/supervisor.rs` | done | short-code test + all-combos ≤15 (`_999`) test |
| 3 | T3: help-text examples | `crates/qbots/src/main.rs` | done | mai_as_1 / q3_rc_1 / q3_rc_gru_1 |
| 4 | T4: scoreboard legend | `crates/qbots/src/supervisor.rs` | done | folded into T1 |
| 5 | T5: live verify + close | `context/` | done | on-server names ≤15, legend prints, groups tally |
