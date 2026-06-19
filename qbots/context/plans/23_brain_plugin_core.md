# Plan 23 — Brain Plugin Core (the `trait Brain` seam)

> **Status**: pending
> **Created**: 2026-06-18
> **Depends on**: Plan 22 (brain seam extraction — the concrete `Brain` already isolates all decisions behind `tick`)
> **Goal**: Turn the single concrete `brain::Brain` into a **plugin contract** — a `trait Brain` + shared I/O types + a `BrainKind` enum/factory mirroring `NavMode`/`build_navigator` — so multiple brain implementations can be selected at runtime, with **zero behavior change** for the existing bot.
> **Agent**: sub-agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Introduce a `trait Brain` (the "core") in `crates/brain/src/brains/core.rs`, move
the shared decision I/O types (`BrainContext`, `BrainOutput`, `BrainConfig`, `BrainMap`) next
to it, add a `BrainKind` enum + `build_brain()` factory (exact mirror of `NavMode` /
`build_navigator`), and make the **existing** concrete brain implement the trait so the fleet
binary drives it as a `Box<dyn Brain>`. Establish `context/brain_notes.md` and bake the
"append on every brain plan" rule into this and all downstream brain plans.

**Deliverables**:
1. `brain::brains::core` module: `trait Brain`, `BrainContext<'a>`, `BrainOutput`, `BrainConfig`, `BrainMap`.
2. `BrainKind` enum (`Main`, …) + `build_brain(kind, skill, cfg) -> Box<dyn Brain + Send>` factory.
3. The existing concrete brain implements `trait Brain` (renamed in place; behavior byte-identical).
4. `bot_task` drives `Box<dyn Brain + Send>` instead of the concrete struct — **no behavior change**.
5. `context/brain_notes.md` created (running-log format, mirrors `map_errors.notes.log.md`) + the append-rule baked into every brain plan.

**Estimated effort**: Medium (1 day)

---

## Context

Plan 22 collapsed every per-tick *decision* into one concrete `brain::Brain::tick`. That made
decision-making a **unit**, but only **one** unit — the bot is hardwired to a single brain.
The user wants behavior/persona work to live behind a **plugin seam**: a small `core` contract
that many brains implement, with `main` as the first ("normal") plugin, selectable per-bot the
same way nav backends are selectable per-bot via `--mode`.

The nav layer already demonstrates exactly the pattern we want:
`crates/brain/src/nav_mode.rs` defines `trait Navigator`; `crates/qbots/src/main.rs` defines
`enum NavMode` + `fn build_navigator(...) -> Box<dyn Navigator + Send>`; one flag (`--mode`)
selects a backend; the tick loop only ever touches the trait. **We mirror this for brains.**

### Why a trait + enum-factory (not dynamic registration)

The codebase's whole nav-backend selection is a `ValueEnum` + a single `match` factory. It is
the idiomatic, already-proven shape here; a dynamic plugin registry (string keys, runtime
`register`) buys nothing the enum doesn't and loses compile-time exhaustiveness and clap's
`ValueEnum` integration. "Plugin system" here means *a stable core trait many impls satisfy,
chosen at startup* — not hot-loading. Keep it an enum.

### Why a `BrainContext` struct (not loose args)

`Brain::tick` currently takes `(view, nav, cm, dt, ticks)`. The downstream behavior plans
(26–33) will need to feed the brain more per-tick facts the server already sends us —
obituary/observed-damage events, water level + air/breath from the playerstate, observed enemy
weapon. Bundling the per-tick inputs into one `BrainContext<'a>` lets us add fields **without
re-churning every brain's signature** each behavior plan. This is the one forward-looking
shape change worth making now while there is a single implementor.

### Core must be able to support (forward requirements — do NOT implement here)

The trait/`BrainContext` shape must not foreclose these downstream behaviors (they motivate the
contract; each is its own plan):

- **Runtester scenario brain** (Plan 26): per-tick `goal_override` in `BrainContext` (lazy goal).
- **Persona** (Plan 27): per-bot aggression / weapon-pref / follow / reaction / risk.
- **Weapon-matchup reads** (Plan 28): back-up-vs-SSG, don't-engage-blaster-vs-railgun,
  per-weapon ideal distance — needs observed/inferred enemy weapon in `BrainContext`.
