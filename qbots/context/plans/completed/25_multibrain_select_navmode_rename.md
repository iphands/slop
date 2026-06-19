# Plan 25 — Multibrain Selection + `--navmode` Rename

> **Status**: pending
> **Created**: 2026-06-18
> **Depends on**: Plan 24 (`main`/`sentry` plugins + `BrainKind`/`build_brain`)
> **Goal**: Make the brain selectable per-bot the way the nav backend is — a `--brain <kind>`
> flag + per-bot fleet config — fully **decoupled** so any brain runs with any nav backend, and
> rename the nav flag `--mode`→`--navmode` (`--modes`→`--navmodes`) across CLI, help, and docs.
> **Agent**: sub-agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Add `BrainKind` as a clap `ValueEnum`; expose `--brain` on `connect-one`, `run`,
`spawn-to-spawn`, `spawn-to-weapon`, and `competition` (plus `--brains` comma-list for
competition); thread the chosen `BrainKind` through `bot_task`/supervisor/scenario into
`build_brain` so **brain × navmode are independent axes** (any `--brain` with any `--navmode`).
Rename the nav-backend flag `--mode`→`--navmode` and the competition `--modes`→`--navmodes`
everywhere (CLI args, help docstrings, `README.md`, `context/mode_perf.md`). The internal
`NavMode` type name stays (it is accurate); only the user-facing flag/word changes.

**Deliverables**:
1. `BrainKind` derives `clap::ValueEnum`; `--brain <kind>` (default `main`) on all bot commands.
2. Per-bot `brain` selection wired through supervisor + scenario into `build_brain`.
3. Brain and navmode are orthogonal — `--brain sentry --navmode navmesh` etc. all valid.
4. `--mode`→`--navmode`, `--modes`→`--navmodes` renamed in CLI, help, `README.md`, `mode_perf.md`.
5. `competition` can vary brain (`--brains main,sentry`) alongside `--navmodes`.
6. Plan 25 outcome appended to `context/brain_notes.md`.

**Estimated effort**: Small–Medium (half day)

---

## Context

Plans 23–24 built the brain plugin core and two implementations selectable in code via
`BrainKind`/`build_brain`. This plan exposes that choice to the operator and makes it a
first-class, **independent** axis from nav — exactly how `--mode` selects a nav backend today.

The user also asked to rename the nav flag `mode`→`navmode` so the two axes read clearly side
by side: `--brain` picks *how the bot thinks*, `--navmode` picks *how it moves through the
graph*. Both are per-bot; both default to a sensible value (`main` / `astar`).

### Rename surface (from a repo grep on 2026-06-18)

- CLI args/fields: `mode: NavMode` on `ConnectOne`/`Run`/`SpawnToSpawn`/`SpawnToWeapon`
  (`main.rs:117/128/224/259`), `modes: Option<String>` on `Competition` (`main.rs:170`), and the
  `mode`/`modes` params threaded into `scenario.rs` + `supervisor.rs`.
- Help docstrings referencing `--mode`/`--modes` (`main.rs:25,115,125,156,167`, etc).
- Docs: `README.md` (lines ~76, 80, 112, 114, 140), `context/mode_perf.md`.
- **Keep** the internal Rust type `NavMode` and `build_navigator`/`mode_tag` names — renaming
  the *flag* (`--navmode`) and the *prose* ("nav mode"), not the type, keeps the diff small and
  the type name remains accurate.

### Decoupling guarantee

`bot_task`, the supervisor's per-bot loop, and the scenario runner each currently take a
`mode: NavMode`. They will additionally take a `brain: BrainKind`, and the two are passed
independently to `build_navigator(mode, …)` and `build_brain(brain, …)`. No combination is
special-cased — that is the "any brain × any navmode" requirement, satisfied structurally.

---

## Step-by-Step Tasks

### T1: `BrainKind` as a clap `ValueEnum` + tag helper

**File**: `crates/brain/src/brains/mod.rs`.

