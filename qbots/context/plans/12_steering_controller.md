# Plan 12 — Steering Controller (turn-then-go, look-ahead, anti-orbit, circle-strafe)

> **Status**: pending
> **Created**: 2026-06-15
> **Depends on**: Plan 11 (honest LOS — chasing must be toward a real target), Plan 10 (recorder to prove it)
> **Goal**: Replace the ad-hoc yaw/forward code in `bot_task` with a real steering controller so bots
> **turn to face a direction, then go**, pursue a look-ahead point along the path, never orbit a
> waypoint, and can circle-strafe an enemy — i.e. move like a human, not spin/wedge/slide.

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: A new `brain::steer::Steering` controller that owns turn-rate, look-ahead target
selection, arrive, facing-validated forward, and the aim/move-yaw split. The `bot_task`
intent-building block (`main.rs:636-718`) shrinks to "ask the steering controller for this
frame's `MovementIntent`".

**Deliverables**:
1. `Steering` struct holding: current view yaw, a max turn rate (deg/sec, skill-scaled),
   look-ahead config, arrive radius, and the last move-yaw (for smoothing).
2. **Turn-rate limiting** — port Eraser `M_ChangeYaw`: step `view_yaw` toward `ideal_yaw` by
   at most `yaw_speed·dt` (shortest arc), instead of snapping. Fixes inhuman instant facing
   + kills the orbit-jitter feedback.
3. **Look-ahead / pursue** — steer toward a point *ahead along the path* (not the raw node),
   so the bot cuts corners legally and stops zigging at grid nodes.
4. **Arrive** — scale `forward` down inside `ARRIVE_RADIUS` of the final goal so the bot
   doesn't overshoot and orbit the destination.
5. **Anti-orbit node-advance** — Z-aware threshold (Eraser `|dz|<16 && horiz<12`) **plus**
   "within `ORBIT_RADIUS` for > `ORBIT_FRAMES` → advance anyway". Kills "rotating in one spot".
6. **Facing-validated forward** — `forward *= clamp(cos(angle_to_ideal), 0..1)`; the bot
   turns first, accelerates as it aligns. Fixes "facing the wrong way while moving forward".
7. **Aim/move-yaw separation** — when engaging, `view_yaw` tracks the enemy (for aim) while
   `forward/side` are computed in a **movement plane** (chase / hold / circle-strafe), so the
   bot can face the enemy and strafe around it. (Currently one `mv.yaw` does both.)

**Estimated effort**: Large (2–3 days) — this is the heart of the series; each sub-mechanism
is independently testable and independently visible in the recorder logs.

---

## Context

### Pre-Identified Bugs (the steering layer)

The intent build in `bot_task` (`main.rs:636-718`) and `NavigationDriver::next_waypoint_direction`
(`nav.rs:177-186`) / `update` (`nav.rs:188-241`) have four compounding defects:

1. **Instant yaw snap.** `mv.look_at(yaw, pitch)` sets absolute yaw every tick
   (`main.rs:573/631/652`). Q2 has **no server-side turn cap** (client sends absolute
   `cmd.angles`; `PM_SetAngles` `pmove.c:1255`), so this *works* but is inhuman and — combined
   with defect #3 — produces the visible spin.
2. **Aim-yaw == move-yaw.** A single `mv.yaw` drives both where the bot looks and (since
   `forwardmove` is view-relative) where it walks. Engaging ⇒ walking straight at the enemy.
   No circle-strafe, no "face enemy, strafe to cover".
3. **Steer-at-raw-node + 64u 3D-Euclidean advance gate.** `next_waypoint_direction` returns
   the unit vector to `current_waypoint`'s node position; `update` advances only when
   `dist < 64.0` (3D). A node the bot can't close to 64u (Z lip of a ledge, drifted off the
   graph edge, node behind a lip) ⇒ the bot circles it, and `atan2(delta)` sweeps every tick ⇒
   **"rotating in one spot endlessly"**.