- **Engagement** (Plan 29): chase-or-not, break a 1v1 when third-partied — needs damage/sound events.
- **Resource** (Plan 30): nearest-health-when-hurt, ammo need.
- **Elevator/plat** (Plan 31): decide→wait→ride; remove `ELEVATOR_PENALTY`; nav exposes plat facts.
- **Underwater/breath** (Plan 32): dive, monitor air, surface — needs water level + air from playerstate.
- **Heatmap preference pull-up** (Plan 33): nav exposes per-node danger; brain owns the preference.

These are explicitly **out of scope** for Plan 23 — it is a behavior-preserving refactor that
only *creates room* for them. Do not add new decision logic in this plan.

### `brain_notes.md` discipline (applies to ALL brain plans 23–33)

Per the user's standing instruction, **every brain plan appends to `context/brain_notes.md`** —
a running log in the same shape as `context/map_errors.notes.log.md` (dated sections, observed
behavior, hypotheses, what was tried, outcome). This is baked into each plan's task list and
verification checklist as a non-optional step. Plan 23 T1 creates the file.

---

## Step-by-Step Tasks

### T1: Create `context/brain_notes.md` + bake the append-rule

**File**: `context/brain_notes.md` (new)

**What to do**: Create the running-log file with a header mirroring `map_errors.notes.log.md`:

```markdown
# Brain development notes (running log)
# Started: 2026-06-18
#
# Append a dated section on EVERY brain plan (23–33) and any ad-hoc brain change.
# Format mirrors context/map_errors.notes.log.md: observed behavior, hypotheses,
# what was tried, outcome. Newest at the bottom. Keep entries dense, no fluff.

## 2026-06-18 — Plan 23: brain plugin core (trait Brain)
- Goal: introduce trait Brain + BrainKind factory; existing brain implements it; zero behavior change.
- (fill in as the plan executes: what the seam looks like, any surprises, verification result)
```

Then confirm the append-rule is present in the verification checklist of this and every
downstream brain plan (it is — see each plan's checklist). Commit `task(T1): seed brain_notes.md running log + bake append-rule`.

### T2: `brains::core` module — trait + shared I/O types

**File**: `crates/brain/src/brains/mod.rs` (new), `crates/brain/src/brains/core.rs` (new), `crates/brain/src/lib.rs`.

**What to do**: Create the `brains` module tree and define the core contract. Move
`BrainConfig`/`BrainOutput` out of `brain.rs` into `core.rs` (re-export for compatibility),
and define the new bundled input + map types and the trait:

```rust
// crates/brain/src/brains/core.rs
use std::sync::Arc;
use world::{CollisionModel, NavGraph};
use crate::nav::NavGoal;
use crate::nav_mode::Navigator;
use crate::perception::Worldview;
use crate::move_ctrl::MovementIntent;
use crate::weapons::Weapon;

/// Per-tick inputs handed to a brain. Bundled so downstream behavior plans (26–33) can add
/// fields (observed enemy weapon, damage/sound events, water/air) without changing every
/// brain's `tick` signature.
pub struct BrainContext<'a> {
    pub view: &'a Worldview,
    pub nav: Option<&'a mut dyn Navigator>,
    pub cm: Option<&'a CollisionModel>,
    pub dt: f32,
    pub ticks: u32,
}

/// Per-map facts a brain learns at map load (was `Brain::set_map` args).
pub struct BrainMap {
    pub roam_nodes: Vec<usize>,
    pub nav_graph: Arc<NavGraph>,
    /// `true` for backends (navmesh) that path to world positions, not bare node indices.
    pub roam_as_position: bool,
}

#[derive(Debug, Clone)]
pub struct BrainConfig { pub combat_enabled: bool, pub goal_override: Option<NavGoal> }
impl Default for BrainConfig { /* combat on, no override (moved verbatim) */ }

#[derive(Debug, Clone, Copy)]
pub struct BrainOutput { pub intent: MovementIntent, pub weapon_request: Option<Weapon> }

/// The plugin contract every brain implements. `Send` so a bot task can own a `Box<dyn Brain>`.
pub trait Brain: Send {
    /// Supply per-map facts once the map has loaded.
    fn set_map(&mut self, map: BrainMap);
    /// Decide one frame.
    fn tick(&mut self, ctx: BrainContext) -> BrainOutput;
    /// React to scoring a frag.
    fn on_kill(&mut self) {}
    /// React to dying (reset held-weapon tracking, etc).
    fn on_death(&mut self) {}
    /// Danger/popularity heatmap cost weights for the nav overlay feed.
    fn heatmap_weights(&self) -> (f32, f32) { (0.0, 0.0) }
    /// Short status label for periodic logging (replaces `behavior()` →`&BehaviorState`,
    /// which is main-specific; core stays decoupled from any one brain's FSM).
    fn status(&self) -> &str { "?" }
}
```

Note the deliberate decoupling: core does **not** reference `BehaviorState` (main-specific).
`set_map` takes a `BrainMap` value; `tick` takes `BrainContext`. Default method bodies let a
trivial brain (Plan 24) skip hooks it doesn't need. Wire the module into `lib.rs`
(`pub mod brains;` + re-export `Brain`, `BrainKind`, `build_brain`, and the shared types).
Unit-test: `BrainConfig::default()` is combat-on/no-override (moved assertion). Commit
`task(T2): brains::core — trait Brain + BrainContext/BrainOutput/BrainConfig/BrainMap`.

### T3: Make the existing concrete brain implement `trait Brain`

**File**: `crates/brain/src/brain.rs`.

**What to do**: Adapt the existing concrete `Brain` to the trait **without changing any
decision logic**:

- Change `set_map(&mut self, roam_nodes, nav_graph, roam_as_position)` and
  `tick(&mut self, view, nav, cm, dt, ticks)` into the trait methods that destructure
  `BrainMap` / `BrainContext` into the same locals the body already uses — a pure adapter, the
  decision body is **untouched verbatim**.
- Replace `pub fn behavior(&self) -> &BehaviorState` with `fn status(&self) -> &str` returning
  a static label derived from `self.fsm` (e.g. `match self.fsm { Roam => "roam", Engage{..} =>
  "engage", Hunt{..} => "hunt", … }`). This is the only caller-visible change; it is cosmetic
  (the periodic log line). Keep a private `behavior()` if any test needs the typed state.
- Keep `on_kill`/`on_death`/`heatmap_weights` as trait method impls (same bodies).
- Move `BrainConfig`/`BrainOutput` imports to the `core` re-exports.

Run the brain crate's unit tests; they must stay green (adjust only the construction call
shape, not assertions). Commit `task(T3): existing brain implements trait Brain (adapter only, no logic change)`.

