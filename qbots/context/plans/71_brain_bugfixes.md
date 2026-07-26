# Plan 71 — `main` brain bug fixes (recovery, move_ctrl, combat)

> **Status**: pending
> **Created**: 2026-07-16
> **Depends on**: Plan 24 (main brain plugin)
> **Goal**: Fix four confirmed bugs in the `main` brain's stuck-recovery direction search, usercmd encoding, and combat stale-target handling.
> **Agent**: implementation agent (ralph-loop)

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Fix four bugs found during investigation of the `main` brain implementation: a `break` early-out in `find_best_direction` that skips better directions, a missing `-135°` direction in the recovery fan-out, `BUTTON_ANY` never set in usercmd encoding, and stale-target re-lock without checking `lock_frames_remaining`.

**Deliverables**:
1. `crates/brain/src/recover.rs`: replace `break` with `continue` in `find_best_direction`; add `-135.0` to `OFFSETS_DEG`
2. `crates/brain/src/move_ctrl.rs`: set `BUTTON_ANY` on every usercmd
3. `crates/brain/src/combat.rs`: check `lock_frames_remaining` before stale-target re-lock

**Estimated effort**: Small (2 h)

---

## Context

### Pre-Identified Bug/Issue

Four bugs were identified during investigation of the `main` brain (`crates/brain/src/brains/main.rs` and its supporting modules). All are confirmed by code inspection:

#### Bug 1: `find_best_direction` early-out `break` skips better directions (CRITICAL)

**File**: `crates/brain/src/recover.rs`, line 170-171

```rust
// Early-out: fully open in this direction.
if score >= TRACE_DIST - 1.0 {
    break;
}
```

The `break` exits the loop entirely when a fully-open direction is found. But `OFFSETS_DEG` is ordered `[0.0, 45.0, -45.0, 90.0, -90.0, 135.0]` — the early-out can fire on the **first** offset (0°) before checking ±45°, ±90°, or 135°. A direction that's 100% open at 0° causes the search to terminate, even if a *better-scoring* direction exists later in the array. The `break` should be `continue` — or better, the early-out should be removed entirely, since the function already picks the max-scoring direction via the `is_better` check.

This directly affects stuck recovery: a bot stuck in a corner might pick a partially-open direction (e.g., 0° with score 250) and `break` before finding the truly open 90° direction (score 256).

#### Bug 2: `OFFSETS_DEG` missing `-135°` direction

**File**: `crates/brain/src/recover.rs`, line 125

```rust
const OFFSETS_DEG: [f32; 6] = [0.0, 45.0, -45.0, 90.0, -90.0, 135.0];
```

The comment says "skip ±180°" but the array includes `+135.0` without `-135.0`. This means the bot only probes half of the ±135° pair — it checks forward-right but not forward-left at that angle. Eraser's original `botRoamFindBestDirection` tests 8 directions (±45°, ±90°, ±135°, 0°); this port only tests 6 and is missing the mirror of 135°.

#### Bug 3: `move_ctrl.rs` `buttons` field never set to `BUTTON_ANY`

**File**: `crates/brain/src/move_ctrl.rs`, line 126

```rust
buttons: 0,
```

The `buttons` field is initialized to 0 and only `BUTTON_ATTACK` is conditionally OR'd in. Q2's protocol requires `BUTTON_ANY` (bit 7) to be set on **every** usercmd that the player intends as input — it's the "this is a real command, not a resend" signal. Without it, the server may treat the cmd as stale/duplicate and drop it, especially on the first frame after a state change. The `BUTTON_ANY` constant is defined (line 15) but never used.

#### Bug 4: `combat.rs` stale-target path doesn't check `lock_frames_remaining`

**File**: `crates/brain/src/combat.rs`, lines 326-340

```rust
// Stale fallback: navigate to last-known pos but do not fire (no LOS).
let stale = view
    .entities()
    .filter(|e| e.class == EntityClass::EnemyPlayer && e.is_stale)
    .min_by(|a, b| { ... });

if let Some(t) = stale {
    self.current_target = Some(t.entity_number);
    self.lock_frames_remaining = TARGET_LOCK_FRAMES;
    return (Some(t.entity_number), false);
}
```

When a stale target is found, `lock_frames_remaining` is set to `TARGET_LOCK_FRAMES` but the stale entity may have been stale for a long time. The `lock_frames_remaining` counter is used in the next tick's `select_target_entity` to decide whether to keep the target (line 276: `self.lock_frames_remaining > 0`). But the stale target path doesn't check whether the entity is still actually stale — it just re-locks. If the entity becomes visible again (fresh), the stale path still fires because it runs before the fresh-selection path. The stale fallback should check `lock_frames_remaining` before re-locking.

