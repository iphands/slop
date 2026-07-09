# Plan 30 — Resource decisions: map-known items, health-when-hurt, ammo awareness

> **Status**: pending (file authored 2026-07-09; SERIES row existed since Plan 23 split)
> **Created**: 2026-07-09
> **Depends on**: Plan 24 (main brain), Plan 41 (item classname aliases), Plan 18 (map cache)
> **Goal**: Bots collect resources like humans — they *know where map items live* (not just what's in PVS), route to the nearest reachable health when hurt, remember what was just taken and when it respawns, and treat low ammo as a reason to re-arm.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Today item seeking is **PVS-blind**: `items::best_item_goal[_weighted]` iterates
`view.items()` — only entities the server currently transmits (`items.rs:45-117`). A hurt
bot with no health pack on screen just keeps fighting/roaming. A human *knows* the mega is
around the corner. Give the brain the map's static item table (from the BSP entity lump we
already parse for `spawn-to-item`), a taken/respawn memory, a health-seek interrupt, and
ammo awareness.

**Deliverables**:
1. `BrainMap.items` — static item table (classname → `EntityClass` + origin) built from the
   BSP entity lump, shared via the existing map-cache/`Arc` path.
2. Item memory: seen-missing / picked-up items tracked with Q2 respawn timers, so bots
   don't run to an empty spawn pad (and *do* time big-item returns).
3. Health-seek interrupt: hurt bot routes to the nearest **reachable** (A*-distance) known
   health/armor, integrated with `main`'s FSM (Flee seeks health, not just "away").
4. Ammo awareness: `held_ammo` (STAT_AMMO) feeds weapon scoring + item values (low ammo →
   re-arm/ammo-pickup goals).

**Estimated effort**: Medium (1 day).

## Context

### What exists (surveyed 2026-07-09)
- `items::best_item_goal` (`crates/brain/src/items.rs:45`) — PVS-only, value/distance with
  a 2× low-health boost. `best_item_goal_weighted` (`items.rs:72`, Plan 45) adds weapon/
  health/armor hunger — still PVS-only, `main`-only.
- BSP item origins are already parsed and alias-resolved for scenarios:
  `bsp.find_class` (`world/src/bsp.rs:238`), `item_classname` aliases
  (`qbots/src/scenario.rs:63` — e.g. `quaddamage`→`item_quad`).
- `BrainMap` (`brain/src/brains/core.rs:44`) carries `roam_nodes` + `nav_graph` — the
  natural place for the item table.
- `SelfState.held_ammo()` (`perception.rs:90`, STAT_AMMO) — the held weapon's ammo is the
  only inventory on the wire. Health thresholds in use: `is_low_health()` <25
  (`perception.rs:349`), `FLEE_HEALTH=30`, `KITE_HEALTH=50` (`brains/main.rs`).
- Q2 respawn times (`vendor/yquake2/src/game/g_items.c`): weapons 30s, ammo 30s, health 30s,
  armor 20s, megahealth 20s-after-wear-off, powerups 60s+. Pin the constants with a comment
  citing the vendor line.

### PVS honesty
Knowing *static spawn locations* from the BSP is fair (humans learn maps; the file is on
disk). Knowing *live state* of an unseen item is not — that's why the memory model (deliv. 2)
tracks only what the bot has itself observed (saw it missing / picked it up), decaying to
"assume present" after the respawn timer. This keeps Constraint #1 (be a client) intact.

## Step-by-Step Tasks

### T1: Static item table into `BrainMap`

**Files**: `crates/world/src/build.rs` (or a small `items` module in `world`),
`crates/brain/src/brains/core.rs`, supervisor + scenario wiring in `crates/qbots/`

**What to do**: At map load, collect `item_*`, `weapon_*`, `ammo_*` entities (classname +
origin) from the BSP entity lump; move/share the `item_classname` alias helper out of
`scenario.rs` so both use one copy. Add `pub items: Vec<MapItem>` to `BrainMap`
(`MapItem { class: EntityClass, origin: Vec3, nav_node: Option<usize> }`, node resolved via
nearest-graph-node at build time). Thread through both `bot_task` and the scenario harness.

### T2: Item memory + respawn model

