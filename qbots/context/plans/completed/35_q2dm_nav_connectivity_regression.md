# Plan 35 — q2dm nav connectivity: hull-valid routes + residual map gaps

> **Status**: in-progress (revised 2026-07-09)
> **Created**: 2026-06-18
> **Depends on**: Plan 34 (diagnostics + tooling), Plan 42 (ride edges), Plan 43 (ride behavior)
> **Goal**: Every stock q2dm map is fully spawn-connected with routes a bot can *physically walk* (hull-valid edges), so far-spawn bots reliably reach ladder/train/lift-gated items (q2dm3 quad from any spawn; q2dm6 8/8; q2dm7 ≥ 5/6).
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: The original "regression" is root-caused and largely fixed — the missing pieces were
**connector mechanisms** (ladder edges, train/lift ride edges, jump-down bridges), not a
`walkable_stair` bug. What remains is **route quality**: some bridge/seed edges are not
traversable by the player hull (they pass a point trace but not a hull trace), and two maps
still have residual component gaps. This plan finishes the job.

**Deliverables**:
1. All bridge/seed edges hull-validated (player hull, full length) — over-long untraversable
   "walk" bridges are split into real hops or rejected.
2. q2dm3 upper level resampled/repaired so **far spawns route reliably to the quad board
   ledge** — live `spawn-to-item quaddamage --count 4` reaches ≥ 3/4 (was 0–1/4).
3. q2dm6 8/8 and q2dm7 ≥ 5/6 spawn connectivity (per-map diagnosis + fix).
4. Cache regen for all q2dm* (`mapcache::VERSION` bump) + live spot-checks.

**Estimated effort**: Large (iterative nav work).

## Context

### What was already resolved (2026-06-19, see git log P35)
- Root cause of the broad 5/8 failure was **missing connectors**, not the `walkable_stair`
  floor-probe (fragmentation persisted with `QBOTS_NO_PRUNE=1`). Fixed via:
  - **Ladder climb edges** (`CONTENTS_LADDER` → vertical `Ride` edges with `ladder: true`;
    `build.rs::add_ladder_edges`, `LADDER_RADIUS=96`, `LADDER_DZ=56`) — the key for q2dm3.
  - **func_train / func_plat ride edges** (Plan 42) + **jump-down floor bridges**
    (`bridge_components_via_jump`, `navgraph.rs:1197`).
- Current per-map state: q2dm1/2/4/5/8 **full**; **q2dm3 7/7**; q2dm6 **7/8**; q2dm7 **4/6**
  (was 3/6). The q2dm3 quad is A*-reachable from all 7 spawns and physically reached from
  spawn3 (Plan 43).

### The remaining problem (recorded 2026-06-19, brain_notes.md)
Far-spawn → quad-board routes on q2dm3 traverse the fragmented upper level over **over-long
bridge "Walk" edges that are hull-BLOCKED** — e.g. `(-121,-161,216) → (191,-329,216)` is a
354u `Walk` edge whose **hull trace stops at fraction 0.07** while the point trace is clear.
The bot cannot physically follow the A* path, so `--count 4` (bots spread across far spawns)
reaches 0–1/4 even though the ride itself is solid.

> **Deferral superseded**: the 2026-06-19 user decision deferred far-spawn route reliability.
> The 2026-07-09 user directive (human-like bots that successfully navigate maps and get
> items from anywhere) **re-opens it as this plan's core scope**.

### Key code anchors
- `crates/world/src/navgraph.rs` — `bridge_*` fns, `walkable_stair` (:1794),
  `floor_waypoints_multi` (:1547), `bridge_components_via_jump` (:1197), `STEP=18` (:22).
- `crates/world/src/build.rs` — generation pipeline, `add_ladder_edges` (:521),
  `add_train_edges`, `add_elevator_edges`, `JUMP_BRIDGE_*` (:268).
- `crates/world/src/mapcache.rs` — `VERSION` (currently 18) + `Fingerprint`.
- Tools: `navinspect <map> compgaps|navquery|gpath`, `spawn-to-point <x> <y> <z>` scenario
  (isolates route-reliability from ride-correctness), `QBOTS_NO_PRUNE=1`.

## Step-by-Step Tasks

### T1: Hull-validate every bridge/seed edge

**File**: `crates/world/src/navgraph.rs` (+ `build.rs` call sites)