4. **No look-ahead / arrive.** The bot runs full speed at each grid node, overshoots the
   final goal, and orbits; between nodes it zigzags at grid angles (corner-clipping,
   slow elapsed time — also Plan 14's concern).

`delta_angles` is **not** implicated — it's correctly subtracted in `move_ctrl::angle_short`
for both axes, so aim and movement are world-correct. The errors are purely in *which*
world yaw/forward we choose and *how fast* we turn to it.

### Why [approach]: one controller, not scattered fixes

Today the decision is smeared across `main.rs` (forward sign from `enemy_dist`, three
`look_at` call sites, stuck back-up) and `nav.rs` (advance gate, give-up). Pulling it into a
single `Steering` type makes the priority order explicit and unit-testable:
**tactical dodge (Plan 07) > stuck recovery (Plan 13) > engage circle-strafe > pursue path > arrive**.
The controller returns a `MovementIntent`; `move_ctrl.build_cmd` is unchanged.

### Key facts (physics oracle — `vendor/yquake2/src/common/pmove.c`)

- `pm_maxspeed=300`, accel `10`, friction `6`, jump `+=270`. `forwardmove/sidemove` are
  ±400-scale but `wishspeed` is clamped to `pm_maxspeed` — so `forward∈[-1,1]` × 300 is the
  real speed envelope. No sqrt(2) diagonal clamp ⇒ a forward+side of (1,1) is *faster* than
  300 until clamped; keep `forward²+side² ≤ 1` if we ever strafe+advance together.
- Movement direction = `AngleVectors(viewangles)` ⇒ **view yaw and walk direction are locked
  together by the server**. To walk a direction `D` while *looking* at `E≠D`, a human uses
  `+forward`/`+moveleft` combos relative to view; i.e. we must decompose the desired world
  move vector into view-relative `(forward, side)`:
  `forward = dot(worldMoveDir, viewForward)`, `side = dot(worldMoveDir, viewRight)`.
  This is the mechanism for circle-strafe and is currently absent.

### Eraser references (distilled §4, §9)

- `bot_ChangeYaw` → `M_ChangeYaw`: `move = ideal−current` (shortest arc), clamp to `±yaw_speed`.
- Look-ahead/arrive: `bot_move` snaps velocity straight to goalentity ×10 when within 32u;
  Eraser trims X/Y velocity when overshooting in Z. We adapt as look-ahead + arrive.
- Node-advance `bot_ReachedTrail`: `|dz|<16 && horiz<12`.

---

## Step-by-Step Tasks

### T1: `Steering` skeleton + turn-rate limiting

**File**: `crates/brain/src/steer.rs` (new), export from `lib.rs`.

**What to do**:
```rust
pub struct Steering {
    view_yaw: f32,      // last commanded view yaw (deg) — the integrator
    yaw_speed_dps: f32, // max deg/sec (skill-scaled; see constants)
}
impl Steering {
    /// Step view_yaw toward ideal_yaw by at most yaw_speed_dps*dt, shortest arc.
    pub fn change_yaw(&mut self, ideal_yaw: f32, dt: f32) -> f32 { ... }
}
```
Shortest-arc: `diff = ((ideal - current + 540) % 360) - 180`; `step = clamp(diff, ±yaw_speed·dt)`.
Constants: `YAW_SPEED_BASE = 720.0` deg/s (a fast but human-ish turn — 0.5 s for a 180°);
skill-scaled `YAW_SPEED_BASE + (combat-1)*120` so high-combat bots snap faster. `dt` from the
tick cadence (≈0.1 s; derive from `msec` or measured frame delta, not a hardcoded 0.1).

**Tests** (`brain/tests/steer.rs`): shortest-arc (+179 vs −179 → +2 not −358); clamp at
`yaw_speed·dt`; skill scaling monotonic. Property: never overshoot ideal.

**Verify**: `cargo test -p brain`; `cargo clippy -p brain -- -D warnings`.

### T2: Facing-validated forward + move-vector decomposition

**File**: `crates/brain/src/steer.rs`.

**What to do**:
```rust
/// Given a desired world-space move direction and the current view yaw, produce
/// view-relative (forward, side) so the bot walks that way *while looking elsewhere*.
/// `throttle` scales by alignment when `face_then_go` (turn first, then accelerate).
pub fn move_from_world_dir(
    world_move_dir: Vec3,   // horizontal, normalized (or zero)
    view_yaw: f32,
    face_then_go: bool,
) -> (f32 /*forward*/, f32 /*side*/) {
    let fwd = world_move_dir.dot(view_forward(view_yaw));
    let side = world_move_dir.dot(view_right(view_yaw));
    if face_then_go {
        let align = fwd.max(0.0);            // negative = facing away → don't reverse-walk
        (fwd.abs().min(1.0) * align, side)    // throttle forward by alignment
    } else {
        (fwd.clamp(-1.0, 1.0), side.clamp(-1.0, 1.0))
    }
}
```
`face_then_go = true` for navigation (turn toward the path, accelerate as you align) — this
single change fixes "facing the wrong way while moving forward". For combat circle-strafe,
`face_then_go = false` (we *want* to strafe sideways while facing the enemy).

**Tests**: facing the move dir → `(1, 0)`; facing 90° off → forward throttled toward 0,
side ≈ ±1; facing away → forward ≈ 0 (don't moonwalk into walls).

**Verify**: `cargo test -p brain`.

### T3: Look-ahead target + arrive + anti-orbit advance

**File**: `crates/brain/src/nav.rs` (extend `NavigationDriver`), `crates/brain/src/steer.rs`.

**What to do**:
1. **Look-ahead pursuit target**: add `NavigationDriver::pursue_target(from) -> Option<Vec3>`
   that returns a point along the current path: walk forward through `current_path` accumulating
   distance until `LOOKAHEAD = 96.0` units from the bot, return that interpolated point (or the
   final goal if closer). The bot steers at *this*, not the raw next node → smooth corners, no
   grid zig. (`nav.rs` already stores `current_path`; interpolate between node positions.)
2. **Arrive**: in `Steering`, scale the move magnitude by `clamp(dist_to_goal / ARRIVE_RADIUS, 0.25, 1.0)`
   inside `ARRIVE_RADIUS = 80.0` of the **final** goal (not each node). Stops overshoot-orbit.
3. **Anti-orbit node-advance** (`nav.rs:188-208`): replace the single `dist < 64.0` gate with:
   ```rust
   let horiz = (wp_pos - pos).truncate().length();   // X/Y only
   let dz = (wp_pos.z - pos.z).abs();
   let reached = horiz < 16.0 && dz < 24.0;          // Eraser-ish, Z-tolerant for steps
   ```
   **plus** an orbit watchdog: track `near_wp_ticks`; if `horiz < ORBIT_RADIUS=80.0` for
   `ORBIT_FRAMES=15` (1.5 s) without reaching, **force-advance** to the next waypoint (the
   node is reachable enough; circling it is worse than moving on). Reset on advance.

**Before** (`nav.rs:195`): `if dist < 64.0 { /* advance */ }`.
**After**: Z-aware `reached` OR orbit-timeout → advance.

**Tests** (`brain/tests/steer.rs`, `nav.rs` tests): pursue target interpolates correctly
across two segments; arrive scales forward near goal; orbit-timeout advances after N frames
of hovering inside `ORBIT_RADIUS` without `reached`.

**Verify**: `cargo test -p brain`; `cargo clippy -p brain -- -D warnings`.

### T4: Wire `Steering` into `bot_task`

**File**: `crates/qbots/src/main.rs` (`bot_task`, ~`main.rs:636-718`).

**What to do**: Hold a `Steering` per bot (next to `move_ctrl`). Replace the three
`mv.look_at(...)` + `mv.move_forward(forward)` sites with a single call that:
1. Picks `ideal_yaw` by priority: dodge (Plan 07) > enemy-aim (if engaging & LOS) >
   path-pursue (from T3) > roam-heading.
2. `view_yaw = steering.change_yaw(ideal_yaw, dt)`.
3. Computes the world move dir: pursue-path (nav) / chase-enemy / back-up, applies arrive.
4. `(forward, side) = move_from_world_dir(world_move_dir, view_yaw, face_then_go)`.
5. Sets `mv.look_at(view_yaw, ideal_pitch)` (pitch only meaningfully non-zero for aim/lob) and
   `mv.move_forward(forward)` + `mv.move_side(side)`.

Keep `combat_dec.should_fire` driving `attack`/aim-pitch; keep ideal-distance (`main.rs:619-645`)
as the *world move dir* chooser (back-up / hold / advance) rather than a bare forward sign.

**Verify**: `cargo build -p qbots`, zero warnings; `cargo clippy -p qbots -- -D warnings`.
Re-run Plan-10 scenarios: `wrong_turns` and orbit-related `hindered` frames should drop hard.

### T5: Circle-strafe (engage movement)

**File**: `crates/brain/src/steer.rs` + `main.rs`.

**What to do**: When `BehaviorState::Engage` and LOS holds: `view_yaw` tracks the enemy (aim),
and the world move dir = a blend of (a) radial component toward/away from ideal-distance
(`BOT_IDEAL_DIST_FROM_ENEMY=160`) and (b) a tangential **strafe** component that flips every
`STRAFE_PERIOD = 3 s` (Eraser `strafe_dir`/`strafe_changedir_time`, distilled §0). Use
`move_from_world_dir(.., face_then_go=false)` so forward+side encode the radial+tangential
move while the view stays on the enemy. `combat` skill gates whether strafe is active (Eraser:
combat 1 = none).

**Verify**: engaging bot's log shows `face_delta` large (looking at enemy) while `move_yaw`
is tangential — i.e. it's moving perpendicular to its view. A targeted unit test on the blend.

### T6: Cleanup the dual control path

**File**: `crates/brain/src/fsm.rs`, `crates/qbots/src/main.rs`.

**What to do**: `FSM::engage` returns a dummy `CombatDecision { should_fire: true, aim_yaw: 0.0 }`
(`fsm.rs:170-176`) that `main.rs` mostly ignores (it uses `combat.evaluate`'s decision). With
the steering controller owning aim/move, remove the dummy and let `main.rs` derive aim purely
from `combat_dec` (or fold aim into `Steering` via an `aim_at(enemy)` that sets ideal_yaw+pitch).
Document the single source of truth for "where do I look" and "where do I walk".

**Verify**: no behavior regression in Plan-10 scenarios; `cargo test` green; clippy clean.

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/steer.rs` | NEW — `Steering`, `change_yaw`, `move_from_world_dir`, arrive, circle-strafe blend | P0 |
| `crates/brain/src/lib.rs` | export `steer` | P0 |
| `crates/brain/src/nav.rs` | `pursue_target`; Z-aware + orbit-timeout node-advance | P0 |
| `crates/qbots/src/main.rs` | replace the `look_at`/`move_forward` block with `Steering`; hold `Steering` per bot; circle-strafe in Engage | P0 |
| `crates/brain/src/fsm.rs` | drop the dummy `engage` CombatDecision (T6) | P1 |
| `crates/brain/src/move_ctrl.rs` | (unchanged) — `MovementIntent` gains explicit `move_side` use; verify `sidemove` already wired (`move_ctrl.rs:113`) | P2 |

---

## Open Questions / Risks

1. **`dt` source**: tick cadence is ~10 Hz but `clc_move` sends 3 × `msec=33` cmds. Use the
   *observed* frame interval (`frame.serverframe` deltas) for `change_yaw`, not a hardcoded
   0.1, so turn rate is correct under lag. If frames are irregular, clamp `dt` to `[0.02, 0.3]`.
2. **Turn-rate vs. combat snap**: a 720°/s turn is fast but not instant; at extreme ranges a
   bot may need >1 tick to face a flanking enemy and will (correctly) not fire until aligned.
   This is *desired* (human-like) but verify it doesn't tank frag rate vs. the snap baseline.
   Make `yaw_speed` config-tunable.
3. **`move_side` already in the intent** (`move_ctrl.rs:54`, sidemove wired at `:113`) —
   confirm nothing clamps `side` to 0 elsewhere (the dodge block at `main.rs:708-718` writes
   `mv.side` directly; T4/T5 must coexist with it, not fight it — dodge stays highest priority).
4. **Orbit-timeout masking unreachable nodes**: force-advancing past a node we never reached
   could skip a needed detour. Mitigation: only force-advance to the *next* node (not skip
   several), and let the give-up watchdog (Plan 13) handle truly stuck paths.
5. **Look-ahead through walls**: the pursue point is interpolated along graph edges, which are
   trace-validated at build time, so the segment is clear. But if the bot has drifted off the
   edge, the straight line to the pursue point may clip. Plan 13's reactive wall-avoidance is
   the backstop; note the interaction.
6. **Circle-strafe + diagonal speed**: forward+side both nonzero can exceed `pm_maxspeed`
   (no sqrt-2 clamp server-side). Normalize `(forward, side)` to magnitude ≤ 1 in
   `move_from_world_dir` when both are used.

---

## Verification Checklist

- [ ] T1: `change_yaw` shortest-arc + clamp + skill-scaling unit tests green.
- [ ] T2: `move_from_world_dir` alignment/strafe/back-facing unit tests green.
- [ ] T3: pursue-target interpolation, arrive scaling, orbit-timeout advance unit tests green.
- [ ] T3: `cargo clippy -p brain -- -D warnings` clean.
- [ ] T4: `cargo build -p qbots`, zero warnings; `cargo clippy -p qbots -- -D warnings` clean.
- [ ] T4: Plan-10 `spawn-to-spawn` shows **0 sustained orbit frames** and lower `wrong_turns` than baseline.
- [ ] T5: engaging bot log shows `face_delta` high while `move_yaw` is tangential (circle-strafe).
- [ ] T5: `(forward, side)` magnitude ≤ 1 (no overspeed) — recorder `max_speed` stays ≤ ~320 grounded.
- [ ] T6: dummy `engage` CombatDecision removed; single source of truth for look/move yaw.
- [ ] End-to-end: elapsed time to the farthest spawn **decreases** vs. the Plan-10 baseline.