### T4: `BrainKind` enum + `build_brain` factory

**File**: `crates/brain/src/brains/mod.rs`.

**What to do**: Mirror `NavMode` / `build_navigator` exactly:

```rust
// crates/brain/src/brains/mod.rs
#[derive(Copy, Clone, Debug, PartialEq, Eq)]  // (clap ValueEnum added in Plan 25)
pub enum BrainKind { Main }   // more variants land in Plan 24

/// Build the brain implementation for `kind`. Single match — the kind→impl mapping lives here.
pub fn build_brain(kind: BrainKind, skill: BotSkill, cfg: BrainConfig) -> Box<dyn Brain + Send> {
    match kind {
        BrainKind::Main => Box::new(crate::brain::Brain::new(skill, cfg)), // moves to brains/main in Plan 24
    }
}
```

(The concrete `Brain` is relocated into `brains/main.rs` and renamed in Plan 24; for Plan 23 it
stays where it is and the factory just references it.) Unit-test `build_brain(BrainKind::Main,
…)` returns a brain that constructs and reports `status()`. Commit `task(T4): BrainKind enum + build_brain factory`.

### T5: Drive `Box<dyn Brain>` from `bot_task` (zero behavior change)

**File**: `crates/qbots/src/main.rs`.

**What to do**: Replace `let mut brain = Brain::new(BotSkill::default(), BrainConfig::default());`
with `let mut brain = build_brain(BrainKind::Main, BotSkill::default(), BrainConfig::default());`.
Update the three call sites to the trait shape:
- `brain.set_map(BrainMap { roam_nodes, nav_graph, roam_as_position: matches!(mode, NavMode::Navmesh) })`.
- `brain.tick(BrainContext { view: &worldview, nav: nav_driver.as_deref_mut().map(|n| n as &mut dyn Navigator), cm, dt, ticks })`.
- Periodic log uses `brain.status()` instead of `brain.behavior()`.
`on_kill`/`on_death`/`heatmap_weights` are unchanged (trait methods). `cargo fmt` + `clippy -D
warnings` + `cargo test`. Commit `task(T5): bot_task drives Box<dyn Brain> via build_brain`.

