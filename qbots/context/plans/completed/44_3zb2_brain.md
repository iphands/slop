# Plan 44 — 3ZB2-Style Brain Plugin (`zb2`)

> **Status**: pending (rewritten 2026-07-09 — grounded in the real crate layout)
> **Created**: 2026-06-19
> **Depends on**: Plan 23/24/25 (brain plugin contract + multibrain select), Plan 43 (ride behavior), Plan 46 (shared traversal executor)
> **Goal**: A pluggable `Zb2Brain` (`--brain zb2`) that plays deathmatch with 3ZB2's signature *decision texture* — sequential route memory with LOS shortcut-skipping (`Search_NearlyPod`) and 3ZB2's mover-aware route states — on top of the existing `Navigator`/nav-graph, then measure it against `q3` and `main` in competition.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Port 3ZB2's *behavior* (route following + shortcuts + mover states) as a third
full brain plugin, the way Plan 37 ported Quake 3's — reusing our nav graph, steering,
recovery, combat, and (Plan 46) traversal executor. **Do not** port 3ZB2's node-linking
(`G_FindRouteLink`) into `world/` — our graph already has Walk/Jump/Swim/Ride edges that are
strictly richer than 3ZB2's `linkpod[6]`; re-porting its linker would be a regression.

**Deliverables**:
1. `crates/brain/src/brains/zb2.rs` — `Zb2Brain`, `BrainKind::Zb2` (`--brain zb2`), wired
   through `build_brain` / CLI / fleet config / competition (mirror Plan 37's `q3` wiring).
2. Sequential route memory + `Search_NearlyPod` shortcut-skip over `Navigator` paths.
3. 3ZB2 mover route-states (`GRS_ONPLAT`/`GRS_ONTRAIN` semantics) mapped onto our
   `EdgeKind::Ride`/ladder edges via the Plan 46 shared traversal executor.
4. Competition results vs `q3` and `main` recorded in `context/mode_perf.md` +
   `context/brain_notes.md` (brain-notes discipline).

**Estimated effort**: Large (2–3 days).

## Context

### Why 3ZB2 (unchanged from the original plan)
Battle-tested for decades, an essential mover state machine, shortcut optimization, and live
C source in `vendor/3zb2-zigflag/src/bot/`. Reviewer guidance (2026-06-19): build **one**
solid additional brain, not six. Reference distillations: `context/distilled/brains/
3zb2_brain.md`, `context/distilled/pathing/3zb2.md` + `3zb2_linking.md`.

### What changed since the original draft (why this rewrite)
- `world/src/nav_generator.rs` **does not exist**; nav generation lives in
  `crates/world/src/build.rs` + `navgraph.rs`, and the graph already models jumps, water,
  trains, lifts, and ladders (Plans 39/42/35). The old T3 ("port `G_FindRouteLink`") is
  **dropped** — 3ZB2's value here is its *runtime* behavior, not its offline linker.
- The brain plugin seam is proven (4 brains: `main`, `sentry`, `runtester`, `q3` —
  `crates/brain/src/brains/mod.rs:27-79`), and Plan 37 is the canonical example of porting a
  foreign bot's brain onto the seam. Follow it file-for-file.
- Traversal execution (ladder/swim/ride) is becoming a shared module (Plan 46). `Zb2Brain`
  consumes it — 3ZB2's `GRS_*` states become a thin state veneer over that executor.

### 3ZB2 texture to port (treat the C as pseudocode)
- **Sequential route memory**: 3ZB2 follows a memorized route index chain (`routeindex++`)
  rather than re-planning every tick (`vendor/3zb2-zigflag/src/bot/bot_za.c`). Emulate:
  plan once with `Navigator`, then *commit* to the polyline, replanning only on goal change,
  route failure, or combat interrupt — this alone gives 3ZB2's characteristic "purposeful
  runner" feel vs `main`'s reactive re-planning.
- **`Search_NearlyPod` shortcut** (`bot_za.c:2214-2247`): while following, if a node further
  along the committed path is visible (BSP LOS via `brain::los`) and closer than the current
  waypoint chain, skip directly to it.
- **Mover states** (`GRS_ONPLAT`, `GRS_ONTRAIN`, `GRS_TELEPORT` in `g_spawn.c`): on a ride/
  ladder edge, enter a dedicated state that delegates to the traversal executor and holds the
  route index until the traversal completes (3ZB2's "don't advance the route while carried").
- **Combat**: reuse `combat.rs`/`aim.rs` (as `q3` reuses shared aim); 3ZB2-specific flavor
  (its aggressive item-route weapon runs) can come from `items::best_item_goal_weighted`
  with weapon-hunger biases — do not fork combat internals for v1.

## Step-by-Step Tasks

### T1: `Zb2Brain` skeleton + plugin wiring

**Files**: `crates/brain/src/brains/zb2.rs` (new), `crates/brain/src/brains/mod.rs`,
`crates/qbots/src/main.rs` (CLI), fleet config, competition `--brains`

**What to do**: Add `BrainKind::Zb2` (tag `zb2`), `build_brain` arm, CLI/config/competition
plumbing — mirror exactly how `Quake3` was added (Plan 37 commits are the template). The
skeleton ticks: roam via `Navigator` to `BrainMap.roam_nodes`, shared combat driver, shared
recovery. Compiles clean; `connect-one --brain zb2` walks and fights.

### T2: Committed-route follower + `Search_NearlyPod` shortcut

**File**: `crates/brain/src/brains/zb2.rs`

**What to do**: Store the planned polyline + current index; advance on arrival; replan only
on: goal change, `Navigator` failure/stuck escalation, or combat target change. Implement
the shortcut scan (cap lookahead ~6 nodes; LOS via `brain::los` BSP trace; require the skip
target be closer in path-distance terms). Unit-test the skip logic with a synthetic polyline
+ mock LOS.

### T3: Mover route-states over the traversal executor

**File**: `crates/brain/src/brains/zb2.rs`

**What to do**: When the committed edge is `Ride` (train/lift/ladder) or `Swim`, enter
`ZbState::OnMover`/`InWater` and delegate movement to the Plan 46 executor; freeze the route
index until the executor reports the edge complete; resume sequential following. No
duplicated ride/ladder code — that is the whole point of Plan 46.

### T4: Item-run flavor (small)

**File**: `crates/brain/src/brains/zb2.rs`

**What to do**: Goal selection = `items::best_item_goal_weighted` with 3ZB2's weapon-run
bias (over-weight weapon pickups until armed, like its weapon-aware route selection). Keep
it parameter-level; no new item model.

### T5: Live proof + competition vs `q3`/`main`

**Files**: `context/mode_perf.md`, `context/brain_notes.md`

**What to do**:
```bash
qbots connect-one --brain zb2                                    # sanity: plays, no panics
qbots spawn-to-weapon railgun --brain zb2 --map q2dm1            # swims (via executor)
qbots competition --count 4 --brains q3,zb2 --navmodes hybrid-fallback   # 5 min
qbots competition --count 4 --brains main,zb2 --navmodes hybrid-fallback # 5 min
```
Record K/D tables (q2dm1 + q2dm3) in `mode_perf.md`; append a dated `brain_notes.md`
section (mandatory). No win threshold — the deliverable is a *distinct, competent* third
brain; tune to at least mid-pack (beat `main`'s 0.68 baseline kd or document why not).

> **Rule B reminder**: commit after *each* task; fmt + clippy(-D warnings) + tests green
> before every commit.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/brains/zb2.rs` | new brain (route memory, shortcut, mover states) | P0 |
| `crates/brain/src/brains/mod.rs` | `BrainKind::Zb2` + factory + tag | P0 |
| `crates/qbots/src/main.rs` + config | `--brain zb2`, competition matrix | P1 |
| `context/mode_perf.md`, `context/brain_notes.md` | results + notes | P1 |

## Open Questions / Risks

1. **Committed routes can go stale** (mover moved, path failed). *Mitigation*: replan
   triggers in T2; the recovery escalation already signals hard failure.
2. **Shortcut skips onto un-walkable straight lines** (LOS ≠ walkable — the classic bot
   trap). *Mitigation*: only skip to nodes on the *committed path* (never arbitrary graph
   nodes), and require a hull-friendly height delta (≤ STEP) between current pos and target.
3. **Plan 46 not landed yet** → T3 blocked. *Mitigation*: sequence after Plan 46; T1/T2 are
   independent and can land first.
4. **Scope creep toward porting 3ZB2's CTF/team logic**. Out of scope for v1 (DM only).

## Verification Checklist

- [ ] T1: `--brain zb2` connects, roams, fights; all brains still build; commit.
- [ ] T2: unit tests for route commitment + shortcut skip pass; live: visibly runs routes
      (fewer replans/tick than `main` in logs); commit.
- [ ] T3: q2dm3 `spawn-to-weapon railgun --instance 1 --brain zb2` reaches (rides); q2dm1
      `spawn-to-weapon railgun --brain zb2` reaches (swims); commit.
- [ ] T4: item weapon-run bias active when Blaster-armed; commit.
- [ ] T5: two 5-min competitions recorded in `mode_perf.md`; `brain_notes.md` appended; commit.
- [ ] fmt + clippy(-D warnings) + tests green before each commit.
