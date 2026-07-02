# Plan 45 — `main` brain competitiveness vs `q3`

> **Status**: in-progress
> **Created**: 2026-07-02
> **Depends on**: Plan 24 (main brain), Plan 37 (q3 brain / q3char)
> **Goal**: Make the `main` brain out-fight the `q3` brain in a 4v4 deathmatch by adding strategic disengage, resource-seeking, and dodge — **without modifying the `q3` brain or hurting its performance**.
> **Agent**: interactive loop

---

## TL;DR

**What**: Add human-like strategy to `MainBrain` — pick better weapons, grab health/armor, flee when out-gunned, dodge — and iterate against a live 5-minute competition until `main_fallback` beats `q3_fallback_major`.

**Deliverables**:
1. Underpowered-disengage: `main` retreats (toward items / away from enemy) when out-gunned or hurt, instead of feeding.
2. Weighted item strategy: `main` over-values weapon pickups when weak and health/armor when hurt (main-only; `q3` item model untouched).
3. Flee tuning + combat dodge; verified by repeated 5-min competitions.

**Estimated effort**: Medium (1 day, iterative).

---

## Context

Baseline (100 s, prior run): `q3_fallback_major` kd=4.50 vs `main_fallback` kd=0.18 — `main` is fed to precise major bots. `main` engages regardless of loadout/health (its combat driver forces `Engage` whenever a target has LOS), only flees at health<30, and never reads a weapon disadvantage. The `q3` brain wins because it **disengages when out-gunned** (aggression scalar) and picks its fights.

### Constraint

`q3` performance must not regress. Therefore: **no changes to `brains/q3/*`**, and shared modules (`items`, `weapons`, `combat`, `q3char`, `steer`) may only be *added to* in ways that leave existing `q3`-consumed behavior byte-identical. Prefer main-local logic. Reusing the **read-only** `q3char::bot_aggression` (pure fn) from `main` is allowed — it does not mutate q3.

### Test loop

`timeout 300 cargo run --release --bin qbots -- competition --count 4 --brains q3,main --chars major --navmodes hybrid-fallback`
Read the FINAL scoreboard `kd=` for `main_fallback` vs `q3_fallback_major`. Stop when `main` consistently wins.

---

## Step-by-Step Tasks

### T1: Underpowered disengage in `main`
**File**: `crates/brain/src/brains/main.rs`
Compute `aggression = q3char::bot_aggression(view, enemy_height_delta)`; when `< RETREAT_THRESHOLD` and a target exists, do not force `Engage` — set `Flee`, pick a retreat goal (best item, else away from enemy), bias movement away while still firing back.

### T2: Weighted item strategy for `main`
**File**: `crates/brain/src/items.rs` (new fn) + `main.rs`
Add `best_item_goal_weighted(view, skill, held_weapon, health, armor)` used only by `main`; weapons weighted up when holding blaster/MG, health/armor up when hurt. `best_item_goal` (q3) unchanged.

### T3: Flee tuning + dodge
Raise flee threshold, make Flee persist & seek health, add small strafe jitter in engagements. Tune via the competition loop.

---

## Verification Checklist
- [ ] `cargo fmt`, `cargo clippy -D warnings`, `cargo test` green.
- [ ] `q3` brain source unchanged (git diff shows no `brains/q3/` edits).
- [ ] 5-min competition: `main_fallback` kd ≥ `q3_fallback_major` kd across ≥2 runs.
