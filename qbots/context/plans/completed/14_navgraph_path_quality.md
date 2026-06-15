# Plan 14 — Nav-Graph & Path Quality (DEFERRED)

> **Status**: deferred
> **Created**: 2026-06-15
> **Depends on**: Plan 10 (recorder to measure path length/elapsed)
> **Goal**: Make the *routes themselves* shorter and smoother — eliminate grid stair-stepping,
> connect spawn points, and add jump links — so bots stop cutting corners into walls and reach
> goals faster. The graph-level complement to the controller-level work in Plan 12.

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Four graph-quality improvements, each independently shippable:

1. **Funnel / string-pull smoothing** — collapse a grid path `[n0,n1,…,nk]` into the longest
   legal straight runs (apex-funnel over the walked corridor) so the bot runs straight between
   corners instead of zigging at every 64 u node.
2. **Jump links** — add `NODE_LANDING`-style up/down edges where a hull trace shows a
   standable surface across a gap/step the walk-edge rejected (Eraser jump nodes, distilled §9).
3. **Spawn-point connectivity** — seed nav nodes at every DM spawn (`info_player_deathmatch`,
   from Plan 10 T1) so a freshly-spawned bot always has a usable nearest node.
4. **Node redundancy pruning** — collapse near-duplicate grid nodes (within `STEP`) to cut
   graph size and A* noise.

**Deliverables**: smoother paths (fewer, longer segments), provably-connected spawns, and a
`nav graph --stats`/recorder "path efficiency" metric (`path_len / straight_line`) to quantify.

**Estimated effort**: Medium–Large (2 days) — funnel is the meat; jump links need careful
trace validation. **Deferred** because Plans 11–13 deliver the bulk of the user-visible
quality; this squeezes elapsed time and corner-clipping once the controller is sane.

---

## Context

### Why deferred

The user's symptoms (orbiting, wrong-way-facing, wall-grinding, not-chasing) are dominated by
the *controller* (Plan 12) and *perception* (Plan 11) bugs, plus stuck behavior (Plan 13).
A smoothed path through a bad controller still looks bad. Land 11–13 first, measure with Plan
10, *then* pursue graph quality for elapsed-time wins. The grid-zigzag does cause real
"wrong turns" and slow times, but it's the lowest-risk-after-baseline item.

### Key facts

- Graph is a spacing-**64** uniform grid over `bsp.models[0]` bounds
  (`main.rs:1007`, `navgraph.rs:30`). Edges are hull-traced + step-gated (`navgraph.rs:59-66`).
- `components()` (`navgraph.rs:96`) already diagnoses fragmentation; multi-level maps often
  split into several components (lifts/stairs not captured) → unreachable goals.
- Eraser funnels via its withheld NavLib; we reimplement the **Simple Stupid Funnel Algorithm**
  (Demyen 2007) over our polygon-free corridor — approximated by successive LOS tests between
  non-adjacent path nodes (a "string-pull": keep the furthest node still LOS-clear from the
  current apex). No portal polygons needed; LOS via `CollisionModel::trace`.
- Jump nodes (Eraser `NODE_LANDING`, distilled §9): a node stores a launch velocity + landing
  target; movement reads `goalentity->velocity` as the jump vector. We approximate: an edge
  tagged `Jump { launch_dir }` that the steering controller (Plan 12) turns into
  `mv.jump()` + a forward burst.

---

## Step-by-Step Tasks

### T1: Funnel / string-pull smoothing

**File**: `crates/brain/src/nav.rs` (+ `crates/world` LOS reuse).

**What to do**: Post-process the A* `current_path` into a smoothed `Vec<usize>` "apex" list:
from the bot, walk forward through the path; for each candidate node `n_j` (j > apex+1), if
`trace(apex_pos, node[n_j])` is clear (`fraction>=1`, hull), keep extending; on the first
block, commit `n_{j-1}` as the new apex. Output the apex subsequence. `pursue_target`
(Plan 12 T3) then aims along the *smoothed* path → straight runs between corners.

