# Plan 58 — Shared locomotion extraction (`brain::locomotion`)

> **Status**: pending
> **Created**: 2026-07-11
> **Depends on**: Plan 46 (traversal executor), 48/50 (hazard gates), 51 (stall fixes)
> **Goal**: Extract the four near-identical path-following blocks (main/q3/zb2/runtester) into one shared `brain::locomotion::follow_path`, plus promote the duplicated lava-escape caller glue and roam-goal ladder — so the upcoming `xon` brain (Plan 60) lands on shared code instead of becoming a fifth copy.
> **Agent**: implementation agent (ralph-loop)

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Behavior-preserving DRY pass: one shared locomotion stage (nav update → steer → hazard creep → traversal gates → stuck recovery → jump edge → traversal apply), one shared lava-override helper, one shared roam-goal helper; all four locomoting brains delegate.

**Deliverables**:
1. `crates/brain/src/locomotion.rs` — `follow_path(...)` + `LocomotionState` (owns `Steering`, `Recovery`, `TraversalExecutor` wiring points), unit-tested against `StubNav`.
2. main, q3, zb2, runtester all delegating to it; their private copies deleted.
3. Shared `hazard::lava_override` (the health-gated escape caller glue currently copy-pasted in main/q3/zb2) and `brain::roam::roam_goal` (the roam-cursor ladder duplicated in main/q3).
4. Live no-regression proof per brain via the Plan 10 scenarios (spawn-to-spawn + swim + ride).

**Estimated effort**: Medium–Large (1–2 days)

## Context

### Why now

Plan 60 adds a fifth locomoting brain (`xon`). The canonical path-follow stage is currently duplicated with small drifts in four places:

- `crates/brain/src/brains/q3/mod.rs:238-340` (`locomote` — the cleanest shape)
- `crates/brain/src/brains/runtester.rs` (tick body)
- `crates/brain/src/brains/zb2.rs:~540-600`
- `crates/brain/src/brains/main.rs:~600-770`

Every past traversal/hazard fix (Plans 46, 48, 50, 51) had to be applied N times; Plan 48's L3 finding (main/q3 steering off raw `pursue_target` while the safe variant existed) was exactly this duplication biting. The user directive for the Xonotic series is explicit: *"share as much code as possible with the other bots, move things into common modules if needed."*

### Key Facts (from the seam audit, 2026-07-11)

- The canonical stage order (q3's `locomote`): `nav.update(pos, None)` → `nav.set_goal(goal, pos)` → `nav.smooth_with_cm(cm, pos)` → `nav.pursue_target_safe(pos, cm)` → yaw via `Steering::change_yaw` (`steer.rs:68`) → `move_from_world_dir` (`steer.rs:133`) → `arrive_scale` (`steer.rs:80`) → `hazard::creep_scale` (`hazard.rs:101`) → `traverse.gates(...)` (`traverse.rs:130`, suspends recovery/jump while any gate active) → `Recovery::evaluate` (`recover.rs:250`) with `hazard::safe_strafe_dir` (`hazard.rs:174`) → jump-edge (`nav.current_edge_is_jump() && !gates.any()`) → `traverse.apply(...)` **last** (`traverse.rs:182`).
- The copies are NOT byte-identical — main adds kite/flee weaving and item detours; zb2 re-expresses recovery legs against aim yaw (Plan 51 R2); runtester records extra telemetry. **Divergences must be preserved via hooks/params, not homogenized** — homogenizing is a behavior change and out of scope.
- The lava-escape caller glue (health gate + yaw override + jump + `EVT lava_escape` log) is duplicated at `main.rs:~823`, `zb2.rs:~722`, `q3/mod.rs:916-938`, all calling the already-shared `hazard::escape_from_lava` (`hazard.rs:129`).
- `roam_goal` (roam-nodes cursor + `roam_as_position` handling) is duplicated in `q3/mod.rs:211-231` and main.

### Why extract q3's shape

q3's `locomote` is the most recently audited copy (Plans 48/50/51 all patched it), is already a single function, and RunTester — the scenario-harness brain whose behavior is pinned by recorded baselines — is structurally closest to it. Extract q3's shape, express the other brains' drifts as optional hooks.

## Step-by-Step Tasks

### T1: `brain::locomotion` module + unit tests

**File**: `crates/brain/src/locomotion.rs` (new), `crates/brain/src/lib.rs`

**What to do**: Create `LocomotionState` owning nothing brain-specific — it *borrows* the brain's `Steering`, `Recovery`, and `TraversalExecutor` per call (they stay fields of each brain so brains keep their per-brain tuning and the traverse executor's cross-tick state). Signature sketch:

```rust
pub struct LocomotionHooks<'a> {
    /// zb2 (Plan 51 R2): re-express recovery legs against aim yaw instead of move yaw.
    pub legs_vs_aim_yaw: Option<f32>,
    /// main: pre-steer world-dir override (kite/flee weave). None = steer at pursue target.
    pub world_dir_override: Option<glam::Vec3>,
    pub _marker: std::marker::PhantomData<&'a ()>,
}

pub struct LocomotionOutcome {
    pub gates_active: bool,
    pub recovering: bool,
    pub steer_fwd: f32,
    pub steer_side: f32,
}

#[allow(clippy::too_many_arguments)]
pub fn follow_path(
    steering: &mut Steering,
    recovery: &mut Recovery,
    traverse: &mut TraversalExecutor,
    nav: &mut dyn Navigator,
    cm: Option<&CollisionModel>,
    view: &Worldview,
    goal: NavGoal,
    mv: &mut MovementIntent,
    dt: f32,
    hooks: LocomotionHooks<'_>,
) -> LocomotionOutcome
```

Order inside must match q3's `locomote` exactly (see Key Facts). Unit tests: `StubNav` + open `CollisionModel` — (a) straight-line path produces forward motion toward the pursue target, (b) jump edge sets `mv.jump()` when no gates, (c) gates suspend recovery (feed a swim-edge stub), (d) hook overrides take effect.

**Commit**: `task(T1): add brain::locomotion::follow_path (shared path-follow stage)`

### T2: migrate `runtester`

**File**: `crates/brain/src/brains/runtester.rs`

**What to do**: Replace the inline tick body's path-follow block with `follow_path`; keep runtester's recorder/telemetry wrapping intact. `intent_forward` must be sourced identically (recorder hindered-flag contract).

**Verify live** (server must run the matching map): `spawn-to-spawn --map q2dm1` exit 0; `spawn-to-weapon railgun --map q2dm1` reaches (swim, `S` flags present); `spawn-to-item quaddamage --map q2dm3` reaches (ride, `P` flags present). Compare SUMMARY lines against a fresh pre-change control run from the same session — `reached`, `path_efficiency`, `hindered_frames` in-family.

**Commit**: `task(T2): runtester delegates to brain::locomotion`

### T3: migrate `q3`

**File**: `crates/brain/src/brains/q3/mod.rs`

**What to do**: `locomote` (q3/mod.rs:238-340) becomes a thin wrapper over `follow_path` (q3 keeps its FSM/combat callers unchanged). Delete the inline copy.

**Verify live**: same three-scenario matrix as T2 with `--brain q3`; plus a 2-min `connect-one --brain q3` sanity (fights, no panics).

**Commit**: `task(T3): q3 locomote delegates to brain::locomotion`

### T4: migrate `main`

**File**: `crates/brain/src/brains/main.rs`

**What to do**: Express main's kite/flee weave through `world_dir_override`; the item-detour goal selection stays upstream (it changes the *goal*, not the follow stage). Delete the inline copy.

**Verify live**: three-scenario matrix `--brain mai`; 2-min `connect-one` sanity.

**Commit**: `task(T4): main delegates to brain::locomotion`

### T5: migrate `zb2`

**File**: `crates/brain/src/brains/zb2.rs`

**What to do**: zb2 follows its own committed polyline via the `Zb2Route` Navigator facade — `follow_path` takes `&mut dyn Navigator`, so it slots in; pass `legs_vs_aim_yaw` (Plan 51 R2). Delete the inline copy.