### T6: Zero-behavior-change verification + close

**File**: tracker, `SERIES.md`, `context/brain_notes.md`.

**What to do**: Run and compare against the pre-refactor behavior:
```bash
cargo run -p qbots -- spawn-to-spawn   --map q2dm1
cargo run -p qbots -- spawn-to-weapon rocketlauncher --map q2dm1
cargo run -p qbots -- connect-one
```
`reached` / `elapsed` (±1 tick) / flag counts (`B/W/H/A/R`) / exit codes must match the
pre-refactor run (the scenarios still run their own path until Plan 24 migrates `scenario.rs`,
so they verify no-collateral-damage; `connect-one` exercises the `Box<dyn Brain>` seam live).
Record before/after SUMMARY lines in the tracker. **Append a Plan 23 outcome section to
`context/brain_notes.md`** (what the seam looks like, the `status()` cosmetic change, any
surprise, verification result). `git mv` plan + tracker to `completed/` and mark `SERIES.md`
done (Rule C). Commit `task(T6): verify zero behavior change; close Plan 23`.

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/brains/core.rs` (new) | `trait Brain`, `BrainContext`, `BrainOutput`, `BrainConfig`, `BrainMap` | P0 |
| `crates/brain/src/brains/mod.rs` (new) | `BrainKind` enum + `build_brain` factory | P0 |
| `crates/brain/src/brain.rs` | existing brain implements the trait (adapter, no logic change) | P0 |
| `crates/brain/src/lib.rs` | export `brains` module + new types | P0 |
| `crates/qbots/src/main.rs` | `bot_task` drives `Box<dyn Brain>` via `build_brain` | P0 |
| `context/brain_notes.md` (new) | running log; append-rule for all brain plans | P0 |
| `SERIES.md`, tracker | series update + progress | P1 |

**Reused (do not reimplement):** the entire concrete `Brain::tick` body — Plan 23 only wraps it
in a trait. **Untouched:** `crates/world/*`, `Navigator` trait + nav backends, `move_ctrl.rs`,
the heatmap overlay plumbing in `bot_task`, all decision logic.

---

## Open Questions / Risks

1. **`status()` replaces `behavior()`** — the periodic log changes from a `Debug`-printed
   `BehaviorState` to a short label. *Mitigation*: it is log-only/cosmetic; keep a private typed
   accessor for any test that asserts FSM state. Note the change in `brain_notes.md`.
2. **`BrainContext` lifetime/borrow of `nav`** — `nav: Option<&mut dyn Navigator>` inside a
   struct must thread the caller's mutable borrow correctly. *Mitigation*: construct
   `BrainContext` inline at the call site each tick (no storage); the borrow ends when `tick`
   returns — identical lifetime to today's by-arg form.
3. **Scope creep into behavior** — tempting to start persona/elevator here. *Mitigation*: this
   plan is behavior-preserving by contract; any SUMMARY-line drift means logic leaked in.
4. **`scenario.rs` still duplicates logic** — not migrated here. *Mitigation*: that is Plan 24
   T-migrate (it was Plan 22's deferred T4); Plan 23 leaves it alone to protect the clean diff.

---

## Verification Checklist

- [ ] T1: `context/brain_notes.md` exists with the running-log header + Plan 23 section started.
- [ ] T2: `brains::core` defines `trait Brain` + `BrainContext`/`BrainOutput`/`BrainConfig`/`BrainMap`; unit test green; clippy clean.
- [ ] T3: existing brain implements `trait Brain`; decision body unchanged; brain crate tests green.
- [ ] T4: `BrainKind::Main` + `build_brain` factory build a working brain; unit test green.
- [ ] T5: `bot_task` drives `Box<dyn Brain + Send>`; fmt/clippy/test clean.
- [ ] T6: `connect-one` runs live (no kick); `spawn-to-spawn`/`spawn-to-weapon` SUMMARY lines match pre-refactor (±1 tick); before/after recorded in tracker.
- [ ] T6: **Plan 23 outcome appended to `context/brain_notes.md`.**
- [ ] `cargo build` + `cargo clippy -- -D warnings` + `cargo test` + `cargo fmt` clean throughout; committed at every task (Rule A/B); plan moved to `completed/` (Rule C).
