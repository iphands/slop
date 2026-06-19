# Plan 22 — Brain Seam Extraction (zero behavior change)

> **Status**: done
> **Created**: 2026-06-18
> **Depends on**: Plan 06 (brain), Plan 07 (Eraser combat/skill), Plan 12 (steering), Plan 13 (stuck recovery)
> **Goal**: Extract the dissolved decision/steering logic out of `bot_task` into a single `Brain` in `crates/brain/`, behind a clean contract, with byte-identical bot behavior — establishing the seam for future behavior/persona work without touching nav or the driver.
> **Agent**: sub-agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Move every *decision* (combat, FSM, back-up/circle-strafe, dodge, goal/roam
selection, stuck recovery, jump-edge press) out of the ~300-line decision/steering body of
`bot_task` (`main.rs:818–1111`) into one `Brain` that owns the sub-drivers and takes the
`Navigator` as an injected dependency. `bot_task` becomes thin orchestration. **No behavior
change** — verified by the Plan 10 movement scenarios producing identical SUMMARY lines.

**Deliverables**:
1. `brain::Brain` + `BrainConfig` + `BrainOutput` — owns combat/fsm/danger/steering/recovery/skill/roam.
2. `Brain::tick(view, nav, cm, dt, ticks) -> BrainOutput` — the lifted decision body, verbatim.
3. `bot_task` reduced to: build `Worldview` → `brain.tick` → feed `MovementController` → transmit.
4. Before/after SUMMARY-line parity recorded in the tracker.

**Estimated effort**: Medium (1 day)

## Context

The nav layer (`world/`) and the driver (`MovementIntent → Usercmd`) are in a good state
and must be **left alone**. The blocker to all future behavior/persona work is that
decision-making **is not a unit**: it lives inline in `bot_task` (`crates/qbots/src/main.rs:818–1111`),
interleaved with steering selection, driver decomposition, and nav-servicing — FSM, combat,
the combat→FSM override, item/roam goal selection, back-up-from-enemy, circle-strafe, arrive
throttle, stuck recovery, jump-edge press, and projectile dodge, all in the binary.

`brain/` the crate already holds the sub-drivers (`fsm`, `combat`, `danger`, `skill`,
`items`, `steer`, `recover`) and `BotSkill`/`Personality` exists — but is hardcoded to
`BotSkill::default()` (`main.rs:588`) and only modulates the inline code. There is no
object that *is* the brain.

### Why "Brain owns the sub-drivers + nav injected"

It puts every decision on one side of a single function boundary while keeping nav a pure
dependency (Brain *uses* the `Navigator` trait, never modifies it). It is the minimal change
that gives a place to work on behavior/personas without touching nav (`Navigator` trait,
`brain/src/nav_mode.rs`) or the driver (`move_ctrl.rs`), and it is the natural host for the
Phase-2 pull-up of elevator + heatmap *policy* (see end of file).

### Key Facts

- The decision body depends on `nav.pursue_target()` to choose move direction *after*
  `nav.set_goal()` — so the Brain must drive nav internally within one `tick`. Pass `ticks`
  in (combat jitter + roam dwell derive from it) so timing is identical.
- The movement scenario runner (`scenario.rs`) carries its **own copy** of much of this logic
  (Plan 15 "nav parity"). The seam is designed to serve both call sites; the actual
  `scenario.rs` migration is **T5 (optional / separable)** to protect the T4 clean diff.
- `heatmap_obs`, `FleetStats`, and `conn` I/O **stay in `bot_task`** this plan. The Brain
  exposes only `heatmap_weights()` so the overlay feed stays byte-identical. The heatmap
  *policy* pull-up is Phase 2.

## Step-by-Step Tasks

### T1: `Brain` module — struct + lifted `tick` body
**File**: `crates/brain/src/brain.rs` (new), `crates/brain/src/lib.rs`.
Define `Brain`, `BrainConfig { combat_enabled: bool, goal_override: Option<NavGoal> }`,
`BrainOutput { intent: MovementIntent, weapon_request: Option<Weapon> }`, `Brain::new(skill,
cfg)` + `set_map(roam_nodes, nav_graph, roam_as_position)`, hooks
`on_kill`/`on_death`/`heatmap_weights`/`behavior`, and `tick(&mut self, view, nav: Option<&mut
dyn Navigator>, cm, dt, ticks) -> BrainOutput`. Lift `main.rs` decision/steering body
**verbatim** into `tick`, swapping locals for `self.*` + the injected `nav`. Guard combat
behind `cfg.combat_enabled` and goal selection behind `cfg.goal_override` so the **default
config reproduces today's behavior exactly**. (Skeleton + body land together — splitting them
warns on unused fields.) Export from `lib.rs`. Unit-test construction + default config.
Commit `task(T1)`.

