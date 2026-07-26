# Plan 71 — `main` brain bug fixes — Tracker

## Overview
- Status: 0% complete (0/6 tasks)
- Start date: 2026-07-16
- Plan revised: 2026-07-25 — all four original claims re-verified against `vendor/`.
  **2 were real bugs with wrong rationales, 2 were not bugs at all**, and 2 further genuine
  defects were found during verification. Net: 4 code defects + 2 housekeeping tasks.

## Verified Defects (4)
1. `recover.rs` `OFFSETS_DEG` asymmetric — has `+135°`, missing `-135°` (T1)
2. `recover.rs` `find_best_direction` returns `Some((yaw, 0.0))` for a fully-blocked
   direction, so a boxed-in bot steers into a wall and the stuck detector is starved (T3)
3. `move_ctrl.rs` never sets `BUTTON_ANY`, so an all-bot fleet **hangs at intermission
   forever** — nothing votes to advance the map (T4)
4. `combat.rs` stale-target pursuit is unbounded in age *and* distance, and re-arms its lock
   every tick, so a bot chases a ghost indefinitely and never returns to roam/item pickup (T5)

## Rejected Claims (2) — do NOT re-fix
- ❌ **"`find_best_direction`'s `break` skips better directions"** — not a bug. Score is
  hard-capped at `TRACE_DIST` (256) and the early-out fires only at `>= 255`, so nothing
  later can beat it by more than 1.0 unit. The plan's motivating example (0° scoring 250
  triggering the `break`) is arithmetically impossible. The `break` is verbatim from Eraser
  (`bot_nav.c:165`); `continue` as the loop's last statement would delete the early-out
  entirely for up to 12 extra hull traces per tick and ≤0.4° of yaw change. **Keep the `break`**
  — T3's `open_world_prefers_straight_ahead` test pins it.
- ❌ **"`combat.rs` stale path shadows a fresh target; guard on `lock_frames_remaining > 0`"** —
  not a bug, and the fix was harmful. Fresh selection is at `combat.rs:315-324`, *before* the
  stale block at `:326-340`, and returns on success; the two sets are disjoint anyway because
  `enemies()` filters `!is_stale` (`perception.rs:289`). Shadowing is impossible. The proposed
  guard is false on essentially every path reaching the stale block, so it would have **silently
  deleted Plan 11 T3's stale pursuit**. T5's `stale_target_pursued_within_bounds` test guards
  against this being reintroduced.

Full reasoning with vendor line citations is in the plan's `### Rejected Claims` section.

## Resume Instructions

Resume at the first `pending` task. **T2 must precede T3** (T3's tests need the
`closet_world()` helper). T1, T4, T5, T6 are otherwise independent and may be done in any
order. Each task: read the file → make the edit → `cargo fmt` → `cargo clippy -- -D warnings`
→ `cargo test` → **commit** (Rule B: commit before marking anything complete).

The one real unknown is T5's `view.server_frame()` — verify `Worldview` exposes the current
server frame before writing it; add a minimal accessor if not.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Restore fan-out symmetry (add `-135°`) | `crates/brain/src/recover.rs` | pending | 7 entries; fix the 2 doc comments; **keep the `break`** |
| 2 | T2: `closet_world()` test helper | `crates/world/src/collision.rs` | pending | `half_space` can't enclose a point; unblocks T3 tests |
| 3 | T3: Reject `fraction <= 0` directions | `crates/brain/src/recover.rs` | pending | Restores Eraser's `bot_nav.c:137` guard; first tests for `recover.rs` |
| 4 | T4: Set `BUTTON_ANY` when pressing anything | `crates/brain/src/move_ctrl.rs` | pending | Conditional, mirroring `cl_input.c:719-722` — not unconditional. Note intermission side effect in the commit |
| 5 | T5: Bound stale-target pursuit | `crates/brain/src/combat.rs` | pending | Age + distance bound, lock decays, clear `sight_grace_remaining`. Check `view.server_frame()` exists |
| 6 | T6: Register Plan 71 in `SERIES.md` | `context/plans/SERIES.md` | pending | Chain currently ends at Plan 70; no `brain_notes.md` append required |

## Post-Implementation Gate

Unit tests do not prove bots move or fight better. Before marking the plan done, re-run both
movement scenarios and record the numbers against the Baseline table in
`context/plans/completed/10_movement_test_harness_tracker.md`:

```bash
cargo run -p qbots -- spawn-to-spawn --map q2dm1
cargo run -p qbots -- spawn-to-weapon rocketlauncher --map q2dm1
```

T3 should help the no-nav-target path (a wedged bot now strafes instead of pressing into a
wall). If either scenario regresses, record that plainly here rather than declaring the plan
complete.
