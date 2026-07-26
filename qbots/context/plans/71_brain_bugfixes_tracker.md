# Plan 71 — `main` brain bug fixes — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-07-16
- Bugs: 4 (recovery early-out, recovery missing direction, move_ctrl BUTTON_ANY, combat stale re-lock)

## Resume Instructions

If interrupted, resume at the first `pending` task below. All tasks are independent and can be done in any order. Each task requires reading the file, making the edit, running `cargo test -p brain`, and committing.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Fix `find_best_direction` early-out + missing `-135°` | `crates/brain/src/recover.rs` | pending | `break`→`continue`, add `-135.0` to `OFFSETS_DEG` |
| 2 | T2: Set `BUTTON_ANY` on every usercmd | `crates/brain/src/move_ctrl.rs` | pending | `buttons: 0` → `buttons: BUTTON_ANY` |
| 3 | T3: Check `lock_frames_remaining` before stale re-lock | `crates/brain/src/combat.rs` | pending | Guard the stale-target re-lock with `lock_frames_remaining > 0` |