**Tests**: an L-shaped grid path → smoothed to the corner + exits; a straight run collapses
to 2 nodes; a blocked chord stops at the right apex.

**Verify**: `cargo test -p brain`; `cargo clippy -p brain -- -D warnings`.

### T2: Jump links

**File**: `crates/world/src/navgraph.rs`.

**What to do**: During generation, for each node, probe forward over a gap (a down-trace that
finds floor beyond a `GAP=48..96` horizontal span with no walk-edge) — if a standable surface
exists and a hull-trace arc is clear, add an edge tagged `EdgeKind::Jump { launch_yaw }`.
Expose `EdgeKind` on adjacency so the steering controller can trigger `mv.jump()` + forward
when traversing one. Conservative: only add when the land node is also a sampled floor node.

**Tests**: synthetic gap → one jump edge added, no walk edge across the gap.

**Verify**: `cargo test -p world`; `nav` CLI shows edge count rise on a multi-level map.

### T3: Spawn-point seeding + connectivity check

**File**: `crates/world/src/navgraph.rs` (or `supervisor::build_map_nav`).

**What to do**: After `NavGraph::generate`, add a node at each DM spawn origin (from Plan 10
T1 `Bsp::spawn_points()`) if none exists within `STEP`, hull-trace-connect it to neighbors.
Then assert every spawn is in the **largest component** (else log a warning — that spawn is a
known-bad start). This guarantees `nav.nearest(spawn_origin)` always returns a connected node.

**Tests**: a graph + a spawn off the grid → spawn seeded and connected to the component.

**Verify**: `cargo test -p world`; `nav` CLI on the target map reports all spawns connected.

### T4: Path-efficiency metric + redundancy prune

**File**: `crates/brain/src/recorder.rs`, `crates/world/src/navgraph.rs`.

**What to do**:
1. Recorder: add `path_efficiency = straight_line_distance / actual_path_len` to the SUMMARY
   (closer to 1.0 = less zigzag). This is the headline metric for *this* plan.
2. Optional node-prune: merge nodes within `STEP/2` of each other (keep one, rewire edges) to
   shrink the graph. Only if generation is slow or A* churn is visible — measure first.

**Verify**: before/after `path_efficiency` and graph size in the tracker.

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/nav.rs` | funnel smoothing; `pursue_target` over smoothed path | P0 (within plan) |
| `crates/world/src/navgraph.rs` | `EdgeKind::Jump`; spawn seeding; optional prune | P0 |
| `crates/brain/src/recorder.rs` | `path_efficiency` metric | P1 |

---

## Open Questions / Risks

1. **Funnel through drift**: string-pulling assumes the bot is *on* the path; if Plan 13
   recovery has pushed it off-edge, the smoothed apex may clip. Mitigation: re-run smoothing
   from the current position each replan; fall back to raw path when off-corridor.
2. **Jump link validation is the riskiest part** — a wrong jump edge launches the bot into a
   pit. Gate on conservative traces and make `EdgeKind::Jump` opt-in per map until trusted.
3. **Graph rebuild cost**: changes here invalidate the cached `Arc<NavGraph>` (Plan 09
   `NavCache`); fine (built once per map), but jump-link probing adds gen time — budget it.
4. **Defer trigger**: only start this plan once Plan 10 baseline + Plans 11–13 are measured;
   if elapsed time is already good, the funnel may be low-ROI — decide from the recorder data.

---

## Verification Checklist

- [ ] T1: funnel smoothing unit tests (L-path, straight collapse, blocked apex).
- [ ] T1: `pursue_target` follows smoothed path; `cargo clippy -p brain` clean.
- [ ] T2: jump-edge synthetic test; `nav` CLI edge count rises on a multi-level map.
- [ ] T2: `cargo clippy -p world` clean.
- [ ] T3: every DM spawn is in the largest component (`nav` CLI assertion).
- [ ] T4: `path_efficiency` in SUMMARY; before/after recorded.
- [ ] End-to-end: `spawn-to-spawn` elapsed time and `path_efficiency` both improve vs the
      post-Plan-12/13 baseline, with no new corner-clipping (`bumps` not worse).
