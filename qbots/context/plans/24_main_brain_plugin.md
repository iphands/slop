# Plan 24 — `main` Brain Plugin (relocate + prove the seam)

> **Status**: pending
> **Created**: 2026-06-18
> **Depends on**: Plan 23 (brain plugin core — `trait Brain` + `BrainKind` + `build_brain`)
> **Goal**: Relocate the concrete decision body into a first-class `main` plugin
> (`brain::brains::main::MainBrain`), add a minimal **second** reference brain to prove the seam
> runs with more than one implementation, and migrate `scenario.rs` onto a brain via the trait
> (closing Plan 22's deferred T4) — all behavior-preserving for `main`.
> **Agent**: sub-agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Move the concrete `Brain` (from `brain.rs`) into `brains/main.rs` as `MainBrain`
implementing `trait Brain` — the "normal" brain. Add a tiny second brain
(`brains/sentry.rs` → `SentryBrain`: stand still, face + fire at any LOS enemy, no nav) purely
to **prove the plugin seam compiles and runs with >1 impl** and to give the movement scenarios a
combat-free option later. Wire both into `BrainKind` + `build_brain`. Migrate `scenario.rs`
off its duplicated decision/steering logic onto a `Box<dyn Brain>` built with `BrainConfig {
combat_enabled: false, goal_override: Some(pinned) }`, retiring the Plan 15 duplication
(Plan 22 T4). `main` behavior stays byte-identical.

**Deliverables**:
1. `brain::brains::main::MainBrain` — the relocated concrete brain (verbatim logic), `BrainKind::Main`.
2. `brain::brains::sentry::SentryBrain` — a ~40-line reference brain, `BrainKind::Sentry`.
3. `build_brain` dispatches both; `BrainKind` is the single source of brain identity.
4. `scenario.rs` runs on a `Box<dyn Brain>` (combat-off, pinned goal) — Plan 15 duplication deleted.
5. Plan 24 outcome appended to `context/brain_notes.md`.

**Estimated effort**: Medium (1 day)

---

## Context

After Plan 23 the trait exists but there is still exactly one implementation, and it lives in
the old `brain.rs` rather than under the `brains/` plugin tree. This plan makes the structure
*say what it means*: `main` is one plugin among potentially many, sitting beside its peers.

A second brain is not busywork — it is the **proof** that `core` is actually a usable seam (a
single-impl trait is easy to get subtly wrong, e.g. a method that secretly assumes main's FSM).
`SentryBrain` is intentionally trivial and shares no state with `MainBrain`; if it satisfies
`trait Brain` and runs, the contract is real. It also doubles as the simplest possible
"combat-only, nav-off" reference for future behavior experiments.

### Why migrate `scenario.rs` now

The movement scenarios (`spawn-to-spawn`, `spawn-to-weapon`) still carry a **copy** of the
decision/steering logic (Plan 15 "nav parity"). Plan 22 deferred its migration (T4) to protect
that plan's clean diff. With the `BrainConfig { combat_enabled, goal_override }` knobs now
behind the trait, the scenarios can drive a `MainBrain` directly — deleting the duplication and
guaranteeing scenario behavior tracks the live bot forever after. This is the right plan for it
because both pieces (relocation + the config knobs) are in hand here.

### Scope guard

Still **behavior-preserving for `main`**: the `MainBrain` decision body is moved verbatim, not
edited. The scenario migration must reproduce the current `reached`/`elapsed` numbers. No new
tactics, persona, or nav changes — those start at Plan 26.

---

## Step-by-Step Tasks

### T1: Relocate the concrete brain into `brains/main.rs` as `MainBrain`

**File**: `crates/brain/src/brains/main.rs` (new, moved from `brain.rs`), `crates/brain/src/brain.rs` (deleted), `crates/brain/src/lib.rs`, `crates/brain/src/brains/mod.rs`.

**What to do**: `git mv crates/brain/src/brain.rs crates/brain/src/brains/main.rs`. Rename the
struct `Brain` → `MainBrain`. Keep the `impl Brain for MainBrain` (trait) body **verbatim** —
this is a move + rename, not a rewrite. Update `lib.rs` (`pub use brains::main::MainBrain;`,
drop the old `pub mod brain;`) and `brains/mod.rs` (`pub mod main;`). Point `build_brain`'s
`BrainKind::Main` arm at `MainBrain::new`. Move the brain's unit tests with it. `cargo test` in
the brain crate stays green. Commit `task(T1): relocate concrete brain → brains/main::MainBrain (move+rename, no logic change)`.

### T2: Minimal second reference brain `SentryBrain`

**File**: `crates/brain/src/brains/sentry.rs` (new), `crates/brain/src/brains/mod.rs`.

**What to do**: Add a ~40-line brain that proves the seam:

```rust
// brains/sentry.rs — a stationary reference brain.
pub struct SentryBrain { combat: CombatDriver, skill: BotSkill }
impl Brain for SentryBrain {
    fn set_map(&mut self, _m: BrainMap) {}                 // ignores nav entirely
    fn tick(&mut self, ctx: BrainContext) -> BrainOutput {
        // evaluate combat; aim + fire at any LOS enemy; never move.
        let dec = self.combat.evaluate(ctx.view, &self.skill, (ctx.ticks as f32)*0.1, ctx.cm);
        let mut mv = MovementIntent::new();
        if dec.should_fire { mv.attack(); mv.look_at(dec.aim_yaw, dec.aim_pitch); }
        BrainOutput { intent: mv, weapon_request: dec.weapon_request.map(|r| r.0) }
    }
    fn status(&self) -> &str { "sentry" }
}
```

Add `BrainKind::Sentry` + a `build_brain` arm. Unit-test: a `SentryBrain` constructs, `status()
== "sentry"`, and a no-enemy `tick` returns zero movement. This is the proof-of-pluggability;
it does not need to be good. Commit `task(T2): add SentryBrain reference plugin (proves the seam runs with >1 brain)`.

### T3: Migrate `scenario.rs` onto a `Box<dyn Brain>`

**File**: `crates/qbots/src/scenario.rs`.

**What to do**: Replace the duplicated decision/steering block with a brain built via
`build_brain(BrainKind::Main, skill, BrainConfig { combat_enabled: false, goal_override:
Some(pinned_goal) })`, then per tick: `brain.set_map(BrainMap { … })` at load and
`brain.tick(BrainContext { view, nav, cm, dt, ticks })` in the loop, feeding `BrainOutput.intent`
into the same `MovementController` the scenario already uses. Delete the now-dead duplicated
locals (the Plan 15 copy). The recorder/SUMMARY plumbing is unchanged. Commit
`task(T3): migrate scenario.rs onto Box<dyn Brain> (combat-off, pinned goal); delete Plan 15 duplication)`.

### T4: Scenario parity verification

**File**: tracker, `context/brain_notes.md`.

**What to do**: Re-run and compare against the **pre-migration** scenario numbers:
```bash
cargo run -p qbots -- spawn-to-spawn   --map q2dm1
cargo run -p qbots -- spawn-to-weapon rocketlauncher --map q2dm1
```
`reached` / `elapsed` (±1 tick) / flag counts must match the Plan 23 T6 "After" numbers (the
scenario now goes through the same `MainBrain` the live bot uses, so any drift is a real bug in
the migration). Record before/after in the tracker. **Append a Plan 24 outcome section to
`context/brain_notes.md`** (the `main`/`sentry` split, the scenario migration, parity result,
any gotcha porting the pinned-goal/combat-off path). Commit `task(T4): verify scenario parity post-migration; note in brain_notes`.

### T5: Close Plan 24

**File**: `SERIES.md`, plan + tracker.

**What to do**: Mark every checklist item done, `git mv` plan + tracker to `completed/`, mark
`SERIES.md` Plan 24 **done** and note Plan 22 T4 is now closed by Plan 24 T3. Commit
`task(T5): close Plan 24; move to completed/`.

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/brains/main.rs` (moved) | concrete brain → `MainBrain` (verbatim) | P0 |
| `crates/brain/src/brains/sentry.rs` (new) | reference brain proving the seam | P0 |
| `crates/brain/src/brains/mod.rs` | `BrainKind::{Main,Sentry}` + `build_brain` arms | P0 |
| `crates/brain/src/lib.rs` | exports follow the move | P0 |
| `crates/qbots/src/scenario.rs` | run on `Box<dyn Brain>`; delete Plan 15 duplication | P0 |
| `context/brain_notes.md` | append Plan 24 outcome | P0 |
| `SERIES.md`, tracker | series update + progress | P1 |

**Reused (do not reimplement):** `CombatDriver`, `MovementController`, the recorder, the
`Navigator` trait, `build_navigator`. **Untouched:** nav layer, the driver, `main.rs` bot_task
(already on `Box<dyn Brain>` from Plan 23).

---

## Open Questions / Risks

1. **Scenario behavior drift** — biggest risk; the pinned-goal/combat-off path must reproduce
   Plan 15's numbers. *Mitigation*: the config knobs already existed in Plan 22's `BrainConfig`;
   the migration is wiring, not logic. Compare SUMMARY lines tick-for-tick (T4).
2. **`git mv brain.rs`** churns imports project-wide. *Mitigation*: it's mechanical; `cargo
   build` surfaces every stale path; fix in the T1 commit.
3. **SentryBrain dead-code warnings** if a field is unused. *Mitigation*: it does use `combat` +
   `skill`; keep it minimal so clippy stays clean (Rule A).
4. **`BrainKind::Sentry` exposed to users prematurely** — Plan 25 adds the `--brain` flag; until
   then `Sentry` is only reachable in code/tests. That's fine (it's a proof, not a feature yet).

---

## Verification Checklist

- [ ] T1: `brains/main::MainBrain` is the relocated concrete brain; logic unchanged; brain tests green; clippy clean.
- [ ] T2: `SentryBrain` implements `trait Brain`; `BrainKind::Sentry` builds via `build_brain`; unit test green.
- [ ] T3: `scenario.rs` runs on `Box<dyn Brain>`; Plan 15 duplicated locals deleted; builds clean.
- [ ] T4: `spawn-to-spawn`/`spawn-to-weapon` SUMMARY lines match Plan 23 T6 "After" (±1 tick); recorded in tracker.
- [ ] T4: **Plan 24 outcome appended to `context/brain_notes.md`.**
- [ ] T5: `SERIES.md` marks Plan 24 done + Plan 22 T4 closed; plan moved to `completed/`.
- [ ] `cargo build` + `cargo clippy -- -D warnings` + `cargo test` + `cargo fmt` clean throughout; committed at every task (Rule A/B/C).
