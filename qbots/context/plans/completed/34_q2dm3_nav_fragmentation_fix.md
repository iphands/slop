# Plan 34 — q2dm3 nav fragmentation: diagnostics + resilient cache batch

> **Status**: done
> **Created**: 2026-06-18
> **Depends on**: Plan 17, Plan 18
> **Goal**: Unblock `generate-map-cache --map 'q2dm*'` (cache all good maps, report the rest) and diagnose why q2dm3 fragments — the deep nav fix is deferred to Plan 35.
> **Agent**: main session

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## OUTCOME (2026-06-18) — rescoped mid-flight

Diagnosis (T3) overturned the "single surgical lift fix" premise. Restoring
`BRIDGE_HDIST=512` does **not** restore connectivity, and the batch revealed the regression
is **broad: 5 of 8 stock DM maps fail** (q2dm2 3/7, q2dm3 3/7, q2dm5 7/9, q2dm6 7/8,
q2dm7 3/6; only q2dm1/4/8 pass). The 2026-06-16 notes had 7/8 passing — so this is a code
regression (likely `walkable_stair`'s floor-existence check `662580e69` + the `BRIDGE_HDIST`
512→256 cut), not a q2dm3 quirk.

**Per user decision: this plan delivers diagnostics (T1–T3) + the resilient batch (T6) to
unblock now.** The nav-graph fix (originally T4/T5/T7) is **deferred to Plan 35**
(broad q2dm connectivity regression). Done tasks: **T1, T2, T3, T6**.

---

## TL;DR

**What**: q2dm3's nav graph fragments to 3/7 spawns, so the connectivity gate fails the
map and sinks the whole `q2dm*` cache batch. Fix the root causes surgically.

**Deliverables**:
1. Diagnostic tools usable on a *failing* map (navinspect live-build; compgaps flat-gap fix).
2. Lift top-node anchoring so a `func_plat`/`func_door` upper landing joins the graph.
3. (If needed) a spawn-aware wider bridge that connects spawn components only — global
   `BRIDGE_HDIST` stays 256.
4. `generate-map-cache` batch caches every good map + reports per-map PASS/FAIL.
5. q2dm3 = 7/7, all q2dm* cache, no regression on q2dm1/2/4/5/6/8 (q2dm7 stays 5/6).

**Estimated effort**: Medium (1 day)

## Context

`just mapcache 'q2dm*'` → `qbots generate-map-cache` fails `ok=0 err=1` on **q2dm3**:
`check_spawn_connectivity` finds 27 fragments, only **3/7 spawns** in the largest component,
so the map is rejected and the batch exits non-zero. This is a **regression + a latent lift
bug**, not an inaccessible map (finer grids fragment *worse*: 27→43→45).

### Key Facts (diagnosed 2026-06-18)
- Notes `context/map_errors.notes.log.md:115-123` record q2dm3 **PASS 7/7** on 2026-06-16 with
  `BRIDGE_HDIST = 512`. Git shows it was later cut 512→128→**256** (`build.rs:28`, commit
  `1c0756a85`, a steering/smoothing change — not a connectivity decision).
- The 512 bridge masked a **lift bug**: `floor_waypoints_multi` (`navgraph.rs:1176`) traces
  down and falls **through** inline-model (func_plat/func_door) platform surfaces → **no node
  at the platform's upper landing** → `add_lift`'s `top_node` (`build.rs:227`) has nothing to
  `connect_node_to_nearby` → upper area (incl. Quad/Railgun) only joined via a >256u walk
  bridge. (notes:71-82: `C1:(-608,512,83) ↔ C2:(-601,511,152) hull=BLOCKED stair=FAIL`.)

### Decided scope
Keep `BRIDGE_HDIST = 256` global (no global false bridges). Lift work = **nav-graph
connectivity only**. Actual lift *riding* (brain FSM, removing `ELEVATOR_PENALTY`) is
**deferred to Plan 31** (`context/elevator_todo.md`).

### Tooling blockers (must fix first)
- **navinspect is load-only** (`cached_map_nav`, `navinspect.rs:42`) → can't open q2dm3 (no
  cache, because generation fails) → the richest diagnostic is unusable on the target map.
- **compgaps over-reports flat gaps**: for `dz ≤ STEP` it falls back to `walkable_stair` when
  the hull trace is blocked, and `walkable_stair(total_dz≈0)` does **0 traces** → returns
  `true` (`navgraph.rs:1321`, `steps=ceil(0/STEP)=0`). Wide-radius "missed links" are noise.

## Step-by-Step Tasks

### T1: navinspect live-build fallback
**File**: `crates/tools/src/bin/navinspect.rs`
**What to do**: When `QBOTS_LIVE=1` (or a `--live` arg), build via `world::generate_map_nav`
instead of `cached_map_nav`, so a map that fails the cache gate can still be inspected.
Default path unchanged.