**File**: `crates/brain/src/items.rs` (new `ItemMemory` struct owned per-brain)

**What to do**: On each tick, for map items inside PVS: if the pad is visibly empty (no
matching entity near origin) or we just picked it up (item entity vanished as we crossed
it / stat changed), mark `taken_at = now`. `available(item, now)` = not marked, or
`now - taken_at > respawn_time(class)`. Unit-test the state transitions with synthetic
views. Keep it per-bot (no shared omniscience).

### T3: Known-item goal selection + health-seek interrupt

**Files**: `crates/brain/src/items.rs`, `crates/brain/src/brains/main.rs`

**What to do**: Extend `best_item_goal_weighted` (or add `best_map_item_goal`) to score
**map items** (filtered by `ItemMemory::available`) alongside PVS items, using **A* path
distance** (`nav_graph` from `BrainMap`) instead of euclidean when a nav node is resolved —
a health pack 200u away through a wall is not "near". Integrate with `main`:
- `Flee` picks the nearest reachable health/armor as its retreat goal (today it just moves
  away) — this is the literal "collecting health when hurt".
- Roam/Pickup prefers available known items over bare roam nodes.
Cap the A* scoring set (nearest ~8 candidates by euclidean prefilter) to bound per-tick cost.

### T4: Ammo awareness

**Files**: `crates/brain/src/weapons.rs`, `crates/brain/src/items.rs`,
`crates/brain/src/brains/main.rs`

**What to do**: `select_best_weapon` gains an `held_ammo` input: at 0 ammo the held weapon
scores 0 (forces fallback switch — today a dry RL keeps clicking); low ammo (< ~5)
penalizes. Item values: matching `ammo_*` and weapon pickups get a hunger multiplier when
`held_ammo` is low. `q3` consumes none of this by default (keep its baseline; wire behind
its existing neutral picker only if trivially additive).

### T5: Live verification + notes

**What to do**: 5-min `competition --count 4 --brains q3,main` — expect `main` deaths to
drop vs the 0.68-kd baseline (Plan 45 table) as hurt bots heal instead of re-feeding;
verify in logs: `Flee` frames ending at health pickups, no runs to empty pads twice in a
row. Append `context/brain_notes.md` (dated) with the before/after table.

> **Rule B reminder**: commit after *each* task; fmt + clippy(-D warnings) + tests green.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/items.rs` | ItemMemory, map-item scoring, A*-distance | P0 |
| `crates/brain/src/brains/core.rs` | `BrainMap.items` | P0 |
| `crates/world/src/build.rs` + `qbots` wiring | item table extraction + threading | P0 |
| `crates/brain/src/weapons.rs` | ammo-aware scoring | P1 |
| `crates/brain/src/brains/main.rs` | Flee→health, goal integration | P0 |

## Open Questions / Risks

1. **A* scoring cost per tick.** *Mitigation*: euclidean prefilter + cap 8 candidates +
   reuse the navigator's existing pathfinder; measure tick time.
2. **"Pad empty" detection is PVS-noisy** (item entity culled ≠ taken). *Mitigation*: only
   mark taken when the pad is within trusted PVS range AND LOS-visible; otherwise leave
   unknown (assume present).
3. **Health camping** (bot orbits the mega). *Mitigation*: hunger multipliers already decay
   as health recovers; add a small per-item cooldown after pickup.
4. **q3 baseline drift** — Plan 45's constraint pattern. *Mitigation*: main-first; q3 only
   via its untouched neutral picker.

## Verification Checklist

- [ ] T1: `BrainMap.items` populated on q2dm1 (spot-check counts vs `navinspect`); commit.
- [ ] T2: `ItemMemory` unit tests (take → unavailable → respawn) pass; commit.
- [ ] T3: hurt `main` bot routes to a known health pack outside PVS (live log proof); commit.
- [ ] T4: dry-weapon auto-switch unit test; low-ammo re-arm goal observed live; commit.
- [ ] T5: 5-min competition, deaths < Plan 45 baseline (25 @ 5min) or cause documented;
      `brain_notes.md` appended; commit.
- [ ] fmt + clippy(-D warnings) + tests green before each commit.
