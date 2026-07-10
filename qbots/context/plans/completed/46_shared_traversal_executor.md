# Plan 46 — Shared traversal executor: ladder/swim/ride parity for ALL brains

> **Status**: pending
> **Created**: 2026-07-09
> **Depends on**: Plan 40 (swim movement), Plan 43 (ride behavior), Plan 24/25 (brain plugins)
> **Goal**: One shared `brain::traverse` executor for ladder climbs, swimming/water-exit, and platform/lift rides — consumed by **every** brain — so `main` and `q3` bots traverse the whole map in live matches, not just the `runtester` scenario brain.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Traversal execution is currently scattered and unequal: `RunTesterBrain` has the
full set (ladder ascent/descent + stateful train riding, `brains/runtester.rs:250-399`),
`MainBrain` has swim + a generic ride block but **no ladder handling**
(`brains/main.rs:546-620`), and **`Q3Brain` has none** — a `q3` bot in a real match cannot
swim, ride a lift/train, or climb a ladder. Extract one executor; all brains call it.

**Deliverables**:
1. `crates/brain/src/traverse.rs` — a stateful `TraversalExecutor` owning the ladder, swim/
   water-exit, and ride (approach/wait/board/carry/dismount) machines, unit-tested.
2. `MainBrain`, `Q3Brain`, and `RunTesterBrain` all delegate to it; per-brain duplicates
   deleted. Recovery/anti-orbit suspension handled inside the executor contract.
3. Live proof: `--brain q3` reaches the q2dm1 railgun (swim) and the q2dm3 railgun
   (train + lift); `--brain main` climbs a ladder route it previously failed.
4. `context/brain_notes.md` appended (brain-notes discipline).

**Estimated effort**: Medium–Large (1–2 days).

## Context

### Current state (surveyed 2026-07-09)
| Capability | runtester | main | q3 |
|---|---|---|---|
| Swim + water-exit (`water.rs`) | ✅ | ✅ (`main.rs:546-573`) | ❌ |
| Ride train/lift (`ride.rs` phases) | ✅ stateful (`runtester.rs:250-399`) | ✅ generic (`main.rs:575-620`) | ❌ |
| Ladder up/down (`RideInfo.ladder`) | ✅ (`runtester.rs:271-325`: face exit, `up=±1`, hop near top, jump-into-shaft descent) | ❌ | ❌ |

The hard-won execution knowledge (Plans 40/43/35: zero-input carry on trains, jump on
board/dismount, face-the-exit ladder top-out, `EXIT_LOOKUP_PITCH` water-jump, live top-center
tracking, `active_ride` edge locking, recovery suspension) lives in per-brain copies that
have already drifted. Every future brain (Plan 44 `zb2`) would re-copy it. This is the
single biggest blocker to "bots that navigate maps like humans" in real matches.

### Design

```rust
// crates/brain/src/traverse.rs
pub struct TraversalExecutor { /* ride/ladder/swim state, active-edge lock */ }
pub struct TraversalOutcome {
    pub intent_override: Option<MoveIntent>, // full movement override while traversing
    pub suspend_recovery: bool,              // stuck-recovery + anti-orbit off
    pub flag: Option<char>,                  // recorder: 'S' swim, 'P' ride, 'L' ladder
}
impl TraversalExecutor {
    /// Call every tick BEFORE steering. Returns None when no traversal edge is active.
    pub fn tick(&mut self, pos: Vec3, view: &Worldview, cm: &CollisionModel,
                nav: &dyn Navigator, pursue: Vec3, dt: f32) -> Option<TraversalOutcome>;
}
```
- Movement is owned by the executor while active; **aim/fire stays with the brain** (a bot
  on a lift can still shoot). Brains apply `intent_override` to movement axes only.
- The executor internalizes: the swim gate (`is_swimming || current_edge_is_swim`), the ride
  phase machine + `active_ride` lock, and the ladder branch (lifted verbatim from
  `runtester.rs:271-325` — it is the only correct copy).
- Ladder flag `'L'` is new; `'P'` lands here if Plan 43 T4 hasn't already added it
  (coordinate — do not double-add).

### Behavior-preservation rule
This is a **seam extraction** (same discipline as Plan 22): lift the best existing copy of
each machine verbatim, then delete the duplicates. No tuning changes in the same commits.

## Step-by-Step Tasks

### T1: Extract `brain::traverse` from the existing copies