**Verify live**: three-scenario matrix `--brain zb2` (q2dm3 ride historically 1/4 for zb2 — the bar is "no worse", not "fixed"); 2-min `connect-one` sanity.

**Commit**: `task(T5): zb2 delegates to brain::locomotion`

### T6: promote lava-override glue + roam-goal ladder

**File**: `crates/brain/src/hazard.rs`, `crates/brain/src/roam.rs` (new), call sites in main/q3/zb2

**What to do**: (a) `hazard::lava_override(view, cm, pos, health, mv) -> bool` — the health-gated escape-from-lava caller block (yaw + jump + `EVT lava_escape` log), replacing the three copies; it stays OUTSIDE `follow_path` (it outranks everything including combat, per Plan 48). (b) `brain::roam::roam_goal(map, cursor, roam_as_position)` replacing the q3/main duplicates. Unit test each.

**Commit**: `task(T6): promote lava_override + roam_goal to shared modules`

### T7: docs + closeout

**Files**: `context/brain_notes.md`, `context/plans/SERIES.md`, this plan + tracker

**What to do**: Dated brain_notes append (what moved, hook inventory, live-matrix results table). `git mv` plan + tracker to `completed/`; SERIES → done.

**Commit**: `task(T7): brain_notes + close Plan 58`

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/locomotion.rs` | new shared follow-path stage | P0 |
| `crates/brain/src/brains/q3/mod.rs` | delete inline `locomote` body, delegate | P0 |
| `crates/brain/src/brains/main.rs` | delegate + `world_dir_override` hook | P0 |
| `crates/brain/src/brains/zb2.rs` | delegate + `legs_vs_aim_yaw` hook | P0 |
| `crates/brain/src/brains/runtester.rs` | delegate | P0 |
| `crates/brain/src/hazard.rs` | `lava_override` helper | P1 |
| `crates/brain/src/roam.rs` | shared roam-goal ladder | P1 |
| `context/brain_notes.md` | dated entry | P1 |

## Open Questions / Risks

1. **Hidden drift between the four copies** — a "cosmetic" difference may be load-bearing (Plan 51 was exactly this). *Mitigation*: diff each copy against q3's shape BEFORE migrating it; anything not expressible as a hook gets surfaced in the tracker as a decision, not silently homogenized. One brain per task, live-verified before the next.
2. **Recorder contract**: `intent_forward` and the flag stream feed the Plan 10 baselines. *Mitigation*: runtester migrates first (T2) and its scenario SUMMARY is the acceptance gate.
3. **Borrow-checker shape**: `follow_path` borrowing three `&mut` fields of the same brain struct is fine (disjoint fields), but `Worldview`/`cm` lifetimes may force the hooks struct to stay simple. *Mitigation*: keep hooks POD; if a brain needs a closure, add it only when that brain migrates.
4. **Scenario availability** — live checks need a running server per map. *Mitigation*: same convention as Plans 43/46 (server on `noir40.lan` historically); if unavailable, mark the task `blocked` in the tracker rather than claiming done.

## Verification Checklist

- [ ] T1: `cargo test -p brain` green incl. new locomotion unit tests
- [ ] T2: runtester s2s q2dm1 exit 0 + swim (`S`) + ride (`P`) scenarios reach; SUMMARY in-family vs same-session control
- [ ] T3: q3 three-scenario matrix + connect-one sanity, no panics
- [ ] T4: main three-scenario matrix + connect-one sanity, no panics
- [ ] T5: zb2 three-scenario matrix (ride "no worse" bar) + connect-one sanity
- [ ] T6: exactly one lava-override + one roam-goal implementation remain (`grep` proves no dupes); unit tests green
- [ ] T7: brain_notes dated entry on disk; plan+tracker in `completed/`; SERIES done
- [ ] Whole plan: `cargo build` zero warnings, `cargo clippy -- -D warnings` clean, `cargo fmt` applied, `cargo test` green before every commit
