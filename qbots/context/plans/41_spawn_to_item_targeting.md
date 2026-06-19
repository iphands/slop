# Plan 41 — `spawn-to-item` subcommand + item/weapon target resolution

> **Status**: pending
> **Created**: 2026-06-19
> **Depends on**: Plan 10 (scenario harness), Plan 26 (runtester brain)
> **Goal**: Add a working `spawn-to-item <item>` movement scenario (with item-name aliases like `quaddamage` → `item_quad`) and disambiguate multi-instance targets so `spawn-to-weapon railgun` can reach the *tricky* q2dm3 railgun.
> **Agent**: TBD

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: There is no `spawn-to-item` subcommand, and `spawn-to-weapon`/item resolution
silently picks the *first* matching entity when a map has several (q2dm3 has **two**
`weapon_railgun`). Add `spawn-to-item`, an alias table for the friendly item names the user
will type, and `--instance N` disambiguation that logs every candidate.

**Deliverables**:
1. `qbots spawn-to-item <item>` mirroring `spawn-to-weapon`'s flags
   (`--count/--max-secs/--navmode/--brain/--spacing/--map/--addr/--name/--lift-penalty`).
2. `ScenarioGoal::Item(String)` resolved against `item_<name>` with an alias map
   (`quaddamage`/`quad` → `item_quad`, `invuln`/`invulnerability` → `item_invulnerability`,
   `mega`/`megahealth` → `item_health_mega`, …).
3. Multi-instance handling: `--instance N` (0-based) on `spawn-to-item`/`spawn-to-weapon`;
   default 0; on resolve, log **all** candidate origins so the operator can pick.
4. Helpful error: unknown item lists the map's available `item_*`/`weapon_*` classnames.

**Estimated effort**: Small (half day).

## Context

### Ground truth (q2dm3 entity lump, dumped 2026-06-19 from `vendor/baseq2/pak1.pak`)

- **Quad** classname is `item_quad` at `(192, 320, 216)` — *not* `item_quaddamage`. The user
  types "quaddamage", so an alias table is required.
- **Railgun** has **two** `weapon_railgun`: `(-368, -64, 352)` (upper ledge, near
  `item_invulnerability` `(-224,-32,352)`) and `(768, 816, 208)` (the loop-train/elevator one
  the user describes). `find_class(...).first()` returns `(-368,-64,352)` — the *wrong* one
  for the user's "ride the moving platforms then elevator" route. We need to target either.

### Current code

- `crates/qbots/src/scenario.rs:43` — `enum ScenarioGoal { FarthestSpawn, Weapon(String) }`.
- `crates/qbots/src/scenario.rs:503` — `resolve_goal` builds `weapon_<name>` and calls
  `bsp.find_class(&cls).first().and_then(|e| e.origin())`.
- `crates/qbots/src/main.rs:280` — `Cmd::SpawnToWeapon { … }`; dispatch at `main.rs:2150`.
- `crates/world/src/bsp.rs` — `find_class`, `BspEntity::origin()`, `entities`.

### Why this plan first

The q2dm3 nav (Plan 42) and ride behavior (Plan 43) can only be *validated* with the exact
commands the user gave (`spawn-to-item quaddamage`, `spawn-to-weapon railgun`). This plan makes
those commands exist and target the right entity, so 42/43 have a working measurement lens.
No nav-graph or brain change here — pure CLI + goal resolution.

## Step-by-Step Tasks

### T1: item alias table + `ScenarioGoal::Item`

**File**: `crates/qbots/src/scenario.rs`

**What to do**: Add `Item(String)` to `ScenarioGoal`. Add a small `fn item_classname(name)`
that lowercases input, applies an alias map, and otherwise prefixes `item_`. Aliases (extend
freely): `quad`/`quaddamage` → `item_quad`; `invuln`/`invulnerability` → `item_invulnerability`;
`mega`/`megahealth` → `item_health_mega`; `redarmor`/`bodyarmor` → `item_armor_body`;
`yellowarmor`/`combat` → `item_armor_combat`. Pass-through `item_*` if already prefixed.

