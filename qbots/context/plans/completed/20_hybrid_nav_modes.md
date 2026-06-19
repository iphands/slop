# Plan 20 — Hybrid Navigation Modes

> **Status**: done
> **Created**: 2026-06-18
> **Depends on**: Plan 14 (nav-graph quality), Plan 10 (movement harness); navmesh backend
> **Goal**: Add four `hybrid-*` `--mode` backends that combine the A* waypoint graph and the navmesh, selectable alongside the untouched `astar`/`navmesh` controls.
> **Agent**: sub-agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Build four new navigation backends that delegate between the existing
`NavigationDriver` (A* waypoint graph) and `NavmeshDriver` (polygon mesh + funnel),
each a new `Navigator` impl, selectable via `--mode hybrid-{fallback,race,hier,segment}`.

**Deliverables**:
1. `hybrid-fallback` — A* primary; on hard-stuck, hand the segment to navmesh, back to A* next goal.
2. `hybrid-race` — plan both per goal, score, run the winner to completion.
3. `hybrid-hier` — navmesh global corridor + A* local execution toward a sliding sub-goal.
4. `hybrid-segment` — navmesh open routing; A* owns jump-link segments only.
5. Read-only planner accessors + a shared navigator factory used by both dispatch sites.

**Estimated effort**: Medium (1 day)

---

## Context

qbots has two interchangeable nav backends behind the `Navigator` trait
(`crates/brain/src/nav_mode.rs`): `astar` (waypoint graph — jump/drop/ledge edges,
blacklists, risk overlay, rich stuck recovery) and `navmesh` (Recast-style polys +
funnel — strong open-area routing, stair bridges, but `current_edge_is_jump()` is
always `false`, so it cannot execute jump links). Their strengths are complementary:
the graph knows *how* to traverse tricky geometry; the navmesh is better at *where to
go* over open space.

This plan adds four architectures that combine them, so we can A/B them against the
Plan 10 movement baselines. The integration seam is clean: the bot tick loop and the
movement scenarios drive navigation **only** through `Navigator` trait methods, so each
hybrid is a new `Navigator` impl owning both sub-drivers — no steering/movement changes.

### Key Facts
- Two dispatch sites build `Box<dyn Navigator + Send>`: `crates/qbots/src/main.rs:615`
  (`bot_task`) and `crates/qbots/src/scenario.rs:261` (movement scenarios). A shared
  factory in the **qbots** crate (lazily building the mesh) keeps `brain` free of the
  `supervisor` dependency where `get_or_build_navmesh` lives.
- The goal-dispatch `match (mode, nav_graph)` at `main.rs:833` keys only on
  `NavMode::Navmesh`; hybrids fall through to `_ => NavGoal::Waypoint(node)` and convert
  the waypoint→position internally before handing it to their navmesh sub-driver
  (navmesh ignores `NavGoal::Waypoint`).
- Reusable primitives: `pursuit::project_onto_path` + `pursuit::point_ahead` (sliding
  sub-goal); `NavGraph::{node_pos, edge_kind, path_len, neighbors}`; `EdgeKind::Jump`.
- The hard-stuck signal is external: the scenario/tick loop runs `StuckDetector` and
  calls `Navigator::force_replan()` / `blacklist_waypoint_if_blocked()` on the driver.
  `hybrid-fallback` interprets a `force_replan()` while A* is active as "A* is stuck".

### Why delegate, not rewrite
Both sub-drivers already encode hard-won Quake semantics. The hybrids are thin
supervisors that pick which sub-driver answers each trait call; the heavy lifting stays
in the proven drivers.

---

## Step-by-Step Tasks

### T1: Read-only planner accessors

**Files**: `crates/brain/src/navmesh_driver.rs`, `crates/brain/src/nav.rs`

**What to do**: Add (no behavior change):
- `NavmeshDriver::path(&self) -> &[Vec3]` and `planned_len(&self) -> Option<f32>`
  (sum of funnel segment lengths; `None` when `path.len() < 2`).
- `NavigationDriver::planned_cost(&self) -> Option<f32>` (`nav_graph.path_len(&current_path)`,
  `None` when path empty) and `planned_jump_count(&self) -> usize` (count `current_path`
  windows whose `edge_kind` is `Jump`).

Unit-test `planned_len` (3-point line) and `planned_cost`/`planned_jump_count` on a tiny
graph. Commit `task(T1): add read-only planner accessors for hybrid scoring`.

### T2: `hybrid/` module scaffold

**Files**: `crates/brain/src/hybrid/mod.rs`, `crates/brain/src/lib.rs`

**What to do**: New `hybrid` module with a `Backend { Astar, Navmesh }` enum, a
`Sub` struct holding `astar: NavigationDriver` + `navmesh: NavmeshDriver` + a
`Arc<NavGraph>` (for waypoint→position), and `fn goal_to_pos(graph, goal) -> NavGoal`
mapping `NavGoal::Waypoint(n)` → `Position(node_pos(n))` (pass other goals through).
Register `pub mod hybrid;` in `lib.rs`. Compiles, no modes wired. Commit
`task(T2): scaffold hybrid nav module (Sub helper + goal_to_pos)`.