### Key Facts

- `BUTTON_ANY = 128` (bit 7) per `common/header/shared.h:660`
- `OFFSETS_DEG` should have 7 entries (0°, ±45°, ±90°, ±135°) to match Eraser's 8-direction fan-out (skipping ±180°)
- `find_best_direction` already picks the max-scoring direction via `is_better` — the early-out is redundant and harmful
- The stale-target path in `combat.rs` runs after the locked-target path but before fresh selection, so it can shadow a freshly-visible target

---

## Step-by-Step Tasks

### T1: Fix `find_best_direction` early-out and missing direction

**File**: `crates/brain/src/recover.rs`

**What to do**:
1. Change `OFFSETS_DEG` to include `-135.0` (7 entries, ordered to preserve the early-out benefit)
2. Replace the `break` with `continue` so the search doesn't terminate early

**Before**:
```rust
const OFFSETS_DEG: [f32; 6] = [0.0, 45.0, -45.0, 90.0, -90.0, 135.0];
```
```rust
        // Early-out: fully open in this direction.
        if score >= TRACE_DIST - 1.0 {
            break;
        }
```

**After**:
```rust
const OFFSETS_DEG: [f32; 7] = [0.0, 45.0, -45.0, 90.0, -90.0, 135.0, -135.0];
```
```rust
        // Early-out: fully open in this direction — keep scanning for an even
        // better one (the array is ordered best-first, so this is a cheap win).
        if score >= TRACE_DIST - 1.0 {
            continue;
        }
```

### T2: Set `BUTTON_ANY` on every usercmd

**File**: `crates/brain/src/move_ctrl.rs`

**What to do**: Set `BUTTON_ANY` in the `buttons` field initialization so every usercmd carries the "real input" signal.

**Before**:
```rust
            buttons: 0,
```

**After**:
```rust
            buttons: BUTTON_ANY,
```

### T3: Fix stale-target re-lock in `combat.rs`

**File**: `crates/brain/src/combat.rs`

**What to do**: In the stale-target fallback path, check `lock_frames_remaining` before re-locking. If the lock has expired, don't re-lock — fall through to fresh selection.

**Before**:
```rust
        if let Some(t) = stale {
            self.current_target = Some(t.entity_number);
            self.lock_frames_remaining = TARGET_LOCK_FRAMES;
            return (Some(t.entity_number), false);
        }
```

**After**:
```rust
        if let Some(t) = stale {
            // Only re-lock if the lock hasn't expired — otherwise fall through
            // to fresh selection (the stale entity may now be visible).
            if self.lock_frames_remaining > 0 {
                self.current_target = Some(t.entity_number);
                self.lock_frames_remaining = TARGET_LOCK_FRAMES;
                return (Some(t.entity_number), false);
            }
        }
```

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/recover.rs` | Fix `OFFSETS_DEG` + `break`→`continue` | P0 |
| `crates/brain/src/move_ctrl.rs` | Set `BUTTON_ANY` on usercmd | P0 |
| `crates/brain/src/combat.rs` | Check `lock_frames_remaining` before stale re-lock | P1 |

---

## Open Questions / Risks

1. **Risk**: Changing `break` to `continue` in `find_best_direction` may change the direction selected in some cases. **Mitigation**: The function already picks the max-scoring direction via `is_better` — the `break` was only an optimization that was incorrect.
2. **Risk**: Adding `-135.0` to `OFFSETS_DEG` changes the array size from 6 to 7. **Mitigation**: The array is iterated by reference, so no indexing changes are needed.
3. **Risk**: Setting `BUTTON_ANY` on every usercmd may change server behavior. **Mitigation**: `BUTTON_ANY` is required by the Q2 protocol — its absence is the bug.

---

## Verification Checklist

- [ ] T1: `cargo test -p brain` passes with recovery tests green
- [ ] T1: `OFFSETS_DEG` has 7 entries including `-135.0`
- [ ] T1: `find_best_direction` uses `continue` not `break`
- [ ] T2: `cargo test -p brain` passes with move_ctrl tests green
- [ ] T2: `BUTTON_ANY` is set in `build_cmd` output
- [ ] T3: `cargo test -p brain` passes with combat tests green
- [ ] T3: stale-target path checks `lock_frames_remaining`
- [ ] Full `cargo clippy` clean, `cargo fmt` applied
- [ ] `just all` passes
