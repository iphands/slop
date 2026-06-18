# Nav-graph "bridges": what they are, why they exist, and should they?

> Written to answer a direct question: *bridges seem strange — are they actually
> needed, or are they a workaround that is causing new bugs?* Short answer: bridges
> are a **fragmentation-repair workaround**, not a fundamental nav primitive. They ARE
> genuinely necessary (the gridscan experiment below proves the fragmentation is
> structural, not a coarse-grid artifact) — but the way they are tuned (`BRIDGE_HDIST`
> + `walkable_stair`) has demonstrably written new bugs. The fix is bridge *accuracy*,
> not bridge removal or a finer grid. This doc lays out the facts so we can decide
> deliberately.
>
> **UPDATE 2026-06-17: the "finer grid removes bridges" hypothesis was RUN and REFUTED.
> See the gridscan results table below.**

## What a bridge actually is (mechanism)

`crates/world/src/navgraph.rs :: bridge_components → bridge_pass`.

- `NavGraph::generate(cm, bounds, GRID_SPACING=24)` samples walkable floor on a 24u
  grid and connects **only 8-neighbors** (±1 grid cell ⇒ ~24–34u edges), each
  validated by a hull trace (flat/small-dz) or `walkable_stair` (dz in `(STEP, STAIR_MAX]`).
- That sampling produces a **fragmented** graph: several disconnected components.
- `bridge_pass` computes the components, then for every pair of nodes that are
  **in different components** and within `BRIDGE_HDIST` horizontally and `STAIR_MAX`
  vertically, tries `walkable_stair_link_orig`; if it passes, it adds an edge.
- Crucial property: **bridges only ever connect different components.** If `generate()`
  produced a single component, `bridge_pass` returns 0 — zero bridges. So bridges exist
  *purely* to stitch fragments back together.

Pipeline order (`crates/world/src/build.rs`): `generate → seed_spawns →
add_elevator_edges → bridge_components(BRIDGE_HDIST) → prune_long_blocked_edges(PRUNE_MAX_HD)
→ detect_jump_edges`.

## Why fragmentation happens (the cases bridges paper over)

All q2dm* spawns are mutually reachable by design, yet `generate()` at 24u splits the
floor into islands. Observed causes (from `map_errors.notes.log.md`):

1. **Stairs/winding staircases.** A flight's endpoints can land in grid columns 160–200u
   apart (the flight winds), so no 8-neighbor pair bridges them. `STAIR_MAX` had to grow
   24→128→**160** just so the stair trace would even be *attempted* across such pairs
   (q2dm3 went FAIL→PASS at 160). This is the single biggest driver.
2. **Doorways / narrow gaps.** At 24u the grid may not place a node on *both* sides of a
   thin passage close enough to be 8-neighbors, so the rooms split.
3. **Elevators / lifts** (`func_plat`, vertical `func_door`). Handled separately by
   `add_elevator_edges`, but residual gaps remain (q2dm3 lift shaft, q2dm7).
4. **One-way pits.** q2dm7 spawn[0] sits in a z=-296 pit reachable only by falling
   (drop 312 > STAIR_MAX=160, no return path). *Genuinely* unbridgeable — accepted
   limitation, not a bug. Some fragmentation is legitimately irreducible.

So: most fragmentation is **stairs + under-sampled doorways**, i.e. an artifact of grid
coarseness and imperfect stair connectivity — not real disconnection.

## The cost: bridges are writing new bugs

`BRIDGE_HDIST` has crept 128 → 256 → 512 over time to span ever-wider stair gaps. Each
bump lets `bridge_pass` consider far-apart pairs, and `walkable_stair` **false-positives**
on long spans: its step-by-step probe passes whenever the sampled points happen to land
on *some* surface, even across open space / through walls.

Concrete (q2dm1, via `tools navinspect`): **node 10300 (1567,543,568)** became a *false
hub* with 19 edges, ~16 of them live-hull-`BLOCKED`. The big ones (dz=±80–96, hd=187–255,
slope 0.32–0.38 — real q2dm1 stairs are slope ~1.0, hd ≲ 136) all report `stair=OK`.
A\* sees these cheap-looking edges, routes bots through 10300, and they cannot physically
traverse → thrash. This was THE cause of clustered `goal=(-80,800,472)` (spawn[3])
failures (6 of 7 in one 32-bot run).

Mitigation already added: `prune_long_blocked_edges(PRUNE_MAX_HD=144)` drops hull-blocked
edges longer than a real stair flight. **But that is treating the symptom** — the false
edges should never have been created. (Note: `BRIDGE_HDIST` in `build.rs` is currently
256; the prune cap is the real guardrail.)

## Should bridges exist? The honest assessment

- **Necessary today:** yes. Without bridging, 7/10 q2dm1 spawns sit in islands and the
  "all spawns reachable" invariant fails. We cannot just delete `bridge_components`.
- **Necessary in principle:** probably not, except for irreducible cases (one-way pits).
  Bridges compensate for coarse sampling. If `generate()` connected stairs and doorways
  reliably, the graph would be (near-)single-component and bridges would need only a tiny,
  safe radius — or vanish.
