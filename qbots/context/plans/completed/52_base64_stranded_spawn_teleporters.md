# Plan 52 — base64 Stranded Spawn: Rescue Jump Pass + Teleporter Edges

> **Status**: done (closed 2026-07-11)
> **Created**: 2026-07-11
> **Depends on**: Plan 42 (jump-down component bridging), Plan 35 (traversal executor), Plan 51 (zb2 watchdogs)
> **Goal**: `generate-map-cache --map base64` passes 46/46 spawns via a spawn-only rescue jump pass, and bots can traverse `misc_teleporter`/`trigger_teleport` pads as nav edges.
> **Agent**: implementation agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Fix the base64 45/46 connectivity failure with a targeted spawn-rescue jump-down pass (deeper fall cap, only for stranded spawn-bearing components), and add first-class teleporter edges to the nav graph + brain traversal.

**Deliverables**:
1. `NavGraph::rescue_stranded_spawns` — jump-down bridging restricted to (stranded spawn component ↔ play component) pairs, `RESCUE_MAX_FALL = 384`.
2. `add_teleporter_edges` build pass + `EdgeKind::Teleport` (one-way pad → dest), cache-encoded.
3. Brain traversal of Teleport legs without tripping the P51 stall watchdogs.
4. Regenerated caches: base64 = 46/46; q2dm* sweep unchanged.

**Estimated effort**: Medium (1 day)

---

## Context

### Pre-Identified Bug/Issue

`generate-map-cache` on `base64` reports `spawns not all reachable (45/46 in largest component)`. Diagnosis (verified with `qbots nav-debug base64` + BSP point-contents cross-sections, 2026-07-11):

- The stranded spawn is **spawn[26] @ (-720, 824, -520)** — the grenade-launcher/water room. Its nearest node (15849) is in component 2 (259 nodes, x[-761..-425] y[791..1247] z[-599..-512]). Component 1 (287 nodes, same XY, z[-416..-168]) is the window/ledge layer above; both are cut off from play component 0 (44 802 nodes).
- The room's real exit is a **drop shaft in the floor at x≈-750..-650, y≈1000**, falling to the west-pit floor at **z≈-808** (play component — spawns 34/42/43 live there). Drop height **~288u**.
- `bridge_components_via_jump` (`navgraph.rs`, Plan 42) would bridge exactly this, but `JUMP_BRIDGE_MAX_FALL = 256` (`build.rs`) is 32u too small. Example valid candidate: hi=(-761,1000,-520) → lo=(-673,1000,-808), hd=88 ≤ `JUMP_BRIDGE_HDIST`(104), launch/fall traces clear.
- The map is NOT broken; we lack the edge. (Consistent with the AGENTS.md "all spawns are reachable" doctrine, extended to this custom map.)

### Why a rescue pass instead of raising the cap

Raising `JUMP_BRIDGE_MAX_FALL` globally to ≥288 would add deeper (minor-fall-damage) drop edges on every map and churn all q2dm* graphs. The rescue pass keeps 256 as the universal cap and only fires when spawn-bearing components remain stranded after normal bridging — a no-op on all q2dm* maps.

### Teleporters (in scope by decision)

The nav graph has zero teleporter handling. base64's single teleporter (pad (-2160, 904, -856) → dest (-3216, -1744, -280), targetname t163) links two areas that are already both in the play component, so it is not the gate culprit — but bots cannot use it as a route, and other custom maps may gate spawns behind teleporters.

### Key Facts

- Connectivity gate: `spawns_in_largest_component` (`navgraph.rs`) over **undirected** `components()` — one-way drop edges count as connectivity by design, and A* routes the drop (directed).
- `jump_down_link` (`navgraph.rs`) validates launch-over + fall-down traces at standing and hop height; reused verbatim by the rescue pass.
- Jump/bridge constants are cache-invalidated **via `mapcache::VERSION`**, not the fingerprint — the new EdgeKind forces a bump anyway.

---

## Step-by-Step Tasks

### T1: Spawn-rescue jump pass

**File**: `crates/world/src/navgraph.rs`, `crates/world/src/build.rs`

