# Plan 42 — Moving-platform (`func_train`) + lift nav-graph integration for q2dm3

> **Status**: done (2026-07-09 — all nav work shipped, cache v18; quad + loop railgun
> A*-reachable from all 7 spawns; T6 offline test landed `crates/world/tests/ride_q2dm3.rs`)
> **Created**: 2026-06-19
> **Depends on**: Plan 17 (BSP/collision), Plan 18 (map cache), Plan 39 (EdgeKind pattern), Plan 34/35 (q2dm3 connectivity diagnosis)
> **Goal**: Represent q2dm3's horizontal moving platforms (`func_train`) and its vertical lift (`func_plat`/`func_door`) in the nav graph as traversable **ride edges**, so A* can plan routes onto the quad and the loop-train railgun, and q2dm3's connectivity gate passes → `generate-map-cache q2dm3` succeeds.
> **Agent**: TBD

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: The nav graph models `func_plat`/`func_door` vertical lifts but has **no concept of
`func_train`** — horizontally moving platforms that ride along `path_corner` loops. On q2dm3
the quad and the "tricky" railgun sit in small isolated nav components reachable *only* by
riding a `func_train`. Add `EdgeKind::Ride` train edges (board node → dismount node) plus a
movement summary the brain can consume, and fix the lift anchoring so the railgun's elevator
joins the graph. Result: q2dm3's quad/railgun components fuse into the reachable graph.

**Deliverables**:
1. `EdgeKind::Ride` (train/plat carry) with the data the brain needs (platform entity key +
   board/dismount world positions + which path endpoint to wait at).
2. `func_train` parsing: model + `path_corner` chain → ride segment(s); **board nodes** (ground
   nodes adjacent to a path endpoint at platform-top height) and **dismount nodes** (reachable
   floor/ledge adjacent to the *other* endpoint); ride edges wired both ways where valid.
3. Lift anchoring fix so q2dm3's `func_plat` `*2` (railgun elevator) + `func_door` t4 top
   landings join the graph (the Plan 34 deep finding).
4. q2dm3 nav: quad `(192,320,216)`, loop-train railgun `(768,816,208)` join the main component;
   `check_spawn_connectivity` passes (coordinate with Plan 35 for the broad `walkable_stair`
   regression — see Risks). `mapcache::VERSION` bumped; offline reachability tests.

**Estimated effort**: Large (uncertain; iterative nav geometry work).

## Context

### Ground truth — q2dm3 movers (dumped 2026-06-19 from `vendor/baseq2/pak1.pak`)

`func_train` follows `path_corner` entities (its `target` names the *first* corner; each
corner `target`s the next, forming a loop). The standable surface is the **inline model's top**
at the corner's XY (the path_corner origin is the model's *origin* reference, not the floor).

- **Quad train `*10`** — `target t1`, `speed 60`. Path: `t1 (143,-296,184) ↔ t2 (143,88,184)`
  (oscillates in Y at z≈184). Rider boards near one endpoint, rides to the `t2` end, then
  jumps onto the static ledge leading to the **quad `(192,320,216)`** (nav comp 29, size 12).
- **Loop trains `*3` (target t14) and `*4` (target t9)** — `speed 60`. Both circulate the
  10-corner loop `t6…t15` at **z=-120** (XY roughly 584–896 × 592–824). These carry a player
  across the lower area to where the **`func_plat` elevator** lifts up to the **railgun
  `(768,816,208)`** (nav comp 62, size 9). User: "hop on one of the two moving platforms …
  then hop onto the area that has elevator OR ladder that leads to the railgun."
- **`func_plat` `*2`** — the railgun elevator (rests top, travels down). Already handled by
  `add_elevator_edges`/`try_add_plat`, but its top landing may not anchor (Plan 34 finding).
- **`func_door` `*5`,`*6`** — `targetname t4`, `spawnflags 4`, button-triggered; geometry only.
- Upper railgun `(-368,-64,352)` + `item_invulnerability (-224,-32,352)` sit in comp 18
  (size 6) — a *separate* upper ledge; not the user's target. Out of scope unless it falls
  out for free.

### Current nav lift code (the pattern to extend)