### T2: fix compgaps flat-gap walkability
**File**: `crates/tools/src/bin/compgaps.rs`
**What to do**: For `dz ≤ STEP`, decide walkability from the direct hull trace only; only use
`walkable_stair` when `dz > STEP`. (Optionally also guard `walkable_stair` itself for
`total_dz < STEP` — only if it leaves bridge/prune results unchanged.)

### T3: diagnose q2dm3 (no code, record findings)
**File**: `context/map_errors.notes.log.md`
**What to do**: Build q2dm3 live; map the 7 spawns to post-pipeline components; list stranded
spawns and classify each connector — lift-anchor miss / stair-walk 256–512 / genuine. Confirm
whether Quad/Railgun nodes exist. Update the q2dm3 OPEN section with results.

### T4: anchor lift top nodes
**Files**: `crates/world/src/build.rs` (`add_lift`), `crates/world/src/navgraph.rs` helper,
`crates/world/src/mapcache.rs` (`VERSION` bump).
**What to do**: After creating a lift `top_node`, probe world-floor at XY positions just
outside the shaft footprint at the upper level, synthesize anchor node(s) in the upper
component, and connect the lift top node to them. Bump cache `VERSION` (geometry changed).

### T5: spawn-aware escalating bridge (only if T4 leaves spawns split)
**Files**: `crates/world/src/navgraph.rs` (`bridge_spawn_components`),
`crates/world/src/build.rs` (`generate_map_nav`), `crates/world/src/mapcache.rs`
(`Fingerprint` + `VERSION` if a new constant).
**What to do**: After `bridge_components(256)`, if spawns remain split, run extra passes at a
larger radius (≤512) restricted to spawn-bearing components, via strict
`walkable_stair_link_orig`. Add any new radius constant to the fingerprint + VERSION bump.

### T6: resilient generate-map-cache batch
**File**: `crates/qbots/src/main.rs` (`generate_map_cache`)
**What to do**: Print a per-map PASS/FAIL summary + failed list; cache all good maps; exit 0
when all *requested* maps cached (or add `--allow-failures`). Single-map + preflight stay
fatal.

### T7: regenerate + verify
**What to do**: `just mapcache` → `err=0`; q2dm3 7/7; q2dm1/2/4/5/6/8 PASS; q2dm7 5/6; live
`spawn-to-spawn`/`spawn-to-weapon` on q2dm3 traverses the new connection (false-bridge guard).
`data/mapcache/` is gitignored — do not commit caches.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/world/src/build.rs` | lift anchoring in `add_lift`; bridge wiring | P0 |
| `crates/world/src/navgraph.rs` | floor-probe-outside-shaft helper; spawn bridge | P0 |
| `crates/world/src/mapcache.rs` | `VERSION`/`Fingerprint` bump on gen change | P0 |
| `crates/tools/src/bin/navinspect.rs` | live-build fallback | P1 |
| `crates/tools/src/bin/compgaps.rs` | flat-gap walkability fix | P1 |
| `crates/qbots/src/main.rs` | batch reporting + exit semantics | P1 |
| `context/map_errors.notes.log.md` | record regression + fix | P2 |

## Open Questions / Risks

1. **False bridges** — keep `BRIDGE_HDIST=256` global; widen only between spawn components,
   via strict `walkable_stair_link_orig`. *Mitigation*: validate with live `spawn-to-*`, not
   just the connectivity gate.
2. **Cache fingerprint drift** — any generation change without a `VERSION`/fingerprint bump
   lets stale caches win silently. *Mitigation*: bump on every gen-affecting task.
3. **Lift anchor heuristic** — "just outside the shaft" must land on real upper floor, not a
   different level. *Mitigation*: trace-verify the anchor and its link; reject on fail.
4. **Don't relax the gate** — resilience lives only in multi-map batch reporting; single-map
   and preflight stay fatal.

## Verification Checklist

- [ ] T1: navinspect opens q2dm3 live (no cache) with `QBOTS_LIVE=1`.
- [ ] T2: compgaps "walkable" count reflects real hull-clear pairs (flat artifact gone).
- [ ] T3: q2dm3 stranded spawns + connectors documented in notes.
- [ ] T4: lift upper landing has a node joined to the upper component (navinspect).
- [ ] T5: q2dm3 full-pipeline connectivity = 7/7 spawns.
- [ ] T6: `generate-map-cache --map 'q2dm*'` caches all good maps + prints PASS/FAIL; exit 0.
- [ ] T7: q2dm1/2/4/5/6/8 PASS, q2dm7 5/6; live `spawn-to-spawn` on q2dm3 traverses the link.
- [ ] All tasks: fmt + clippy(-D warnings) + tests green before each commit.