**What to do**: Add `NavGraph::rescue_stranded_spawns(&mut self, cm, max_hdist, max_fall, spawns) -> usize`:
- Compute `largest_spawn_component(spawns)` (play component) and the components holding each spawn's `nearest()` node; collect stranded spawn components (non-play).
- Bucketed candidate search as in `bridge_components_via_jump`, restricted to pairs (stranded-comp node, play-comp node) in either vertical order; validate with `jump_down_link(nodes, cm, &zero, max_fall, hi, lo)`; apply shortest-drop-first with union-find dedupe.
- `build.rs`: `pub const RESCUE_MAX_FALL: f32 = 384.0;` (documented: survivable fall, covers base64's 288u shaft; fires only for stranded spawn components). Call after `bridge_components_via_jump` in `generate_map_nav`, gated on `spawns_in_largest_component` reporting stranded spawns; log the added count.
- Unit tests: synthetic two-floor box with a >256u, ≤384u drop — with a spawn on the stranded floor it bridges; without a spawn it stays split.
- **Commit `task(P52-T1): …` (build/clippy/fmt/tests clean first).**

### T2: Teleporter edges

**File**: `crates/world/src/build.rs`, `crates/world/src/navgraph.rs`

**What to do**: `add_teleporter_edges(graph, cm, bsp) -> usize` called in `generate_map_nav` alongside `add_ladder_edges`:
- Point pads: `find_class("misc_teleporter")`, `target` → `misc_teleporter_dest` by `targetname`.
- Brush pads: `trigger_teleport` (center via `entity_model` bounds) → `misc_teleporter_dest` or `info_teleport_destination`.
- Snap pad + dest to ground, add/find nodes, wire to nearby walkable nodes (plat-pass pattern), add **one-way** `EdgeKind::Teleport` pad → dest with a small fixed cost.
- Unit test: entity-lump fixture → one directed Teleport edge.
- **Commit `task(P52-T2): …`.**

### T3: Brain traversal of Teleport legs

**File**: `crates/brain/src/traverse.rs`, `crates/brain/src/ride.rs`, `crates/brain/src/brains/zb2.rs`

**What to do**: Walk toward the pad node like a Walk leg; the server teleports on touch; leg completes on proximity to the **dest** node. Ensure the position snap resyncs the route cursor (nearest-node) instead of tripping the P51 waypoint-progress watchdog / stuck detectors; add a Teleport-leg guard if the existing resync doesn't cover it.
- **Commit `task(P52-T3): …`.**

### T4: Cache format + regeneration

**File**: `crates/world/src/mapcache.rs`

**What to do**: Bump `VERSION` (new EdgeKind + build passes), encode/decode `Teleport`. Regenerate `base64` (expect 46/46, exit ok) and the q2dm* sweep (all pass; jump_bridged/node/edge counts unchanged → rescue pass is a no-op there). Side check: why today's failed run left a `data/mapcache/24/base64.qnav` that `mapcache::load` rejects; ensure a gate-FAILED generate doesn't leave a half-trusted cache.
- **Commit `task(P52-T4): …`.**

### T5: End-to-end verification

**What to do**: Run the Verification Checklist below (nav-debug, cache sweep, tests; live `spawn-to-point` runs if a server on base64 is available via qctrl).
- **Commit `task(P52-T5): …` if any changes.**

### T6: Knowledge capture + close

**What to do**: `context/distilled.md` (base64 geometry + nav-debug→cross-section diagnosis method), `context/pitfalls.md` (jump-bridge fall cap vs custom maps; gate message doesn't name the spawn — nav-debug does), SERIES.md → done, `git mv` plan+tracker to `completed/`.
- **Commit `task(P52-T6): …`.**

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/world/src/navgraph.rs` | `rescue_stranded_spawns`, `EdgeKind::Teleport`, tests | P0 |
| `crates/world/src/build.rs` | `RESCUE_MAX_FALL`, rescue call site, `add_teleporter_edges` | P0 |
| `crates/world/src/mapcache.rs` | VERSION bump, Teleport encoding | P0 |
| `crates/brain/src/traverse.rs`, `ride.rs`, `brains/zb2.rs` | Teleport leg + snap resync | P1 |
| `context/plans/SERIES.md`, `distilled.md`, `pitfalls.md` | Process + knowledge | P1 |

---

## Open Questions / Risks

1. **Rescue candidate node placement** — the (-761,1000,-520)→(-673,1000,-808) pair came from cross-sections; if no candidate validates at hdist 104, escalate rescue hdist to 128 (rescue-only) and re-check with nav-debug.
2. **Teleport snap vs watchdogs** — the position discontinuity must not read as a stall/slide; verify against the P51 watchdog before closing T3.
3. **Fall damage on 288–384u drops** — minor and rescue-only; no other map gains such edges.

---

## Verification Checklist

- [ ] T1: `cargo test -p world` green incl. rescue tests; `qbots nav-debug base64` → `spawn[26] … comp=0 [ok]`, `in_largest=46`
- [ ] T2: teleporter unit test green; base64 build logs 1 teleporter edge (pit → t163 dest)
- [ ] T3: bot crosses the base64 pit teleporter in a live `spawn-to-point` run without watchdog recovery triggering (if server available)
- [ ] T4: `generate-map-cache --map base64` ok=1 err=0 (46/46); q2dm1–q2dm8 sweep still passes with unchanged counts
- [ ] T5: `cargo build` + `cargo clippy` zero warnings, `cargo fmt` applied, all tests green
- [ ] T6: distilled.md + pitfalls.md entries on disk; SERIES.md updated; plan+tracker moved to `completed/`
