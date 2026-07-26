# Plan 71 — `main` brain bug fixes (recovery fan-out, BUTTON_ANY, stale-target pursuit)

> **Status**: pending
> **Created**: 2026-07-16
> **Revised**: 2026-07-25 — all four original claims re-verified against `vendor/`; two were
> wrong and are now recorded under *Rejected Claims*. Two survive with corrected rationales,
> and two genuine defects found during verification were added.
> **Depends on**: Plan 24 (main brain plugin)
> **Goal**: Fix the asymmetric recovery fan-out, the blocked-direction fallthrough, the missing
> intermission-exit signal, and unbounded stale-target pursuit in the `main` brain.
> **Agent**: implementation agent (ralph-loop)

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Fix four verified defects in the `main` brain: `find_best_direction`'s fan-out is
asymmetric (probes `+135°` but not `-135°`) and returns a fully-blocked direction as a valid
heading; `usercmd.buttons` never carries `BUTTON_ANY`, so an all-bot fleet can never advance the
map out of intermission; and the stale-target fallback pursues a ghost with no age or distance
bound, re-locking it every frame forever.

**Deliverables**:
1. `crates/brain/src/recover.rs` — add `-135.0` to `OFFSETS_DEG` (7 entries); fix the two
   doc comments that already disagree with the array
2. `crates/brain/src/recover.rs` — skip candidates whose trace `fraction <= 0` so `None`
   genuinely means "boxed in", restoring Eraser's `if (trace.fraction > 0)` guard
3. `crates/world/src/collision.rs` — `closet_world()` test helper (enclosed box) so the
   above is unit-testable; first tests for `recover.rs`
4. `crates/brain/src/move_ctrl.rs` — set `BUTTON_ANY` when the bot has any input this frame,
   mirroring the real client
5. `crates/brain/src/combat.rs` — bound stale-target pursuit by staleness age, stop refreshing
   the lock every frame, and clear `sight_grace_remaining` on the stale path
6. `context/plans/SERIES.md` — register Plan 71 (currently missing from the chain)

**Estimated effort**: Small–Medium (half day, tests included)

---

## Context

### Pre-Identified Bug/Issue

#### Bug 1: `OFFSETS_DEG` is asymmetric — probes `+135°` but not `-135°`

**File**: `crates/brain/src/recover.rs:125`

```rust
const OFFSETS_DEG: [f32; 6] = [0.0, 45.0, -45.0, 90.0, -90.0, 135.0];
```

Every other offset is present as a `±` pair; `135.0` is not. A bot wedged into a corner
therefore has one fewer escape candidate on one side than the other, and the bias is fixed in
world terms once you know its view yaw — it will preferentially peel out over its left shoulder
and never test the mirror.

The array also contradicts its own documentation in two places, so this is a typo, not a design
choice:

- `recover.rs:1` — module header says "**7-direction** fan-out"
- `recover.rs:119` — the function doc says "Test 6 directions fanning out from `view_yaw`
  (**±45°, ±90°, ±135°**, 0° — skip ±180°)" — which lists 7 directions while saying 6

Adding `-135.0` makes the array match both comments (7 entries) and restores symmetry.

> ⚠️ **The original justification for this task was false and must not be reinstated.** It
> claimed Eraser's `botRoamFindBestDirection` "tests 8 directions (±45°, ±90°, ±135°, 0°)".
> It does not — see *Key Facts* below. Eraser tests **five**, and deliberately skips ±90°.
> `main` is not "missing 2 of Eraser's 8"; it *added* ±90° and dropped one 135°. Keeping ±90°
> is a deliberate improvement (the sideways escape is exactly what `StuckLevel::Mild` strafing
> wants), so keep them — this task only restores the missing mirror.

#### Bug 2: a fully-blocked direction is returned as a valid heading (the real recovery bug)

**File**: `crates/brain/src/recover.rs:137-167`

The loop rejects `startsolid` but nothing else:

```rust
let t = cm.trace(&lifted, &end, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
if t.startsolid {
    continue;
}

let mut score = t.fraction * TRACE_DIST;
...
let is_better = best.map(|(_, s)| score > s).unwrap_or(true);
if is_better {
    best = Some((yaw, score));
}
```