- `crates/world/src/build.rs:188` `add_elevator_edges` → `try_add_plat`/`try_add_vertical_door`
  → `add_lift` (node@top, node@bottom, ride edge, `connect_node_to_nearby(256)`).
- `add_lift` puts nodes at `[cx, cy, surface_z + 24]` (player origin = floor + 24).
- `EdgeKind` (`navgraph.rs:82`): `Walk | Jump{launch_yaw} | Swim`. Edge-kind sets stored as
  `HashSet<(usize,usize)>` + side tables (see `swim_edges`, `jump_yaws`). **Mirror this for
  `Ride`.**

### Why a new `EdgeKind` (not reuse the lift ride edge)

A `func_plat` ride is *vertical and stationary in XY*: the brain stands on a pad and waits. A
`func_train` ride is *horizontal and moving*: the brain must board at the moving platform's
**current** position (read live from frames) and step/jump off at the dismount end. The brain
needs to distinguish them and know the platform's entity + board/dismount points — data a bare
cost edge can't carry. `EdgeKind::Ride` carries that; the brain side is Plan 43.

### Design: ride edge = (board_node, dismount_node, RideInfo)

For each `func_train`:
1. Resolve the inline model + its `path_corner` chain; compute each corner's **platform-top
   world Z** (`model.maxs[2]` offset applied at the corner XY) and the standable origin
   `[corner.x, corner.y, top_z + 24]`.
2. For the two *useful* endpoints (oscillator: both ends; loop: the corners adjacent to a
   reachable ledge), synthesize a node at the endpoint's standable origin.
3. A **board node** = a normal walkable graph node within ~`BOARD_RADIUS` (and ≤ small dz) of
   an endpoint node → `Walk`-adjacent to the endpoint node.
4. A **dismount node** = a walkable node near the *other* endpoint (the ledge the rider steps
   onto). Connect endpoint→dismount with a `Ride` edge (and the reverse where symmetric).
5. Ride cost = path length along corners (so A* prefers a short ride; trains are slow but they
   are the *only* link, so no penalty hack needed — unlike the lift).

Keep it **q2dm3-correct first**; generalize cautiously. Guard every synthesized edge with a
real trace (board/dismount must be genuinely walkable floor) so we never invent a false bridge
(the recurring nav failure mode — see `context/pitfalls.md`).

## Step-by-Step Tasks

### T1: `EdgeKind::Ride` + side tables + `edge_kind`/`is_ride_edge`

**File**: `crates/world/src/navgraph.rs`

**What to do**: Add `Ride { /* discriminant only; details in side table */ }` to `EdgeKind`.
Add `ride_edges: HashSet<(usize,usize)>` and a `ride_info: HashMap<(usize,usize), RideInfo>`
(define `RideInfo { board: [f32;3], dismount: [f32;3], wait_at: [f32;3], model_index: usize }`).
Extend `edge_kind`, add `is_ride_edge`, `ride_info(from,to)`, and an `add_ride_edge(...)`.
Protect ride edges from the false-edge prune (mirror `swim_edges` protection). Keep node
ordering deterministic (cache byte-stability — see Plan 18).

### T2: `func_train` parse → corners + platform-top heights

**File**: `crates/world/src/build.rs` (+ helper in `navgraph.rs` if needed)

**What to do**: `fn train_corners(bsp, entity) -> Vec<Corner>` following `target`→`targetname`
chain (stop on cycle/loop close; cap iterations). For each corner compute the standable origin
using the inline model's top surface (reuse `entity_model` + `model.maxs[2]`). Unit-test the
chain walk on the q2dm3 `t1↔t2` (oscillator) and `t6…t15` (loop) data baked as fixtures.

### T3: board/dismount node synthesis + `add_train_edges`

**File**: `crates/world/src/build.rs`

**What to do**: `fn add_train_edges(graph, cm, bsp) -> usize`, called from `generate_map_nav`
alongside `add_elevator_edges`. For each train: synthesize endpoint nodes; find nearby walkable
board/dismount nodes via trace-checked `connect_node_to_nearby` (tight radius + dz gate); add
`Ride` edges endpoint→dismount and board→endpoint. **Trace-guard** each. Log every edge added.

### T4: lift anchoring fix (railgun elevator top landing)