**What to do**: Any edge synthesized by a bridge pass (`bridge_components*`, spawn seeding,
stair links) must be validated with a **player-hull trace over its full length** (with STEP
stepping, like `walkable_stair`), not a point trace. Reject edges the hull can't traverse.
Add a regression unit test around the known-bad q2dm3 edge shape (long flat hull-blocked
span). Log every rejected bridge with both fractions (point vs hull) for diagnosis.

### T2: Split over-long bridges into real hops / resample the q2dm3 upper level

**Files**: `crates/world/src/navgraph.rs`, `crates/world/src/build.rs`

**What to do**: Where T1 rejection disconnects a previously-"connected" area (q2dm3 upper
level, comp0 z152–600), restore *genuine* connectivity:
- Split long bridge candidates at intermediate walkable floor points (probe the midpoint
  columns with `floor_waypoints_multi`-style sampling; insert nodes; connect hull-valid hops).
- Densify sampling on floors that currently have sparse/no nodes (the q2dm3 z=83..168 tread
  band; upper-level islands around the ladder exits and the `*10` board ledge).
Success metric: A* far-spawn → quad-board path exists **and every edge is hull-valid**.

### T3: q2dm6 + q2dm7 residual gaps

**What to do**: Per-map diagnosis with `navinspect compgaps` (q2dm6 is 7/8, q2dm7 is 4/6).
Identify each missing spawn's blocking geometry (ladder? lift? jump? tread band?) and apply
the matching existing mechanism (ladder/ride/jump-bridge/resample) — do **not** invent new
generic bridges without hull validation (T1). q2dm7's accepted pit means ≥ 5/6 passes.

### T4: Regen + live verification

**Files**: `crates/world/src/mapcache.rs` (VERSION bump), live runs

**What to do**: Bump `mapcache::VERSION`; `generate-map-cache --map 'q2dm*' --spacing 24`
must cache all 8 maps (q2dm7 ≥ 5/6 with `--allow-failures` documented). Live checks:
- q2dm3: `spawn-to-item quaddamage --count 4 --max-secs 150 --lift-penalty 0` → ≥ 3/4 reached.
- q2dm6/q2dm7: `spawn-to-spawn --count 8 --max-secs 90` reaches from the previously-orphaned
  spawns.
Record results in `context/map_errors.notes.log.md` (dated) and append `context/brain_notes.md`.

> **Rule B reminder**: commit after *each* task; fmt + clippy(-D warnings) + tests green
> before every commit; `mapcache::VERSION` bumped in the same commit as any generation change.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/world/src/navgraph.rs` | hull-valid bridge gate, bridge splitting, resample helpers | P0 |
| `crates/world/src/build.rs` | pipeline wiring, per-map fixes | P0 |
| `crates/world/src/mapcache.rs` | `VERSION` bump | P0 |
| `crates/world/tests/` | hull-blocked-bridge regression test | P1 |
| `context/map_errors.notes.log.md` | dated per-map results | P2 |

## Open Questions / Risks

1. **Hull validation may disconnect maps that currently "pass"** (their bridges were fake).
   *Mitigation*: T2 restores genuine hops; run the full q2dm* gate after T1 and treat new
   failures as newly-honest, not regressions — fix forward.
2. **Splitting bridges can explode node count / gen time.** *Mitigation*: split only bridge
   candidates that fail the hull gate; cap inserted nodes per bridge; measure gen time
   (Plan 18 cache keeps runtime cost at zero).
3. **Connectivity passing ≠ navigable** (the recurring pitfall). *Mitigation*: T4's live runs
   are the gate; `spawn-to-point` isolates route bugs from ride bugs.
4. **q2dm7's pit** is genuinely one-way. Accepted: ≥ 5/6.

## Verification Checklist

- [ ] T1: bridge/seed edges hull-validated; regression test pins the q2dm3 hull-blocked case; commit.
- [ ] T2: far-spawn → quad-board A* path is fully hull-valid; q2dm3 upper level has nodes in the
      previously-empty bands; commit.
- [ ] T3: q2dm6 8/8; q2dm7 ≥ 5/6; per-map cause documented; commit.
- [ ] T4: `generate-map-cache --map 'q2dm*'` caches 8/8 (q2dm7 note); live q2dm3 quad ≥ 3/4 from
      mixed spawns; `map_errors.notes.log.md` + `brain_notes.md` appended; VERSION bumped; commit.
- [ ] fmt + clippy(-D warnings) + tests green before each commit.