A hull flush against a wall traces `fraction == 0` **without** `startsolid`. That candidate
scores `0.0`, and because `unwrap_or(true)` accepts the first candidate unconditionally, `best`
becomes `Some((yaw, 0.0))`. If every direction is blocked, the function returns
`Some((yaw, 0.0))` instead of `None` — directly contradicting its own doc ("or `None` if all
blocked", `recover.rs:120`).

The consequence is in `Recovery::evaluate` (`recover.rs:260-266`), which runs this **before**
the stuck detector, on every tick the bot has no nav target:

```rust
if !has_nav_target {
    if let Some(cm) = cm {
        if let Some((yaw, _)) = find_best_direction(cm, pos, view_yaw) {
            return RecoveryAction::UseHeading(yaw);
        }
    }
}
```

`Some((yaw, 0.0))` → `UseHeading(yaw)` → the bot walks *into the wall* and the recorder logs
`heading` as though recovery were working. Because `evaluate` returns early, the stuck detector
never even runs, so `Mild`/`Hard` escalation is skipped for as long as this persists.

**Eraser guards exactly this** and the port dropped the guard — `bot_nav.c:137` wraps the whole
scoring body in `if (trace.fraction > 0)`. Restoring it makes `None` mean what the doc says.

#### Bug 3: `BUTTON_ANY` is never set, so bots cannot exit intermission

**File**: `crates/brain/src/move_ctrl.rs:126` (`buttons: 0`); the constant is defined at
`move_ctrl.rs:15` and never used.

The real consequence is narrow but total: **an all-bot fleet hangs at intermission forever.**
When a DM server hits `fraglimit`/`timelimit` it enters intermission and waits for *some client*
to send a usercmd with `BUTTON_ANY` before advancing to the next map
(`yquake2/src/game/player/client.c:2112-2124`):

```c
if (level.intermissiontime)
{
    client->ps.pmove.pm_type = PM_FREEZE;
    /* can exit intermission after five seconds */
    if ((level.time > level.intermissiontime + 5.0) &&
        (ucmd->buttons & BUTTON_ANY))
    {
        level.exitintermission = true;
    }
    return;
}
```

No bot sends it → nothing sets `exitintermission` → the map never changes unless a human
presses a key. Eraser's own gamecode has the identical gate (`bot_nav.c:241-247`).

> ⚠️ **The original justification for this task was false and must not be reinstated.** It
> claimed `BUTTON_ANY` is "the 'this is a real command, not a resend' signal. Without it, the
> server may treat the cmd as stale/duplicate and drop it." **No such mechanism exists**
> anywhere in the Q2 protocol. Nothing in netchan, `sv_user.c`, or `pmove` reads `BUTTON_ANY`.
> yquake2 says so in as many words (`cl_keyboard.c:1490-1498`): *"the server reads this value
> and sends it to `gi->ClientThink()` where it's used to determine if the intermission shall
> end. Needless to say that **this is the only consumer of BUTTON_ANY**."*

**Set it conditionally, not unconditionally.** The real client only sets it when a key is
actually down (`cl_input.c:719-722`):

```c
if (anykeydown && cls.key_dest == key_game)
{
    cmd->buttons |= BUTTON_ANY;
}
```

Mirroring that keeps wire behaviour honest (a bot standing perfectly still is a bot pressing
nothing) and still satisfies the intermission gate, because a bot with a live brain is always
producing movement input. Unconditional is the lazier option and misrepresents idle bots.

**Accepted side effect, stated up front**: bots will now vote to exit intermission ~5 s in. On a
server shared with humans, that means 5 s of scoreboard instead of an indefinite wait. That is
the intended behaviour for a bot fleet, but it *is* a user-visible change — note it in the
commit message.

#### Bug 4: stale-target pursuit is unbounded and re-locks a ghost forever

**File**: `crates/brain/src/combat.rs:326-340`

```rust
let stale = view
    .entities()
    .filter(|e| e.class == EntityClass::EnemyPlayer && e.is_stale)
    .min_by(|a, b| { /* nearest */ });

if let Some(t) = stale {
    self.current_target = Some(t.entity_number);
    self.lock_frames_remaining = TARGET_LOCK_FRAMES;
    return (Some(t.entity_number), false);
}
```

Three separate problems, all of which let a bot chase a ghost indefinitely:

1. **No age bound.** `is_stale` is a boolean latch set once `serverframe - last_seen_frame >
   STALE_THRESHOLD` (`perception.rs:34,208` — 10 frames ≈ 1 s) and *never cleared while the
   entity remains unseen*. An enemy last seen 60 s ago is exactly as attractive a target as one
   last seen 1.1 s ago. `PerceivedEntity` already carries `last_seen_frame`
   (`perception.rs:66`), so the true age is available and simply unused here.
2. **No distance bound.** `min_by` picks the *nearest* stale enemy with no ceiling, so a bot
   will cross the whole map toward a position an enemy left a minute ago.
3. **The lock is refreshed to full every frame.** `lock_frames_remaining = TARGET_LOCK_FRAMES`
   re-arms on every tick the stale entity is still in the table, so pursuit can never time out
   on its own. The bot is pinned in `Engage`-adjacent behaviour and never returns to roam/item
   collection.

Fix: bound by age, bound by distance, and let the lock **decay** rather than re-arm, so pursuit
gives up by itself. Also clear `sight_grace_remaining` on this path — it is left at whatever the
previous target set it to (`combat.rs:99,285`), so a leftover grace value can leak into the next
locked-target evaluation and buy a fresh target free frames of "LOS holds".

### Rejected Claims

Both were investigated in full and are **not bugs**. Recorded here so a future pass does not
"re-fix" them.

#### ❌ Rejected: "`find_best_direction`'s early-out `break` skips better directions" (was Bug 1, rated CRITICAL)

**Claim**: the `break` at `recover.rs:170-171` can fire on the first offset (0°) and skip a
better-scoring direction later in the array; it should be `continue`.

**Why it is wrong**: `score = t.fraction * TRACE_DIST` and `fraction <= 1.0`, so score is hard-
capped at `TRACE_DIST` (256). The early-out fires only at `score >= TRACE_DIST - 1.0`, i.e.
`>= 255`. **No later direction can beat 255 by more than 1.0 unit**, because 256 is the ceiling.
The plan's own motivating example — "0° with score 250 causes `break` before finding the 90°
direction at 256" — is arithmetically impossible: 250 < 255, so the `break` does not fire.

The `break` is also **verbatim from the reference implementation**
(`bot_nav.c:165`: `if (this_dist == TRACE_DIST) break;`). The port's `>= TRACE_DIST - 1.0` is
the correct float-safe form of Eraser's exact `==` comparison.

Changing it to `continue` would be a pure regression: `continue` as the last statement of a loop
body is a no-op, so the "After" block deletes the early-out entirely, costing up to 12 extra
hull traces per call — in a function called *every tick* whenever the bot has no nav target —
to change the selected yaw by at most 0.4°. And the proposed replacement comment ("keep scanning
for an even better one … the array is ordered best-first, so this is a cheap win") would
describe an optimization that no longer exists. **Keep the `break`.** The genuine defect in this
function is Bug 2 above, which the `break` claim was masking.

#### ❌ Rejected: "`combat.rs`'s stale path shadows a freshly-visible target; guard it on `lock_frames_remaining > 0`" (was Bug 4)

**Claim (a)**: "the stale-target path runs after the locked-target path **but before fresh
selection**, so it can shadow a freshly-visible target."

**Why it is wrong**: the ordering is backwards. In `select_target_entity` the fresh-selection
block is `combat.rs:315-324` and the stale fallback is `combat.rs:326-340` — fresh comes
**first** and `return`s on success, so the stale path is unreachable whenever a fresh target
exists. Independently, the two sets are **disjoint by construction**: fresh selection goes
through `nearest_visible_enemy`/`nearest_enemy` → `enemies()`, which filters `!e.is_stale`
(`perception.rs:289`), while the stale block filters `e.is_stale`. An entity cannot be in both.
Shadowing is not possible.

**Claim (b)**: the proposed fix — only re-lock a stale target `if self.lock_frames_remaining > 0`.

**Why it is harmful**: it would make the stale fallback **near-dead code and silently delete
Plan 11 T3's stale pursuit.** Every path that reaches `combat.rs:336` has already either
expired the lock (`lock_frames_remaining = 0`, set at `combat.rs:289` when LOS grace runs out)
or never had one (`current_target == None`), so the guard is false in essentially every case
that matters. Bots would stop navigating toward a last-known enemy position altogether. The one
surviving path — target dropped out of `enemies()` while the counter still had frames left —
immediately re-arms the counter to `TARGET_LOCK_FRAMES` and latches anyway, so the guard does
not even bound pursuit. It is strictly worse than the status quo in both directions.

The defect the claim was groping at is real but different: unboundedness, not ordering. That is
Bug 4 above.

### Key Facts

- **`BUTTON_ANY = 128`** — `yquake2/src/common/header/shared.h:673`. Its **only** consumer
  anywhere is the intermission-exit check in `ClientThink`
  (`yquake2/src/game/player/client.c:2112-2124`), stated explicitly in
  `yquake2/src/client/cl_keyboard.c:1490-1498`. The client sets it conditionally on
  `anykeydown` (`yquake2/src/client/cl_input.c:719-722`).
- **Eraser's `botRoamFindBestDirection` tests five directions, not eight.** The loop
  (`bot_nav.c:125-131`) is:
  ```c
  for (i=1; i<8; i++) {
      if (i==4) i=6;                      // ← skips i=4 and i=5, i.e. ±90°
      this_angle[1] = anglemod(angle[1] + ((((i % 2)*2 - 1) * (int)(floor(i/2))) * 45));
  ```
  Offsets by iteration: `i=1 → 0°`, `i=2 → -45°`, `i=3 → +45°`, `i=4→6 → -135°`,
  `i=7 → +135°`. The `// check eight compass directions` comment at `bot_nav.c:115` is
  aspirational and wrong — `if (i==4) i=6` prunes ±90° deliberately. Set = `{0, ±45, ±135}`.
- **Eraser guards on `trace.fraction > 0`** (`bot_nav.c:137`) before scoring a direction at all.
  The `main` port only rejects `startsolid` — this is Bug 2.
- **Eraser's early-out `break` is real** — `bot_nav.c:165`, nested inside
  `if (this_dist > best_dist)` (`bot_nav.c:160`).
- `STALE_THRESHOLD = 10` frames ≈ 1 s at 10 Hz (`perception.rs:34`); `is_stale` is a latch that
  is never cleared while the entity stays unseen (`perception.rs:208-209`).
- `PerceivedEntity.last_seen_frame` (`perception.rs:66`) already exposes true staleness age.
- `TARGET_LOCK_FRAMES = 5` (~0.5 s at 10 Hz), `TICK_HZ = 10.0` (`combat.rs:20,40`).
- `CollisionModel` has no `Default`; tests build geometry with `CollisionModel::half_space`
  (`collision.rs:195`) or the doc-hidden `water_channel_world()` / `v_groove_world()`
  (`collision.rs:662,748`). A single half-space **cannot** enclose a point, so Bug 2's
  "all directions blocked" case needs a new multi-plane helper — see T3.
- `crates/brain/src/recover.rs` currently has **no test module at all**.

---

## Step-by-Step Tasks

> **Rule B reminder: commit at the end of every task.** `cargo fmt` + `cargo clippy -- -D
> warnings` + `cargo test -p brain` (and `-p world` for T3) must be clean *before* each commit.
> Never batch two tasks into one commit.

### T1: Restore fan-out symmetry (add `-135°`)

**File**: `crates/brain/src/recover.rs`

**What to do**: Add the missing `-135.0` mirror and fix the two doc comments that already
disagree with the array. Keep `±90°` (a deliberate improvement over Eraser) and keep the
`break` (see *Rejected Claims*).

**Before** (`recover.rs:119-125`):
```rust
/// Test 6 directions fanning out from `view_yaw` (±45°, ±90°, ±135°, 0° — skip ±180°).
/// Returns the `(yaw_degrees, score)` of the most open direction, or `None` if all blocked.
///
/// Port of Eraser `botRoamFindBestDirection` (`bot_nav.c:96-176`). (Plan 13 T2)
pub fn find_best_direction(cm: &CollisionModel, origin: Vec3, view_yaw: f32) -> Option<(f32, f32)> {
    // 6 angular offsets relative to view_yaw (skip ±180°).
    const OFFSETS_DEG: [f32; 6] = [0.0, 45.0, -45.0, 90.0, -90.0, 135.0];
```

**After**:
```rust
/// Test 7 directions fanning out from `view_yaw` (0°, ±45°, ±90°, ±135° — skip ±180°).
/// Returns the `(yaw_degrees, score)` of the most open direction, or `None` if all blocked.
///
/// Port of Eraser `botRoamFindBestDirection` (`bot_nav.c:96-176`), which tests only
/// `{0, ±45, ±135}` — its `if (i==4) i=6` (`bot_nav.c:127`) deliberately prunes ±90°
/// despite the "eight compass directions" comment above it. We keep ±90° because a pure
/// side-step is exactly the escape `StuckLevel::Mild` strafing wants. (Plan 13 T2, Plan 71 T1)
pub fn find_best_direction(cm: &CollisionModel, origin: Vec3, view_yaw: f32) -> Option<(f32, f32)> {
    // 7 angular offsets relative to view_yaw, ordered 0° first then outward in ± pairs so
    // ties favour straight ahead (`is_better` uses strict `>`). Skip ±180°.
    const OFFSETS_DEG: [f32; 7] = [0.0, 45.0, -45.0, 90.0, -90.0, 135.0, -135.0];
```

Also update the module header at `recover.rs:1` — it already says "7-direction fan-out", so
confirm it now matches rather than changing it.

**Verify**: no indexing depends on the array length (it is iterated by reference), so the size
change is inert. `cargo clippy` must stay clean.

### T2: Add a `closet_world()` test helper to `world`

**File**: `crates/world/src/collision.rs`

**What to do**: T3 needs a collision model that **encloses** a point, which
`CollisionModel::half_space` cannot express (one plane). Add a doc-hidden test-support helper
next to `water_channel_world()` (`collision.rs:662`) and `v_groove_world()` (`collision.rs:748`),
following their exact construction pattern (`mk` plane closure, leaves encoded as `-(leaf+1)`).

- Signature: `#[doc(hidden)] pub fn closet_world(half_extent: f32) -> CollisionModel`
- Geometry: air inside the axis-aligned box `|x| < half_extent`, `|y| < half_extent`,
  `z >= 0`; `CONTENTS_SOLID` everywhere else. Four vertical walls plus a floor is enough —
  no ceiling needed, the fan-out traces are horizontal.
- Doc comment must say it is test-support only and name its consumer
  (`brain::recover` "boxed in" tests), matching the style of the two existing helpers.

**Test** (in `crates/world/src/collision.rs` tests): with `half_extent = 40.0`, assert a point
at the origin is not solid, that a point at `(200, 0, 16)` is solid, and that a horizontal
`trace` from the origin outward in each of ±x/±y returns `fraction < 1.0`.

**Why a separate task**: it lands in a different crate than T3 and is independently useful, so
per Rule B it gets its own commit.

### T3: Reject fully-blocked directions in `find_best_direction`

**File**: `crates/brain/src/recover.rs`

**What to do**: Restore Eraser's `if (trace.fraction > 0)` guard (`bot_nav.c:137`) so a
`fraction == 0` candidate is never adopted as `best`. This makes the documented "`None` if all
blocked" contract true, and stops `Recovery::evaluate` from emitting
`UseHeading(<straight into a wall>)` and short-circuiting the stuck detector.

**Before** (`recover.rs:136-141`):
```rust
        let t = cm.trace(&lifted, &end, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        if t.startsolid {
            continue;
        }

        let mut score = t.fraction * TRACE_DIST;
```

**After**:
```rust
        let t = cm.trace(&lifted, &end, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        // `startsolid` = already inside a brush. `fraction <= 0` = hull flush against a wall,
        // which does NOT set startsolid but yields a zero-length move. Eraser rejects both
        // (`bot_nav.c:137` wraps the whole scoring body in `if (trace.fraction > 0)`); the
        // original port only rejected startsolid, so a boxed-in bot got `Some((yaw, 0.0))`
        // back and `Recovery::evaluate` steered it into the wall as `UseHeading` — while
        // returning early and starving the stuck detector. (Plan 71 T3)
        if t.startsolid || t.fraction <= 0.0 {
            continue;
        }

        let mut score = t.fraction * TRACE_DIST;
```

**Tests** — `recover.rs` has no test module yet; create one:

1. `boxed_in_returns_none`: `closet_world(40.0)` (from T2) with the bot at the centre — every
   horizontal trace is blocked, so `find_best_direction` returns `None`. **Fails before this
   change** (returns `Some((_, 0.0))`); this is the regression test for the bug.
2. `never_returns_zero_score`: over a spread of `view_yaw` values in the closet world and in
   `half_space([1.0, 0.0, 0.0], 0.0)` with the hull flush at `x = -16.0` (`HULL_MAXS.x`),
   assert that any `Some((_, score))` has `score > 0.0`.
3. `open_world_prefers_straight_ahead`: with an all-clear model
   (`half_space([0.0, 0.0, 1.0], -100_000.0)`, the idiom already used at `pursuit.rs:105`),
   assert the returned yaw equals `view_yaw` — pins the tie-break-favours-0° ordering and the
   `break` early-out, so a future pass cannot quietly reintroduce the rejected `continue`.
4. `symmetric_fanout_probes_both_135s`: sanity-check T1 by asserting `OFFSETS_DEG.len() == 7`
   and that for every offset its negation is also present.

### T4: Set `BUTTON_ANY` when the bot has input

**File**: `crates/brain/src/move_ctrl.rs`

**What to do**: Mirror `cl_input.c:719-722` — set `BUTTON_ANY` when the bot is actually pressing
something this frame, so the intermission gate in `ClientThink` can fire. Do **not** set it
unconditionally: an idle bot presses nothing, and the wire should say so.

**Before** (`move_ctrl.rs:126-137`):
```rust
            buttons: 0,
        };

        if intent.jump {
            cmd.upmove = JUMP_VELOCITY as i16;
        }
        if intent.crouch {
            // Crouch is typically handled by viewoffset, not a button
        }
        if intent.attack {
            cmd.buttons |= BUTTON_ATTACK;
        }
```

**After**:
```rust
            buttons: 0,
        };

        if intent.jump {
            cmd.upmove = JUMP_VELOCITY as i16;
        }
        if intent.crouch {
            // Crouch is typically handled by viewoffset, not a button
        }
        if intent.attack {
            cmd.buttons |= BUTTON_ATTACK;
        }

        // `BUTTON_ANY` mirrors the real client's `anykeydown` (`cl_input.c:719-722`): set it
        // whenever we are pressing *anything*. Its ONLY consumer server-side is the
        // intermission-exit gate in `ClientThink` (`game/player/client.c:2112-2124`): after
        // intermission + 5 s, a usercmd carrying BUTTON_ANY sets `level.exitintermission`.
        // Without it an all-bot fleet hangs at the scoreboard forever, because no client ever
        // votes to advance the map. It is NOT a keepalive or anti-resend flag — nothing in
        // netchan or pmove reads it (`cl_keyboard.c:1490-1498`).
        let pressing_anything = cmd.buttons != 0
            || cmd.forwardmove != 0
            || cmd.sidemove != 0
            || cmd.upmove != 0
            || cmd.impulse != 0;
        if pressing_anything {
            cmd.buttons |= BUTTON_ANY;
        }
```

**Tests** (extend the existing `mod tests` at `move_ctrl.rs:175`):

1. `build_cmd_sets_button_any_when_moving`: `move_forward(1.0)` → `cmd.buttons & BUTTON_ANY != 0`.
2. `build_cmd_sets_button_any_when_attacking`: attack-only intent (no movement) → both
   `BUTTON_ATTACK` and `BUTTON_ANY` set.
3. `build_cmd_omits_button_any_when_idle`: a default `MovementIntent` → `cmd.buttons == 0`.
4. Extend `test_build_cmd` (`move_ctrl.rs:246`) to assert `BUTTON_ANY` alongside `BUTTON_ATTACK`.

**Commit message must note the user-visible side effect**: bots now vote to exit intermission
~5 s in, so a human sharing the server gets 5 s of scoreboard rather than an indefinite wait.

### T5: Bound stale-target pursuit

**File**: `crates/brain/src/combat.rs`

**What to do**: Keep the stale fallback (it is Plan 11 T3's last-known-position pursuit and must
not be deleted — see *Rejected Claims*) but bound it in all three of the ways it currently is
not. Add two constants near `TARGET_LOCK_FRAMES` (`combat.rs:20`):

```rust
/// Give up on a stale (out-of-PVS) enemy after this long without a sighting. `is_stale` is a
/// latch that never clears while the entity stays unseen (`perception.rs:208`), so without an
/// age bound a bot will chase a position an enemy left a minute ago. ~3 s at [`TICK_HZ`].
const STALE_PURSUE_MAX_FRAMES: i32 = 30;

/// Don't cross the map for a ghost. Beyond this the last-known position is stale enough in
/// space as well as time that roaming toward items is the better play.
const STALE_PURSUE_MAX_DIST: f32 = 1024.0;
```

**Before** (`combat.rs:326-340`):
```rust
        // Stale fallback: navigate to last-known pos but do not fire (no LOS).
        let stale = view
            .entities()
            .filter(|e| e.class == EntityClass::EnemyPlayer && e.is_stale)
            .min_by(|a, b| {
                let da = (a.origin - view.self_state().origin).length_squared();
                let db = (b.origin - view.self_state().origin).length_squared();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            });

        if let Some(t) = stale {
            self.current_target = Some(t.entity_number);
            self.lock_frames_remaining = TARGET_LOCK_FRAMES;
            return (Some(t.entity_number), false);
        }
```

**After**:
```rust
        // Stale fallback: navigate to last-known pos but do not fire (no LOS). Bounded in age
        // AND distance, and the lock is allowed to DECAY rather than re-arm — otherwise a bot
        // latches onto a ghost, re-locks it every tick, and never returns to roam/item pickup.
        let now = view.server_frame();
        let stale = view
            .entities()
            .filter(|e| e.class == EntityClass::EnemyPlayer && e.is_stale)
            .filter(|e| now - e.last_seen_frame <= STALE_PURSUE_MAX_FRAMES)
            .filter(|e| {
                (e.origin - view.self_state().origin).length() <= STALE_PURSUE_MAX_DIST
            })
            .min_by(|a, b| {
                let da = (a.origin - view.self_state().origin).length_squared();
                let db = (b.origin - view.self_state().origin).length_squared();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            });

        if let Some(t) = stale {
            self.current_target = Some(t.entity_number);
            // Decay, don't re-arm: only seed the lock when switching onto a new ghost. A
            // stale target we are already pursuing burns down its counter and expires.
            if prev_stale != Some(t.entity_number) {
                self.lock_frames_remaining = TARGET_LOCK_FRAMES;
            }
            // No LOS on a stale target, so it must not inherit the previous target's grace.
            self.sight_grace_remaining = 0;
            return (Some(t.entity_number), false);
        }
```

**Implementation notes** (resolve these while coding — they are the only unknowns):

- `prev_stale` needs the pre-call `self.current_target`. Capture it at the top of
  `select_target_entity` (`let prev_stale = self.current_target;`) *before* the locked-target
  block mutates it at `combat.rs:288`.
- **`view.server_frame()` may not exist** — check `Worldview` before writing it. The frame
  number is already threaded in for the `is_stale` computation (`perception.rs:208` uses
  `frame.serverframe`), so either a public accessor exists or add a small one alongside
  `self_state()`. Do not duplicate the frame number into `CombatDriver`.
- `last_seen_frame` is `i32` (`perception.rs:66`); keep the arithmetic in `i32` and do not
  cast to `u32` — a negative difference must stay representable.

**Tests** (extend the existing `mod tests` at `combat.rs:401`, following the `Frame`/
`EntityState` fixture style already used at `perception.rs:563-590`):

1. `stale_target_pursued_within_bounds`: one stale enemy 300 u away, last seen 15 frames ago →
   returned as the target with `fire_allowed == false`. **Guards against the rejected fix**,
   which would have broken exactly this.
2. `stale_target_dropped_when_too_old`: same enemy, last seen 60 frames ago → `None`.
3. `stale_target_dropped_when_too_far`: last seen 15 frames ago but 4000 u away → `None`.
4. `stale_lock_decays_across_ticks`: pursue the same stale enemy over consecutive calls and
   assert `lock_frames_remaining` strictly decreases rather than pinning at
   `TARGET_LOCK_FRAMES`.
5. `stale_path_clears_sight_grace`: seed `sight_grace_remaining > 0` via a fresh target, then
   fall through to the stale path, and assert it is 0.

### T6: Register Plan 71 in `SERIES.md`

**File**: `context/plans/SERIES.md`

**What to do**: Plan 71 is **absent** from the chain — the table currently ends at Plan 70.
Add a row in the established format (`| **71** | … | deps | status | notes |`) with
`Depends on: 24`, status `pending` → flip to `done` on completion per Rule C.

The `brain_notes.md` discipline listed at the foot of `SERIES.md` names Plans 23–33, 36–38, 40,
43, 58–62 — **71 is not in that set**, so no brain-notes append is required. Do not add one
speculatively.

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/recover.rs` | Add `-135.0`; reject `fraction <= 0`; first test module | P0 |
| `crates/brain/src/move_ctrl.rs` | Set `BUTTON_ANY` when pressing anything; tests | P1 |
| `crates/brain/src/combat.rs` | Bound stale pursuit (age + distance + lock decay); tests | P1 |
| `crates/world/src/collision.rs` | `closet_world()` test helper | P2 (unblocks T3 tests) |
| `context/plans/SERIES.md` | Register Plan 71 | P2 |

**Task order**: T1 → T2 → T3 → T4 → T5 → T6. T2 must precede T3 (its tests need the helper).
T4 and T5 are independent of the rest and may be done in any order.

---

## Open Questions / Risks

1. **`Worldview` may not expose the current server frame** (needed by T5's age bound). Check
   before writing `view.server_frame()`; add a minimal accessor if absent rather than caching
   the frame number in `CombatDriver`. *This is the one real unknown in the plan.*
2. **`STALE_PURSUE_MAX_FRAMES = 30` and `STALE_PURSUE_MAX_DIST = 1024.0` are first guesses.**
   They are not from any vendor source — Eraser has no direct equivalent. Both are a
   deliberate behaviour change: bots will abandon distant ghosts they previously chased. If
   post-change logs show bots disengaging too readily, raise the distance bound first (it is
   the cruder of the two). Document whatever the final values are and why.
3. **Risk: T3 changes what `find_best_direction` returns in genuinely boxed-in cases** —
   `Some((yaw, 0.0))` becomes `None`, so `Recovery::evaluate` no longer returns early and the
   stuck detector runs instead, escalating to `Mild` → `Strafe`. **That is the point** (a
   strafe can break a wedge; walking into a wall cannot), but it is a real behavioural change
   in the no-nav-target path and should be watched in the first movement-scenario run.
4. **Risk: T4 makes bots advance the map at intermission.** Intended (see T4), but a
   user-visible change on a shared server. Flagged, not mitigated.
5. **Not a risk: the `break` early-out.** Verified as correct and vendor-faithful; see
   *Rejected Claims*. T3's `open_world_prefers_straight_ahead` test pins it so it is not
   silently removed later.
6. **Verification gap**: these are all behavioural changes in the movement/combat path, and
   unit tests alone will not show whether bots actually move or fight better. The
   movement-scenario runs in the checklist below are the real gate.

---

## Verification Checklist

- [ ] T1: `OFFSETS_DEG` has 7 entries and every offset's negation is present
- [ ] T1: both doc comments (`recover.rs:1`, `recover.rs:119`) agree with the array
- [ ] T1: the `break` early-out is **still present** (not converted to `continue`)
- [ ] T2: `closet_world()` is `#[doc(hidden)]`, documented as test-support, `cargo test -p world` green
- [ ] T3: `boxed_in_returns_none` fails on the pre-change code and passes after
- [ ] T3: no `Some((_, score))` with `score <= 0.0` is ever returned
- [ ] T3: `recover.rs` has a test module where it previously had none
- [ ] T4: `BUTTON_ANY` set when moving/attacking, **absent** on a default idle intent
- [ ] T4: commit message notes the intermission side effect
- [ ] T5: stale pursuit still works inside the bounds (the rejected fix would have killed it)
- [ ] T5: stale pursuit gives up on age, on distance, and by lock decay
- [ ] T5: `sight_grace_remaining` is cleared on the stale path
- [ ] T6: Plan 71 appears in `SERIES.md`
- [ ] `cargo fmt` applied; `cargo clippy -- -D warnings` clean; `cargo test` green (Rule A)
- [ ] `just all` passes
- [ ] A commit exists for **every** task, none batched (Rule B)
- [ ] Re-run both movement scenarios against the Plan 10 baseline and record the result:
      `cargo run -p qbots -- spawn-to-spawn --map q2dm1` and
      `cargo run -p qbots -- spawn-to-weapon rocketlauncher --map q2dm1`.
      T3 is expected to help the no-nav-target path; if either regresses versus the
      `10_movement_test_harness_tracker.md` Baseline table, say so plainly in the tracker
      rather than declaring the plan done
- [ ] Rule C: `git mv` plan + tracker to `completed/` and mark `SERIES.md` **done**
