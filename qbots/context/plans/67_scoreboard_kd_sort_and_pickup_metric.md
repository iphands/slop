# Plan 67 — Scoreboard K/D-first ranking + health/armor pickup metric

> **Status**: in-progress
> **Created**: 2026-07-13
> **Depends on**: Plan 21 (competition runner), Plan 63 (env= column pattern), Plan 65 (fleet durability — long-run scoreboards)
> **Goal**: Rank the competition scoreboard by K/D (kills as tiebreak) and record per-bot health/armor points picked up as a new scoreboard column pair.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Two scoreboard upgrades requested after the 18-group 44-min competition run of 2026-07-13: (1) the default ranking becomes K/D desc (subsort kills desc, then tag), because kills-ranked boards hide efficiency differences; (2) a new measured axis — health points and armor points picked up per group — recorded from the bot's own playerstate deltas and carried as `hp=`/`ap=` scoreboard columns.

**Deliverables**:
1. `mode_scoreboard` ranks by K/D desc → kills desc → tag asc; unit-tested.
2. `BotTally` gains `health_picked`/`armor_picked`; `FleetStats` recorders; unit-tested.
3. `bot_task` detects health/armor gains while alive (playerstate stat deltas), emits debug `EVT pickup kind=health|armor amount=N`, feeds `FleetStats`; per-map-change reset.
4. Scoreboard (live + FINAL) and `log_final_stats` show the new columns.

**Estimated effort**: Small (2 h)

## Context

### Why the sort change
The 2026-07-13 FINAL board ranked `xon_sg_shp` (kd 0.88) at #2 over `q3_sg_cam` (kd 1.06, #10) purely on kill volume. K/D is the headline metric everywhere else in the repo (acceptance aggregator, A/B baselines), so the board should lead with it. Kills stays as the subsort so equal-K/D groups still order by activity.

### How pickups are detectable at all (the "no wire signal" caveat, resolved)
`acceptance.md` deferred pickup counters as "no direct wire signal for pickups" — true for *item identity* (no reliable `svc_*` per-pickup event we parse today), but the **amount** of health/armor gained IS on the wire: our own `playerstate.stats[STAT_HEALTH=1]` / `stats[STAT_ARMOR=5]` (`brain/src/perception.rs:25,29`). A stat increase while alive is a pickup by definition in stock DM (no regen):

- Health up while alive (prev > 0, cur > prev) = health item (stimpack/small/large/mega/adrenaline). Megahealth's 1/s rot is a *decrease* — ignored.
- Armor up while alive = armor item (shard/jacket/combat/body). Damage absorption only decreases it.
- Respawn (prev ≤ 0 → 100) is excluded by the alive-before guard; armor respawn reset (X → 0) is a decrease — ignored.
- Map change re-baselines the playerstate → reset the trackers in the existing Plan 64 reset block (`main.rs:1229`).

This measures **points gained**, not item counts — which is the better competition metric anyway (a bot that grabs one mega beats a bot that grabs two stimpacks).

### Pre-identified trap: TWO health-delta blocks in `bot_task`
`main.rs` has two last_health delta readers per tick: the Plan 51 block (~line 1176, feeds `dmg_this_tick`/stall monitor, runs first, **updates `last_health` only while alive**) and the older Active-branch block (~line 1372, catches the death edge because block 1 skips dead frames). Because block 1 runs first and re-baselines `last_health`, the *heal* branch of block 2 never fires while block 1 is live — the pickup hook must go in **block 1's** heal branch, and armor tracking goes next to it.

## Step-by-Step Tasks

### T1: K/D-first scoreboard ranking

**File**: `crates/qbots/src/supervisor.rs`

**What to do**: Add `ModeScore::kd()` (deaths==0 → kills as f32, the existing display convention); sort rows `kd desc (total_cmp) → kills desc → tag asc`; the display code reuses `kd()`. Update `mode_scoreboard_groups_by_name_prefix_and_ranks_by_kills` for the new order and add a same-K/D-different-kills tiebreak case.

### T2: `BotTally` pickup fields + `FleetStats` recorders

**File**: `crates/qbots/src/stats.rs`

**What to do**: Add `health_picked: u64` and `armor_picked: u64` to `BotTally` (update the `totals()` fold); add `record_health_pickup(name, amount)` / `record_armor_pickup(name, amount)`; unit tests.

### T3: pickup detection in `bot_task`

**File**: `crates/qbots/src/main.rs`

**What to do**: In the Plan 51 delta block's heal branch, call `stats.record_health_pickup` and emit `tracing::debug!(... "EVT pickup")` with `kind=health amount=N`. Add `last_armor: Option<i32>` beside `last_health`; while alive (prev health > 0), an armor increase records `record_armor_pickup` + `EVT pickup kind=armor`. Reset `last_armor` in the Plan 64 map-change reset block.

### T4: scoreboard + final-stats columns

**File**: `crates/qbots/src/supervisor.rs`

**What to do**: `ModeScore` gains `health_picked`/`armor_picked`; `mode_scoreboard` sums them; the board line gains `hp={:<5} ap={:<4}` and the header text mentions them. `log_final_stats` totals line gains `health_picked`/`armor_picked` fields.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/qbots/src/supervisor.rs` | K/D-first sort; `hp=`/`ap=` columns | P0 |
| `crates/qbots/src/stats.rs` | `BotTally` pickup fields + recorders | P0 |
| `crates/qbots/src/main.rs` | stat-delta pickup detection + map-change reset | P0 |
| `context/acceptance.md` | note pickup counters now exist (un-defer) | P2 |

## Open Questions / Risks

1. **Adrenaline/mega inflate `hp`** — intended: the metric is "points gained", not item count. Documented in the header text choice (`hp`/`ap`, not `items`).
2. **Non-DM mods with regen would inflate `hp`** — out of scope; we only run stock DM.
3. **Block-2 heal branch is dead code while block 1 runs** — do NOT hook it (double-count risk if a future refactor revives it); T3 hooks block 1 only.
4. **Live verification needs a running server** — unit tests gate the logic; a live competition spot-check is listed in the checklist as best-effort (server availability).

## Verification Checklist

- [ ] T1: `cargo test -p qbots` green; board ranks kd 3.0 > 1.0 > 0.5 > 0.0; kills breaks a kd tie. **Commit.**
- [ ] T2: `cargo test -p qbots` green; totals fold sums the new fields. **Commit.**
- [ ] T3: `cargo build` + clippy clean; heal path records; armor tracker resets on map change (code-inspection + compile; live EVT spot-check best-effort). **Commit.**
- [ ] T4: scoreboard line renders `hp=`/`ap=`; final stats totals include them; all tests green. **Commit.**
- [ ] `cargo fmt` + `cargo clippy -- -D warnings` + full `cargo test` before every commit (Rule A).
- [ ] Plan + tracker moved to `completed/`, SERIES.md updated.