### T3: `hybrid-fallback`

**Files**: `crates/brain/src/hybrid/fallback.rs`, `crates/qbots/src/{main.rs,scenario.rs}`,
add `NavMode::HybridFallback` to `main.rs`.

**What to do**: `HybridFallback` delegates to `astar` while `active == Astar`. Track the
current goal; a changed goal resets `active → Astar`. A `force_replan()` while
`active == Astar` flips to `Navmesh` (seed navmesh with current goal+pos via
`goal_to_pos`). When `active == Navmesh`, delegate. Wire dispatch via the new factory.
Unit-test the astar→navmesh→(new goal)→astar transition. Commit
`task(T3): hybrid-fallback nav mode (A* primary, navmesh on stuck)`.

### T4: `hybrid-race`

**Files**: `crates/brain/src/hybrid/race.rs`, dispatch + `NavMode::HybridRace`.

**What to do**: On a changed goal, `set_goal` on both; score each
`= planned_len + JUMP_PENALTY*jump_count + STUCK_BIAS*recent_stuck[backend]`
(navmesh uses `planned_len`, no jump edges; A* uses `planned_cost` + `planned_jump_count`).
Run the lower-scoring backend to completion; stuck recovery replans the active backend.
Unit-test the scorer picks the cheaper plan. Commit
`task(T4): hybrid-race nav mode (plan both, run the winner)`.

### T5: `hybrid-hier`

**Files**: `crates/brain/src/hybrid/hier.rs`, dispatch + `NavMode::HybridHier`.

**What to do**: `set_goal` plans the navmesh corridor. Each tick, project the bot onto
`navmesh.path()` and take `point_ahead(.., LOCAL_HORIZON≈300)` as a sliding sub-goal fed
to `astar.set_goal(Position(sub))`. `pursue_*` + `current_edge_is_jump` delegate to A*
(local executor, so jump links fire). If navmesh has no path, A* drives straight to goal.
Commit `task(T5): hybrid-hier nav mode (navmesh corridor + A* local)`.

### T6: `hybrid-segment`

**Files**: `crates/brain/src/hybrid/segment.rs`, dispatch + `NavMode::HybridSegment`.

**What to do**: Default `active == Navmesh`. Each tick, scan the graph for a node within
`R` of the bot with an outgoing `EdgeKind::Jump` aimed roughly toward the goal; if found,
flip `active → Astar` so the graph executes the jump link (its `current_edge_is_jump`
drives the loop's jump press + `launch_yaw`), then flip back once past it. Commit
`task(T6): hybrid-segment nav mode (navmesh open + A* jump links)`.

### T7: Docs + close plan

**Files**: `context/distilled.md`, `context/pitfalls.md`, `SERIES.md`, this plan + tracker.

**What to do**: Note the four modes + trade-offs in `distilled.md`; record any gotchas in
`pitfalls.md`; `git mv` plan + tracker to `completed/`; mark SERIES row done. Commit
`task(T7): document hybrid modes; close Plan 20`.

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/nav.rs` | `planned_cost`/`planned_jump_count` accessors | P0 |
| `crates/brain/src/navmesh_driver.rs` | `path`/`planned_len` accessors | P0 |
| `crates/brain/src/hybrid/{mod,fallback,race,hier,segment}.rs` | new modes | P0 |
| `crates/brain/src/lib.rs` | register `hybrid` module + re-exports | P0 |
| `crates/qbots/src/main.rs` | 4 `NavMode` variants + factory call | P0 |
| `crates/qbots/src/scenario.rs` | factory call (2nd dispatch site) | P0 |

## Open Questions / Risks

1. **fallback switch trigger reuses `force_replan`** — also called in non-stuck contexts?
   Mitigation: in the tick loop `force_replan` is raised by `StuckDetector::Hard`; a
   spurious early switch only costs one navmesh segment, then resets on the next goal.
2. **hier/segment conceptual overlap** — keep them code-distinct (hier = all local
   execution; segment = jump links only) so the A/B comparison is meaningful.
3. **Building both sub-drivers per bot** — cheap: graph already loaded; mesh cached by
   `get_or_build_navmesh`. Factory builds the mesh lazily (only for navmesh/hybrid modes).

## Verification Checklist

- [ ] T1: `cargo test -p brain` — `planned_len`/`planned_cost`/`planned_jump_count` covered.
- [ ] T2: `cargo build` clean; `hybrid` module compiles, no modes wired.
- [ ] T3: unit test covers fallback's astar→navmesh→astar transitions; `--mode hybrid-fallback` runs.
- [ ] T4: unit test covers the race scorer; `--mode hybrid-race` runs.
- [ ] T5: `--mode hybrid-hier` reaches goal on `spawn-to-spawn` (exit 0) on q2dm1.
- [ ] T6: `--mode hybrid-segment` runs; jump-link segments delegate to A*.
- [ ] All: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test` green before each commit.
- [ ] Controls unchanged: `--mode astar` / `--mode navmesh` behave as before.
- [ ] Each mode's `# SUMMARY` line compared to the Plan 10 baselines for both scenarios.
