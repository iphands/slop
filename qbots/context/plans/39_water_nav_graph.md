# Plan 39 — Water Nav Graph (swim nodes + swim edges)

> **Status**: pending
> **Created**: 2026-06-19
> **Depends on**: Plan 05 (world), Plan 17 (BSP/collision), Plan 18 (map cache)
> **Goal**: Make the A* nav graph traverse water — sample submerged/surface swim
> nodes and connect them with `EdgeKind::Swim` edges (incl. dry↔water entry/exit), so
> the q2dm1 railgun room (reachable only by swimming) joins the main nav component.
> **Agent**: implementation agent (ralph-loop / sub-agent)

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Teach the A* `NavGraph` to represent water. Today **every** water position is
discarded during nav generation (`navgraph.rs:1200`, `heightfield.rs:258`), so any route
that crosses water has no nodes/edges. On q2dm1 the railgun room is reachable **only** by
jumping into water, swimming a submerged tunnel, and surfacing — so its dry floor is an
**isolated component**. This plan adds water sampling + swim connectivity to the A* graph.

**Deliverables**:
1. `EdgeKind::Swim` + a `swim_edges` set + `NavGraph::current_edge_is_swim()` API
   (mirrors the existing `jump_edges` machinery).
2. Water **node sampling**: submerged lattice nodes + a water-**surface** node per water
   column (in addition to the existing dry-floor nodes).
3. Water **edges**: swim↔swim (3D, no STEP limit, cost-scaled for slow water move),
   plus dry-floor→water **entry** and water-surface→dry-floor **exit** edges (the latter
   is the railgun-ledge climb-out — the critical bridge).
4. Cache fingerprint bump (new SWIM constants) so stale caches auto-invalidate.
5. A reusable `navinspect contents`/`watermap` diagnostic mode (no tmp scripts).
6. **Offline proof**: an A* path from a q2dm1 spawn to the railgun origin
   `(240,-384,464)` now exists (today it is `NO PATH` — see Context).

**Estimated effort**: Medium (1 day)

---

## Context

### Confirmed root cause (verified 2026-06-19, offline)

- **Railgun origin** (q2dm1 entity lump, `weapon_railgun`): `240 -384 464`.
- **Dry floor nodes already exist** in the railgun room (`navinspect q2dm1 240 -384 464 200`
  lists nodes at z≈468–472) — the room floor is sampled fine.
- **But it is unreachable**: `navinspect q2dm1 navpath <spawn> 240 -384 464` returns
  **`NO PATH (… disconnected)`** from both a main-area spawn `(544,352,482)` and the
  nearest spawn `(1488,-48,664)`. The only route between the main area and the railgun
  room is **underwater**, and water is excluded from nav generation, so the two dry
  regions are in **separate components** with no edge bridging the water gap.
- The user's description matches the geometry exactly: *jump into the water from the main
  area → swim through a tunnel → swim up / hold jump to surface in the railgun room.*

### Why this is a bug, not a map property

Per `qbots/CLAUDE.md`: all q2dm locations are mutually reachable by design. The railgun is
reachable by a real player via swimming; our nav simply has **no representation of water**,
so it declares an in-bounds, reachable item unreachable. That is our bug.

### Where water is currently discarded (the two exclusion sites)

```rust
// crates/world/src/navgraph.rs:1200  (A* graph — THIS PLAN)
if cm.point_contents(&wp) & MASK_WATER == 0 {
    let stand = cm.trace(&wp, &wp, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
    if !stand.startsolid { results.push(wp); }
}
// crates/world/src/navmesh/heightfield.rs:258  (navmesh — OUT OF SCOPE, see Risks)
if headroom && cm.point_contents(&[x, y, oz]) & MASK_WATER == 0 { ... }
```

### Scope decision: A* graph only (navmesh water is a follow-up)