**Files**: `crates/brain/src/traverse.rs` (new), `crates/brain/src/lib.rs`

**What to do**: Move the ladder machine (runtester's, the only one), the stateful train/lift
machine (runtester's stateful version, which superseded main's generic block), and the swim
block (main's) into `TraversalExecutor`. Keep `ride.rs`/`water.rs` as the pure helpers they
are. Unit-test: synthetic `Worldview`s drive each machine through its phases (mirror
`ride.rs`/`water.rs` existing tests).

### T2: `RunTesterBrain` adopts the executor

**File**: `crates/brain/src/brains/runtester.rs`

**What to do**: Replace the inline ladder/ride/swim blocks with the executor; delete the
copies. Regression gate (must match pre-change results): q2dm1 `spawn-to-weapon railgun`
(swim), q2dm3 `spawn-to-weapon railgun --instance 1` (ride ≥3/4), q2dm3
`spawn-to-item quaddamage --count 1` from spawn3 (train over lava).

### T3: `MainBrain` adopts the executor (gains ladders)

**File**: `crates/brain/src/brains/main.rs`

**What to do**: Replace `main.rs:546-620` (swim + ride blocks) with one executor call;
recovery suspension comes from `suspend_recovery`. Combat aim retained during traversal
(movement-only override). Live check: `connect-one --brain main` on q2dm3 roams through
ladder + lift routes without wedging (watch for recovery `R` frames during traversal — must
be zero).

### T4: `Q3Brain` adopts the executor (gains everything)

**File**: `crates/brain/src/brains/q3/mod.rs` (+ `q3/move.rs`)

**What to do**: Call the executor at the movement stage of the q3 tick (before its
dodge/strafe texture; skip dodge while traversing). This is additive for q3 — it previously
had no traversal at all, so there is no baseline to preserve, only new capability. Live
proof: `spawn-to-weapon railgun --brain q3 --map q2dm1` reaches (swims); q2dm3
`spawn-to-weapon railgun --instance 1 --brain q3` reaches (rides).

### T5: recorder flags + notes

**Files**: `crates/brain/src/recorder.rs`, `context/brain_notes.md`, `context/pitfalls.md`

**What to do**: Emit `S`/`P`/`L` from the executor's `flag` in all brains (coordinate with
Plan 43 T4 for `P`). Append a dated `brain_notes.md` section; note any extraction gotchas in
`pitfalls.md`.

> **Rule B reminder**: commit after *each* task; fmt + clippy(-D warnings) + tests green
> before every commit. T2's regression gate runs BEFORE the T3/T4 adoption commits.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/traverse.rs` | new — the shared executor | P0 |
| `crates/brain/src/brains/runtester.rs` | adopt + delete inline copies | P0 |
| `crates/brain/src/brains/main.rs` | adopt (gains ladders) | P0 |
| `crates/brain/src/brains/q3/mod.rs` | adopt (gains swim/ride/ladder) | P0 |
| `crates/brain/src/recorder.rs` | `L` flag (+ `P` if not done by Plan 43) | P1 |

## Open Questions / Risks

1. **Extraction drift** — the three copies differ subtly (main's ride block lacks the
   stateful board/carry lock). *Mitigation*: lift the **best** copy per machine (stated in
   T1), and gate T2 on the live regression matrix before touching main/q3.
2. **q3's combat texture fighting the executor** (jump-dodge mid-ride = pit death).
   *Mitigation*: `suspend_recovery` also suppresses dodge/jump texture; T4 states it.
3. **Combat vs traversal priority** — a bot attacked mid-ladder is helpless. Accepted for
   v1 (humans are too); aim/fire stays live, movement committed.
4. **Recorder `P` double-add** if Plan 43 T4 lands first. *Mitigation*: T5 coordinates.

## Verification Checklist

- [ ] T1: executor unit tests pass (ladder/ride/swim phase walks); commit.
- [ ] T2: runtester regression matrix matches pre-change (q2dm1 swim, q2dm3 ride ×2); inline
      copies deleted; commit.
- [ ] T3: main traverses ladders live; zero `R` frames during traversal; commit.
- [ ] T4: `--brain q3` reaches q2dm1 railgun (swim) AND q2dm3 railgun (ride); commit.
- [ ] T5: `S`/`P`/`L` flags emitted; `brain_notes.md` appended; commit.
- [ ] fmt + clippy(-D warnings) + tests green before each commit.