### T2: Thin out `bot_task` onto `Brain`
**File**: `crates/qbots/src/main.rs`.
Construct `Brain` early; `brain.set_map(...)` at map load (`roam_as_position = mode ==
Navmesh`). Replace the moved body with: build `Worldview`, compute `dt`, call `brain.tick(...,
nav_driver.as_deref_mut().map(|n| n as &mut dyn Navigator), ...)`, queue `use <weapon>` from
`BrainOutput.weapon_request`, `move_ctrl.build_cmd(out.intent)`, transmit. Keep `heatmap_obs`
feeding the overlay via `brain.heatmap_weights()`; route death/frag detection through
`brain.on_death()` / `brain.on_kill()`; periodic log via `brain.behavior()`. Delete the
now-dead locals (`fsm`/`combat`/`danger`/`skill`/`steering`/`recovery`/`roam_*`/`nav_graph`).
Commit `task(T2)`.

### T3: Zero-behavior-change verification + close
**File**: tracker, `SERIES.md`.
Run before vs after and diff:
```bash
cargo run -p qbots -- spawn-to-spawn   --map q2dm1
cargo run -p qbots -- spawn-to-weapon rocketlauncher --map q2dm1
cargo run -p qbots -- connect-one
```
`reached` / `elapsed` (±1 tick) / flag counts (`B/W/H/A/R`) / exit codes must match the
pre-refactor run. `cargo fmt` + `clippy -D warnings` + `cargo test` green. Record before/after
SUMMARY lines in the tracker. Commit `task(T3)`. Then `git mv` plan + tracker to `completed/`
and mark `SERIES.md` done (Rule C).

### T4 (optional, separable): Migrate `scenario.rs` onto `Brain`
**File**: `crates/qbots/src/scenario.rs`.
Replace the duplicated decision/steering logic with a `Brain` built from `BrainConfig {
combat_enabled: false, goal_override: Some(pinned) }`, retiring the Plan 15 parity
duplication. Re-run the scenarios; identical `reached`/`elapsed`. If scope runs long, defer
to a fast-follow plan rather than risk T3's clean diff. Commit `task(T4)`.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/brain.rs` (new) | `Brain`, `BrainConfig`, `BrainOutput`, `tick`, hooks | P0 |
| `crates/brain/src/lib.rs` | export the new module | P0 |
| `crates/qbots/src/main.rs` | lift `818–1111` into `Brain::tick`; thin `bot_task`; route hooks | P0 |
| `crates/qbots/src/scenario.rs` | migrate onto `Brain` (T5, optional) | P1 |
| `SERIES.md`, tracker | series update + progress | P1 |

**Reused (do not reimplement):** `CombatDriver::evaluate`, `BehaviorState::tick`,
`DangerDriver::evaluate`, `Steering`, `StuckDetector`/`RecoveryAction`, `BotSkill`/`Personality`,
`brain::items::best_item_goal`, `brain::los::has_los_player`, the `Navigator` trait,
`MovementController::build_cmd`.

**Untouched:** `crates/world/*` (nav), `Navigator` trait, `move_ctrl.rs`, the elevator
penalty, the heatmap overlay (all Phase 2).

## Open Questions / Risks

1. **Scenario duplication (T4)** — biggest behavior-diff risk (combat-off, pinned-goal path).
   *Mitigation*: land T1–T3 (bot_task only, byte-identical) first; keep T4 separable/deferrable.
2. **`ticks`/jitter coupling** — combat jitter + roam dwell derive from the loop's `ticks`.
   *Mitigation*: pass `ticks` into `tick`, don't re-derive inside Brain.
3. **Heatmap stays in main this plan** — *Mitigation*: expose only `heatmap_weights()`; don't
   move `heatmap_obs`; resist scope creep.
4. **"Zero behavior change" is the contract** — any SUMMARY drift means the lift wasn't
   verbatim. *Mitigation*: T1's lift is mechanical; review the diff for logic edits before T3.

## Verification Checklist

- [x] T1: `Brain` constructs; `tick` holds the lifted body; unit tests green; clippy clean.
- [x] T2: `bot_task` is thin orchestration; dead locals removed; clippy clean; tests green.
- [x] T3: seam validated live via `connect-one` (full combat+nav+FSM pipeline through
      `brain.tick`, no kick); `scenario.rs` (separate path) unaffected — `spawn-to-spawn`
      still reaches; fmt/clippy/test all green. (Scenarios run through `scenario.rs`, not the
      refactored `bot_task`, so they validate no-collateral-damage, not the seam itself.)
- [~] T4 (deferred → Plan 23): migrate `scenario.rs` onto `Brain`; retire Plan 15 duplication.

---

## Phase 2 (separate plan — Plan 23, pending)

Once the seam exists, behavior/persona work lands on the Brain side, nav untouched except to
**expose facts**:
- **Personas / different brains**: expand `BotSkill`/`Personality` (or a `trait Brain`) —
  aggression, weapon preference, "follow or not", reaction; wire per-bot config (the
  competition runner already threads per-bot identity).
- **Elevator behavior**: Brain decides to take the lift, then waits-clear / steps-on / rides
  via movement intents; **remove `ELEVATOR_PENALTY`** (`world/build.rs`); nav exposes the plat
  top/bottom fact only.
- **Heatmap preference → Brain**: nav exposes per-node danger; Brain owns the danger/crowd
  *preference* (persona-weighted) instead of A* pricing it.
- **Tactical reads**: "back up because he has the SSG" becomes an explicit persona-tuned
  decision rather than the fixed `BACKUP_DIST` inline rule.