**Files**: `crates/world/src/build.rs` (`add_lift`/`try_add_plat`), `navgraph.rs` helper

**What to do**: When a lift `top_node` connects to **zero** nearby nodes, probe world-floor at
XY positions just *outside* the shaft footprint at the upper level, synthesize an anchor node
in the upper component, and connect. (Plan 34 T4, reused here for q2dm3 `*2`.) Confirm the
railgun elevator top joins the loop-train dismount area.

### T5: q2dm3 connectivity + cache regen

**Files**: `crates/world/src/mapcache.rs` (`VERSION` + `Fingerprint`), regen step

**What to do**: Bump `mapcache::VERSION` (graph geometry changed) and add any new constant
(`BOARD_RADIUS`, ride cost factor) to the `Fingerprint`. Run `navinspect q2dm3 navquery` (live)
on quad/railgun — confirm they now share the main component with the spawns. Run
`generate-map-cache --map q2dm3` → caches. Coordinate with Plan 35 for the remaining
`walkable_stair` regression if the gate still trips on the non-mover spawns.

### T6: offline reachability tests

**File**: `crates/world/tests/` (gated, like Plan 39's q2dm1 water tests)

**What to do**: Add a test that builds q2dm3 nav and asserts an A* path exists from a DM spawn
to the quad node and to the loop-train railgun node, and that the path includes ≥1 `Ride` edge.
Mark `#[ignore]`/feature-gate if it needs the pak (mirror existing gated map tests).

> **Rule B reminder**: commit after *each* task. fmt + clippy(-D warnings) + tests green before
> every commit. Bump `mapcache::VERSION` in the **same** commit as any generation change.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/world/src/navgraph.rs` | `EdgeKind::Ride`, side tables, accessors, prune-protect | P0 |
| `crates/world/src/build.rs` | `train_corners`, `add_train_edges`, lift anchoring | P0 |
| `crates/world/src/mapcache.rs` | `VERSION` bump, `Fingerprint` fields | P0 |
| `crates/world/tests/` | q2dm3 quad/railgun reachability (ride-edge) tests | P1 |

## Open Questions / Risks

1. **Broad `walkable_stair` regression (Plan 35) may still fail the q2dm3 gate** for non-mover
   spawns even after ride edges land. *Mitigation*: this plan owns the *mover* connectors;
   sequence with or fold in Plan 35 T2 so q2dm3 reaches full 7/7. Track which spawns remain.
2. **False ride bridges** (the classic nav failure). *Mitigation*: trace-guard every board/
   dismount node; tight radius + dz gate; the T6 test asserts the path *uses* a ride edge, and
   a live spawn-to-* (Plan 43) is the false-bridge guard.
3. **path_corner top-surface height math** (model origin vs maxs[2]) is easy to get wrong →
   nodes float/sink. *Mitigation*: reuse `add_lift`'s `surface_z + 24` convention; verify with
   `navinspect navquery` Δz≈8 like the existing nodes.
4. **Two railguns**: `find_class().first()` is `(-368,-64,352)`; the user wants `(768,816,208)`.
   Resolved by Plan 41 `--instance`; this plan just makes the loop-train one *reachable*.
5. **Trains oscillate vs loop** — different endpoint semantics. *Mitigation*: T2 handles both;
   q2dm3 has one oscillator (`*10`) and two loopers (`*3`,`*4`).

## Verification Checklist

- [ ] T1: `EdgeKind::Ride` round-trips through `edge_kind`/`is_ride_edge`; prune keeps ride edges.
- [ ] T2: `train_corners` unit tests pass on q2dm3 `t1↔t2` and `t6…t15` fixtures.
- [ ] T3: `add_train_edges` logs ≥3 ride edges on q2dm3; every endpoint trace-guarded.
- [ ] T4: q2dm3 `func_plat *2` top landing has ≥1 neighbor (no orphan).
- [ ] T5: `navinspect q2dm3 navquery` shows quad + loop-train railgun in the main component;
      `generate-map-cache --map q2dm3` succeeds (with Plan 35 for residual spawns).
- [ ] T6: q2dm3 A* path spawn→quad and spawn→railgun exists and contains a `Ride` edge.
- [ ] `mapcache::VERSION` bumped; fmt + clippy(-D warnings) + tests green before each commit.