**What to do**: Derive `clap::ValueEnum` on `BrainKind` (mirror `NavMode`'s derive), with
kebab-case value names (`main`, `sentry`). Add `pub fn brain_tag(kind: BrainKind) -> &'static
str` (mirror `mode_tag`) for logging/competition naming. Unit-test the round-trip
(`BrainKind::from_str("main")`). Commit `task(T1): BrainKind ValueEnum + brain_tag`.

### T2: `--brain` flag on the single-bot + scenario commands

**File**: `crates/qbots/src/main.rs`.

**What to do**: Add `#[arg(long, value_enum, default_value_t = BrainKind::Main)] brain:
BrainKind` to `ConnectOne`, `SpawnToSpawn`, `SpawnToWeapon` (and `Run` — see T3). Thread it into
`bot_task(..., brain: BrainKind)` and `run_scenario(..., brain: BrainKind)`; pass to
`build_brain(brain, skill, cfg)` (replacing the hardcoded `BrainKind::Main` from Plan 23 T5 /
Plan 24 T3). Keep nav independent: `build_navigator(navmode, …)` and `build_brain(brain, …)` are
separate calls. Add a help docstring: "Brain (decision plugin): `main` (default) or `sentry`.
Independent of `--navmode`." **Default stays `BrainKind::Main` for all commands here** —
`RuntesterBrain` doesn't exist until Plan 26, which **flips the `spawn-to-spawn`/`spawn-to-weapon`
default to `runtester`** (and migrates `scenario.rs` onto the trait). Commit
`task(T2): --brain flag on connect-one/spawn-to-* + scenario`.

### T3: `--brain` for the fleet (`run`) + per-bot config

**File**: `crates/qbots/src/main.rs`, `crates/qbots/src/supervisor.rs`, the fleet config struct (wherever `[fleet]` is defined).

**What to do**: Add `--brain` to `Run` (whole-fleet override, default `main`) and a
`brain: Option<String>` (parsed to `BrainKind`, default `main`) field to the fleet config so a
roster can pin a brain per bot or fleet-wide. Thread `BrainKind` through
`bot_supervisor_loop(..., mode, brain)` into `bot_task`. CLI `--brain` overrides config like
`--mode`/`--count` already do. Commit `task(T3): --brain for run fleet + per-bot fleet config`.

### T4: Competition varies brain (`--brains`)

**File**: `crates/qbots/src/main.rs`, `crates/qbots/src/supervisor.rs`.

**What to do**: Add `--brains <comma-list>` to `Competition` (default `main`), parsed the same
way `--navmodes` is (T5 renames it). Extend the competition supervisor to spawn `per_count`
bots for the **cross product** of selected `{navmode} × {brain}` (or, if simpler and clearer,
one group per `(navmode, brain)` pair from explicit lists), naming bots `<brain>-<navmode>_<i>`
and grouping the scoreboard accordingly (extend `ModeScore`/`mode_tag` grouping to include the
brain tag). Keep the default (no `--brains`) identical to today (all bots `main`). Commit
`task(T4): competition --brains; scoreboard grouped by (brain,navmode)`.

### T5: Rename `--mode`→`--navmode` / `--modes`→`--navmodes` (flag + prose only)

**File**: `crates/qbots/src/main.rs`, `crates/qbots/src/scenario.rs`, `crates/qbots/src/supervisor.rs`, `README.md`, `context/mode_perf.md`.

**What to do**: Rename the clap flag from `mode`→`navmode` and `modes`→`navmodes`. Two clean
options for the field rename without churning every internal use:
- Preferred: `#[arg(long = "navmode", value_enum, default_value_t = NavMode::Astar)] navmode:
  NavMode` and rename the local bindings, **or**
- keep the field named `mode` internally but set `#[arg(long = "navmode")]` so only the
  user-facing flag changes (smaller diff). Pick whichever keeps the code readable; document the
  choice in the commit. Update every help docstring mentioning "`--mode`"/"nav mode" to
  "`--navmode`". Update `README.md` (the `## Navigation backends` heading → `(--navmode)`, the
  example invocations, the competition line) and `context/mode_perf.md` headings/prose. **Do
  not** rename the `NavMode` type, `build_navigator`, or `mode_tag` (internal, accurate). Commit
  `task(T5): rename --mode→--navmode / --modes→--navnodes across CLI, help, README, mode_perf.md`.

### T6: Verify + close

**File**: tracker, `SERIES.md`, `context/brain_notes.md`.

**What to do**: Verify the matrix builds and runs:
```bash
cargo run -p qbots -- connect-one --brain sentry --navmode navmesh   # brain×navmode orthogonal
cargo run -p qbots -- spawn-to-spawn --brain main --navmode astar --map q2dm1
cargo run -p qbots -- competition --brains main,sentry --navmodes astar,navmesh --count 2
cargo run -p qbots -- run --help   # confirm --navmode (not --mode) + --brain in help
```
Confirm `--mode` is gone from `--help` output and `--navmode`/`--brain` are present and
independent. Record the matrix + scoreboard grouping in the tracker. **Append a Plan 25 outcome
section to `context/brain_notes.md`** (the selection axes, the rename, any clap gotcha). `git
mv` plan + tracker to `completed/`, mark `SERIES.md` done (Rule C). Commit `task(T6): verify
multibrain×navmode matrix + rename; close Plan 25`.

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/brains/mod.rs` | `BrainKind: ValueEnum` + `brain_tag` | P0 |
| `crates/qbots/src/main.rs` | `--brain` on all bot cmds; `--navmode`/`--navmodes` rename; `--brains` | P0 |
| `crates/qbots/src/supervisor.rs` | thread `BrainKind`; competition cross-product + scoreboard grouping | P0 |
| `crates/qbots/src/scenario.rs` | accept + use `BrainKind` | P1 |
| `README.md`, `context/mode_perf.md` | `--mode`→`--navmode` prose + examples | P1 |
| `context/brain_notes.md` | append Plan 25 outcome | P0 |
| `SERIES.md`, tracker | series update + progress | P1 |

**Untouched:** the `NavMode` type/`build_navigator`/`mode_tag` names, nav backends, the brain
decision logic (this plan is wiring + renaming only).

---

## Open Questions / Risks

1. **Competition cross-product blow-up** — `{navmodes} × {brains} × count` bots can exceed
   `max_bots`. *Mitigation*: reuse the existing clamp logic (`supervisor.rs:374`) on the new
   total; log the clamp.
2. **Flag rename breaking muscle memory / scripts** — `--mode` disappears. *Mitigation*: it's an
   explicit user request; do a hard rename (no hidden alias) and update README so the new flag is
   discoverable. If a deprecation alias is wanted later, that's a follow-up.
3. **Config back-compat** — adding `brain` to `[fleet]` config must default to `main` when
   absent so existing `config.yaml` files keep working. *Mitigation*: `#[serde(default)]` → `main`.
4. **Scoreboard grouping key change** — grouping by `(brain, navmode)` instead of `navmode`
   alone. *Mitigation*: keep the no-`--brains` default producing today's exact board (all `main`).

---

## Verification Checklist

- [ ] T1: `BrainKind` is a `ValueEnum`; `brain_tag` + from_str round-trip tested.
- [ ] T2: `--brain` on `connect-one`/`spawn-to-spawn`/`spawn-to-weapon`; passed to `build_brain`; nav still via `build_navigator`.
- [ ] T3: `run --brain` + per-bot `[fleet].brain` config (default `main`) threaded to `bot_task`.
- [ ] T4: `competition --brains main,sentry --navmodes astar,navmesh` spawns the matrix; scoreboard grouped by `(brain,navmode)`.
- [ ] T5: `--mode`→`--navmode`, `--modes`→`--navmodes` in CLI/help/README/mode_perf.md; `NavMode` type unchanged; `--help` shows no `--mode`.
- [ ] T6: matrix invocations build + run; brain and navmode independently selectable; recorded in tracker.
- [ ] T6: **Plan 25 outcome appended to `context/brain_notes.md`.**
- [ ] `cargo build` + `cargo clippy -- -D warnings` + `cargo test` + `cargo fmt` clean throughout; committed at every task (Rule A/B/C).
