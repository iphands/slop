# Plan 24 — `main` Brain Plugin (relocate + prove the seam)

> **Status**: pending
> **Created**: 2026-06-18
> **Depends on**: Plan 23 (brain plugin core — `trait Brain` + `BrainKind` + `build_brain`)
> **Goal**: Relocate the concrete decision body into a first-class `main` plugin
> (`brain::brains::main::MainBrain`) and add a minimal **second** reference brain to prove the
> seam runs with more than one implementation — all behavior-preserving for `main`.
> **Agent**: sub-agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Move the concrete `Brain` (from `brain.rs`) into `brains/main.rs` as `MainBrain`
implementing `trait Brain` — the "normal" brain. Add a tiny second brain
(`brains/sentry.rs` → `SentryBrain`: stand still, face + fire at any LOS enemy, no nav) purely
to **prove the plugin seam compiles and runs with >1 impl**. Wire both into `BrainKind` +
`build_brain`. `main` behavior stays byte-identical.

> **Note**: the scenario (`spawn-to-spawn`/`spawn-to-weapon`) migration that Plan 22 deferred
> (T4) is **not** done here. It moves to **Plan 26**, which lifts `scenario.rs`'s tick into a
> dedicated `RunTesterBrain` (the scenario loop is meaningfully *different* from
> MainBrain-combat-off — `pursue_target_safe` + a richer escape — so migrating it onto MainBrain
> would regress the scenarios and Plan 26 would re-migrate). Plan 24 leaves `scenario.rs` alone.

**Deliverables**:
1. `brain::brains::main::MainBrain` — the relocated concrete brain (verbatim logic), `BrainKind::Main`.
2. `brain::brains::sentry::SentryBrain` — a ~40-line reference brain, `BrainKind::Sentry`.
3. `build_brain` dispatches both; `BrainKind` is the single source of brain identity.
4. Plan 24 outcome appended to `context/brain_notes.md`.

**Estimated effort**: Small–Medium (half day)

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

### Scope guard

**Behavior-preserving for `main`**: the `MainBrain` decision body is moved verbatim, not edited.
`MainBrain` keeps its `BrainConfig.combat_enabled` (and `goal_override`, until Plan 26 T1 moves
it to `BrainContext`). No scenario changes, no new tactics/persona/nav — those start at Plan 26+.

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

### T3: `main` unchanged verification

**File**: tracker, `context/brain_notes.md`.

**What to do**: Confirm the relocation didn't change `main`. Run:
```bash
cargo run -p qbots -- connect-one
cargo run -p qbots -- spawn-to-spawn   --map q2dm1   # scenario UNTOUCHED — no-collateral check
cargo run -p qbots -- spawn-to-weapon rocketlauncher --map q2dm1
```
`connect-one` runs live (no kick); the scenarios (still on their own untouched path) reproduce
the Plan 23 T6 numbers — they verify nothing in the relocation leaked into the binary. Record in
the tracker. **Append a Plan 24 outcome section to `context/brain_notes.md`** (the `main`/`sentry`
split, why the scenario migration moved to Plan 26). Commit `task(T3): verify main unchanged; note in brain_notes`.

### T4: Close Plan 24

**File**: `SERIES.md`, plan + tracker.

**What to do**: Mark every checklist item done, `git mv` plan + tracker to `completed/`, mark
`SERIES.md` Plan 24 **done** (note Plan 22 T4 remains open → closed by Plan 26). Commit
`task(T4): close Plan 24; move to completed/`.

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/brains/main.rs` (moved) | concrete brain → `MainBrain` (verbatim) | P0 |
| `crates/brain/src/brains/sentry.rs` (new) | reference brain proving the seam | P0 |
| `crates/brain/src/brains/mod.rs` | `BrainKind::{Main,Sentry}` + `build_brain` arms | P0 |
| `crates/brain/src/lib.rs` | exports follow the move | P0 |
| `context/brain_notes.md` | append Plan 24 outcome | P0 |
| `SERIES.md`, tracker | series update + progress | P1 |

**Reused (do not reimplement):** `CombatDriver`, `MovementController`, the recorder, the
`Navigator` trait, `build_navigator`. **Untouched:** nav layer, the driver, `crates/qbots/src/scenario.rs`
(Plan 26 owns it), `main.rs` bot_task (already on `Box<dyn Brain>` from Plan 23).

---

## Open Questions / Risks

1. **`git mv brain.rs`** churns imports project-wide. *Mitigation*: it's mechanical; `cargo
   build` surfaces every stale path; fix in the T1 commit.
2. **SentryBrain dead-code warnings** if a field is unused. *Mitigation*: it does use `combat` +
   `skill`; keep it minimal so clippy stays clean (Rule A).
3. **`BrainKind::Sentry` exposed to users prematurely** — Plan 25 adds the `--brain` flag; until
   then `Sentry` is only reachable in code/tests. That's fine (it's a proof, not a feature yet).
4. **Plan 22 T4 stays open after this plan** — by design; Plan 26 closes it via `RunTesterBrain`.
   *Mitigation*: SERIES note + this plan's TL;DR call it out so it isn't lost.

---

## Verification Checklist

- [ ] T1: `brains/main::MainBrain` is the relocated concrete brain; logic unchanged; brain tests green; clippy clean.
- [ ] T2: `SentryBrain` implements `trait Brain`; `BrainKind::Sentry` builds via `build_brain`; unit test green.
- [ ] T3: `connect-one` runs live (no kick); scenarios (untouched) match Plan 23 T6 numbers; recorded in tracker.
- [ ] T3: **Plan 24 outcome appended to `context/brain_notes.md`.**
- [ ] T4: `SERIES.md` marks Plan 24 done (Plan 22 T4 → Plan 26); plan moved to `completed/`.
- [ ] `cargo build` + `cargo clippy -- -D warnings` + `cargo test` + `cargo fmt` clean throughout; committed at every task (Rule A/B/C).