- **Net-harmful as tuned:** the large `BRIDGE_HDIST` + permissive `walkable_stair` is a
  bug factory. Every widening to fix one map's gap risks new false hubs on another.

## The grid-size hypothesis (and the decisive experiment)

Hypothesis: **a finer grid (24 → 16 → 12 → 8) drops the pre-bridge component count toward
1**, because nodes land densely enough on both sides of doorways and along stair treads to
be 8-neighbors. If so, we can lower `BRIDGE_HDIST` to a small safe value (or remove
bridging) and the node-10300-style false hubs disappear at the source. Caching
(`data/mapcache/*.qnav`) makes the one-time generation cost acceptable as long as it is
not enormous — worth measuring, not assuming.

Known data point (grid=24, q2dm1, pre-bridge): **5 components; only 3/10 spawns in the
largest.** That is the baseline to beat.

Decisive test — `crates/tools/src/bin/gridscan.rs` (added): builds **generate-only** (no
seed/elevator/bridge/prune) at several spacings and reports nodes, component count,
largest-component size, and spawns-in-largest. Run:

```
cargo run -p tools --bin gridscan -- vendor/baseq2 q2dm1 24 16 12 8
```

Interpretation:
- If components → 1 (or spawns-in-largest → 10) as spacing shrinks: bridges were masking
  a coarse grid. Action: adopt the smallest spacing with acceptable gen time, then lower
  `BRIDGE_HDIST` toward ≤128 and re-verify all q2dm* connectivity + the 32-bot scenarios.
- If components stay high even at 8u: fragmentation is structural (true stair/lift gaps),
  bridges are genuinely needed, and the right fix is better stair/elevator edge generation,
  not grid density.

### gridscan RESULTS (q2dm1, 2026-06-17 — RUN, hypothesis REFUTED)

| map | spacing | nodes | components | largest | spawns/total | gen_ms |
|-----|--------:|------:|-----------:|--------:|:------------:|-------:|
| q2dm1 | 24 | 12886 | 66 | 6097 | **2/10** | 79 |
| q2dm1 | 16 | 29059 | 138 | 13837 | **2/10** | 162 |
| q2dm1 | 12 | 51761 | 140 | 24570 | **2/10** | 288 |
| q2dm1 |  8 | 116127 | 138 | 55348 | **2/10** | 648 |

Spawns-in-largest stays pinned at **2/10** at every spacing; component count does NOT fall
toward 1 — it RISES (66→138). The second interpretation branch is the truth: fragmentation
is **structural**. And finer is worse for stairs: `generate()` links only 8-neighbours
(±1 cell); a stair tread is ~16–24u deep, so at spacing 8 one step spans 2–3 cells and
gets no direct edge → stairs fragment more, not less.

## Bottom line / recommendation (updated with data)

1. **Bridges are genuinely needed.** gridscan proves the islands are structural at every
   grid density. Keep them. The user's instinct that bridges are "strange" correctly
   points at their *radius + walkable_stair accuracy*, not their existence.
2. The real lever is bridge **accuracy**, not grid density or bridge removal:
   - `prune_long_blocked_edges` (DONE, connectivity-preserving): node 10300 19→8 edges,
     all spawns still connected. Symptom guardrail.
   - Root fix candidate: connect a **wider neighbourhood (±2–3 cells)** in `generate()`
     so real stair steps get direct edges — shrinks the fragmentation bridges must repair,
     letting `BRIDGE_HDIST` drop. NEXT EXPERIMENT.
   - Or lower `BRIDGE_HDIST` (256→~160) now that the prune exists. gridscan + 32-bot
     scenarios decide.
3. Accept irreducible cases (one-way pits, lift-only routes) — not bugs.

> NOTE (status at time of writing): `gridscan` is committed but the experiment was **not
> run** here — the `world` crate was mid-refactor by concurrent work and did not compile
> (`UnionFind` undefined in `navgraph.rs`). Run `gridscan` once the tree builds and record
> the table below.

### gridscan results (fill in)

| map | spacing | nodes | components | largest | spawns/total | gen_ms |
|-----|--------:|------:|-----------:|--------:|:------------:|-------:|
| q2dm1 | 24 | | | | | |
| q2dm1 | 16 | | | | | |
| q2dm1 | 12 | | | | | |
| q2dm1 |  8 | | | | | |

## Bottom line / recommendation

1. Keep bridges for now (invariant depends on them) but treat the large radius as debt.
2. Run `gridscan` to get real numbers; this directly decides the grid question.
3. If a finer grid collapses fragmentation, **shrink `BRIDGE_HDIST` and lean on grid
   density** — that removes the false-hub bug class at the root rather than pruning it
   after the fact.
4. Accept that a few cases (one-way pits) are irreducibly unbridgeable and not bugs.

See also: `map_errors.notes.log.md` (Finding 3 — false bridge hub node 10300; STAIR_MAX and
BRIDGE_HDIST evolution).