### T2: multi-instance resolution + logging

**File**: `crates/qbots/src/scenario.rs` (`resolve_goal`)

**What to do**: Factor a helper `fn resolve_class_origin(bsp, cls, instance) -> io::Result<[f32;3]>`
used by both `Weapon` and `Item`. It collects **all** `bsp.find_class(cls)` origins, `tracing::info!`
logs the full list (`cls`, count, each origin + index), then returns index `instance`
(error if out of range). On no match, list available `item_*`+`weapon_*` classnames (current
behavior for weapons, extended to items).

### T3: `--instance` flag + `spawn-to-item` CLI command

**File**: `crates/qbots/src/main.rs`

**What to do**: Add `#[arg(long, default_value = "0")] instance: usize` to `SpawnToWeapon` and
the new `SpawnToItem` command. `SpawnToItem` is a copy of `SpawnToWeapon`'s clap struct with
`weapon_name` → `item_name`. Wire both dispatch arms (`main.rs:2150` region) to pass the
instance through to `run_scenario` (thread `instance` into `ScenarioGoal` or `resolve_goal`).

### T4: thread `instance` through `run_scenario` / `resolve_goal`

**Files**: `crates/qbots/src/scenario.rs`, `crates/qbots/src/main.rs`

**What to do**: Carry `instance` either inside the `ScenarioGoal::{Weapon,Item}` variant
(simplest: `Weapon { name, instance }` / `Item { name, instance }`) or as a `run_scenario`
arg. Prefer putting it in the enum so the goal is self-describing. Update the `BrainContext`
goal-override seeding path (scenario sets goal origin from `resolve_goal`).

### T5: docs + help

**Files**: `crates/qbots/src/main.rs` (doc comments), `crates/qbots/CLAUDE.md` (Movement
Testing section), `README` if it lists scenarios.

**What to do**: Document `spawn-to-item` next to `spawn-to-weapon`; note the alias examples and
`--instance` (with the q2dm3 railgun-0 vs railgun-1 note). Keep it terse.

> **Rule B reminder**: commit after *each* task (`task(T1): …`). fmt + clippy(-D warnings) +
> tests green before every commit.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/qbots/src/scenario.rs` | `Item` variant, alias table, shared instance-aware resolver | P0 |
| `crates/qbots/src/main.rs` | `SpawnToItem` cmd, `--instance`, dispatch wiring | P0 |
| `crates/qbots/CLAUDE.md` | document new scenario | P2 |

## Open Questions / Risks

1. **Item classname drift across Q2 builds** (rerelease renames). *Mitigation*: alias table
   maps friendly→canonical; the "available classnames" error lets the operator self-correct.
2. **Instance order is BSP-entity order, not stable across maps.** *Mitigation*: T2 logs all
   candidates with indices and origins, so the operator picks deterministically per map.
3. **A reached-item run still needs the area to be connected** (Plan 42). *Mitigation*: this
   plan only resolves the *goal*; reachability is Plan 42/43. On a stranded goal the run
   returns exit 2 (ran-to-cap), which is the correct signal.

## Verification Checklist

- [ ] T1: `cargo test` covers `item_classname` aliases (quaddamage→item_quad, pass-through).
- [ ] T2: unit test resolves a 2-instance class by `--instance 0/1` and errors on index 2.
- [ ] T3: `qbots spawn-to-item --help` and `spawn-to-weapon --help` show `--instance`.
- [ ] T4: `spawn-to-item quaddamage --map q2dm3` logs goal origin `(192,320,216)`.
- [ ] T4: `spawn-to-weapon railgun --instance 1 --map q2dm3` logs goal `(768,816,208)`.
- [ ] T5: docs updated; fmt + clippy(-D warnings) + tests green before each commit.