This plan adds water to the **A* `NavGraph`** (`crates/world/src/navgraph.rs`), the proven
backend used by `--navmode astar` and the `runtester` default. The navmesh
(`heightfield`→`polymesh`, Recast-style 2.5-D walkable surface) cannot represent a 3-D
water **volume** without a redesign, so the four navmesh/hybrid modes will still fail to
reach the railgun. **That is an intended, informative outcome**: Plan 40's ranking sweep
will show `astar` (and A*-backed hybrid segments) reach it while pure-`navmesh` does not.
Navmesh water is explicitly deferred (see Open Questions / Risks #1).

### Key facts (Q2 collision)

- `MASK_WATER = CONTENTS_WATER|CONTENTS_LAVA|CONTENTS_SLIME` (`collision.rs:23`). Swim
  nodes go only in **`CONTENTS_WATER`** — never lava/slime (deadly). Sample with
  `CONTENTS_WATER` specifically, not `MASK_WATER`.
- A submerged bot is **not** floor-constrained: a swim node is any non-solid point inside
  a water volume. Validate with a point/zero-extent solid trace (the full standing hull is
  too strict in tight tunnels — swimming bots can occupy tighter space; use a reduced hull,
  e.g. `±16,±16,−12..+12`, or a zero-extent point check + a small radius probe).
- Water "move speed" is ~0.5× ground (`pmove.c:579 wishspeed *= 0.5`), so swim edge cost
  should be scaled up (≈2×) vs. an equal-length walk edge, keeping A* preferring dry
  routes when one exists (here none does).

---

## Step-by-Step Tasks

> Commit after **every** task (Rule B). Run `cargo fmt`, `cargo clippy -- -D warnings`,
> `cargo test` green before each commit (Rule A).

### T1: `EdgeKind::Swim` + swim-edge bookkeeping + `current_edge_is_swim`

**Files**: `crates/world/src/navgraph.rs`, `crates/brain/src/nav.rs`,
`crates/brain/src/nav_mode.rs` (Navigator trait).

**What to do**: Add a `Swim` variant to `EdgeKind`; add a `swim_edges: HashSet<(usize,usize)>`
to `NavGraph` (parallel to `jump_edges`); update `edge_kind()`, `add_edge` callers, the
serialize/deserialize paths, `remove_edge`, and the constructor(s). Add
`NavGraph::is_swim_edge(a,b)` and, on the `Navigator` trait + `NavigationDriver` +
hybrid/stub impls, `current_edge_is_swim(&self) -> bool` mirroring
`current_edge_is_jump` (`nav.rs:290`). (Brain consumption is Plan 40 — here just expose it.)

**Note**: keep `Swim` and `Jump` mutually exclusive per edge. Serialization must round-trip
both sets (see mapcache T5).

### T2: Water node sampling (submerged lattice + surface node)

**File**: `crates/world/src/navgraph.rs` (extend `floor_waypoints_multi` or add a sibling
`water_waypoints` pass invoked from the same per-column `flat_map` in `generate`).

**What to do**: Per grid column `(x,y)`, in addition to the dry-floor probe, scan Z to find
`CONTENTS_WATER` spans. For each contiguous water span `[z_lo, z_hi]` (z_hi = surface):
- Emit a **surface node** at `z_hi` (bot floats with eyes ~at surface).
- Emit **submerged nodes** every `SWIM_SPACING` (new const, propose 32u) from just below
  the surface down to `z_lo + ε`, **and** a node just above the pool floor.
- Validate each candidate is non-solid via a reduced-hull trace (see Key Facts); skip
  startsolid points. Tag each emitted node as water (parallel `is_water: Vec<bool>` on
  `NavGraph`, or a `water_nodes: HashSet<usize>`; pick the cheaper-to-serialize one).

Keep determinism: water nodes participate in the same `(grid_key, z)` sort
(`navgraph.rs:113`) so node indices stay byte-identical across runs.

### T3: Swim edges — swim↔swim (3-D, vertical-aware)

**File**: `crates/world/src/navgraph.rs` (Phase-3 edge pass, ~`navgraph.rs:128`).

**What to do**: When connecting a node, if **either** endpoint is a water node:
- Consider **vertical** neighbors too (same/adjacent column, the node directly above/below
  within ~`SWIM_SPACING * 1.5`) — water needs 3-D adjacency, unlike the XY-only ground grid.
- Connect with a **3-D hull trace** (`MASK_SOLID`, reduced hull) — **bypass the `STEP`/
  `STAIR_MAX` height gates** entirely (vertical swim has no step limit).
- Edge cost = `3d_dist(a,b) * SWIM_COST_FACTOR` (new const, propose 2.0). Record the edge
  in `swim_edges`.

### T4: Dry↔water entry/exit edges (the railgun-ledge bridge)

**File**: `crates/world/src/navgraph.rs` (same edge pass; also extend
`connect_node_to_nearby` + `bridge_components` so scenario goal-seeding bridges through
water).

**What to do**:
- **Entry** (dry floor → adjacent water surface/submerged node): if a dry node is within
  the XY connect radius of a water node and a hull trace between them is clear, add a
  `Swim` edge (cost ~ normal; walking/falling in is cheap).
- **Exit** (water-surface node → nearby dry ledge node): if a surface node is within
  `~STEP + slack` vertically and the connect radius horizontally of a dry node, and the
  horizontal hull trace at the dry node's Z is clear, add a `Swim` edge. **This is the
  critical edge** that connects the railgun-room dry floor to the water column, hence to
  the main component.
- Ensure `bridge_components` (`navgraph.rs:~818`) will consider water nodes when stitching
  fragments, so the railgun component fuses with the main one.

### T5: Map-cache fingerprint bump + serialization

**File**: `crates/world/src/mapcache.rs`.

**What to do**: Bump the cache **version** and add `SWIM_SPACING`, `SWIM_COST_FACTOR`
(f32 bits) to `Fingerprint` so older caches auto-reject. Extend the on-disk format to
serialize the new `swim_edges` set and the water-node tag alongside `jump_edges`. Confirm a
round-trip test (write→read→equal graph incl. swim edges).

### T6: `navinspect contents` / `watermap` diagnostic mode

**File**: `crates/tools/src/bin/navinspect.rs`.

**What to do**: Add a reusable mode `navinspect <baseq2> <map> contents <x> <y> <z>` that
prints `point_contents` decoded (solid/water/lava/slime/empty), and a `watermap <x0> <y0>
<x1> <y1> <z> <step>` grid mode that marks water vs. air vs. solid (mirrors `scan`). This
replaces ad-hoc probing during this work and documents the railgun water column.
(No tmp scripts — Constraint #6.)

### T7: Offline connectivity proof + regression test

**Files**: `crates/world/tests/` (new), `crates/world/src/navgraph.rs` (unit tests).

**What to do**:
1. **Unit test** (synthetic): build a `CollisionModel` with a water box between two dry
   ledges; assert water nodes are sampled, swim edges connect them, and `path()` crosses
   the water (start ledge → swim → exit ledge).
2. **Integration test** (q2dm1, gated like `navinspect QBOTS_LIVE` / behind a feature or
   `#[ignore]` if it needs the pak): build the q2dm1 graph live and assert
   `path(nearest(spawn), nearest([240,-384,464]))` is `Some` for at least one spawn — i.e.
   the railgun is now reachable. Today this is `None`.
3. Manually confirm via `navinspect`: `navpath`/A* from `(1488,-48,664)` to
   `(240,-384,464)` returns a path.

### T8: Knowledge capture

**Files**: `context/distilled.md` (or `context/distilled/pathing/`), `context/pitfalls.md`.

**What to do**: Record the water-nav approach (submerged lattice + surface node + 3-D swim
edges + slow-move cost) in distilled, and a pitfall entry: *"all-water positions discarded
→ water-only routes (q2dm1 railgun) appear unreachable; fix = swim nodes/edges."* Update
`SERIES.md` to mark Plan 39 done and move plan+tracker to `completed/` (Rule C) once T1–T7
are green.

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/world/src/navgraph.rs` | `EdgeKind::Swim`, water node sampling, swim edges, bridging | P0 |
| `crates/world/src/mapcache.rs` | fingerprint bump + swim-edge/water-tag serialization | P0 |
| `crates/brain/src/nav.rs` / `nav_mode.rs` | `current_edge_is_swim` on driver + trait | P0 |
| `crates/tools/src/bin/navinspect.rs` | `contents` / `watermap` diagnostic modes | P1 |
| `crates/world/tests/*` | synthetic + q2dm1 reachability regression | P0 |
| `context/distilled.md`, `context/pitfalls.md` | capture approach + pitfall | P2 |

---

## Open Questions / Risks

1. **Navmesh water out of scope.** Pure `navmesh` + navmesh-corridor hybrids will still not
   reach the railgun. *Mitigation*: explicitly document; Plan 40's ranking shows the split;
   open a follow-up plan for navmesh water if the project wants all 6 modes to pass.
2. **Swim node explosion.** A volumetric lattice could add many nodes (cost/CPU/cache size).
   *Mitigation*: coarse `SWIM_SPACING` (32u), only sample columns that actually contain
   water, cap nodes per span; measure node count delta on all 8 maps via
   `generate-map-cache`.
3. **False swim edges through thin brush.** A 3-D trace with a reduced hull could clip a
   thin separator. *Mitigation*: keep the reduced hull modest; the trace is still
   `MASK_SOLID` so real geometry blocks; add the synthetic unit test for a separator wall.
4. **Reduced-hull tuning.** Too small → nodes inside crevices; too large → tunnel entrances
   rejected. *Mitigation*: tune against the q2dm1 tunnel using `watermap`; pin in a test.
5. **Cache regen cost.** Bumping the fingerprint invalidates all 8 cached maps.
   *Mitigation*: expected; `generate-map-cache --map 'q2dm*'` regenerates (~10s, Plan 18).

---

## Verification Checklist

- [ ] T1: `EdgeKind::Swim` + `current_edge_is_swim` compile across all Navigator impls; `cargo test` green.
- [ ] T2: synthetic + q2dm1 builds emit water nodes (assert count > 0 in the railgun column).
- [ ] T3: swim↔swim edges exist and bypass STEP (unit test on a vertical water shaft).
- [ ] T4: q2dm1 railgun room fuses into the main component (`components()` count drops; railgun node in largest comp).
- [ ] T5: cache round-trips swim edges + water tags; old-version cache auto-rejected.
- [ ] T6: `navinspect contents 240 -384 ...` reports water in the railgun column; `watermap` renders the pool.
- [ ] T7: **A* `path(spawn → [240,-384,464])` returns `Some`** on q2dm1 (was `None`); synthetic crossing test green.
- [ ] T8: distilled + pitfalls updated; `cargo clippy -- -D warnings` clean; plan moved to `completed/`, SERIES updated.
