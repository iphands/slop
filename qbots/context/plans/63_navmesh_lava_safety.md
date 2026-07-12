# Plan 63 — Navmesh lava safety (q2dm6 lava suicides → near zero)

> **Status**: pending
> **Created**: 2026-07-12
> **Depends on**: Plan 48 (A* lava fixes), Plan 50 (jump-landing fixes), Plan 20 (navmesh/hybrids)
> **Goal**: Eliminate lava/slime suicides on navmesh-backed navmodes (`nm`, `sg`, and the other hybrids) by porting the Plan 48/50 deadly-floor validation into the heightfield/navmesh builder and closing the driver-level fallback gaps — measured live on q2dm6 with the new `EVT env_suicide` counters.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: The navmesh backend never learned what the A* graph learned in Plans 48/50 — lava is not solid, so `MASK_SOLID` probes see the lava *bed* as floor. Port the deadly-floor checks into span acceptance and drop-link validation, guard the driver fallbacks, and prove it live on q2dm6.

**Deliverables**:
1. Heightfield span acceptance rejects shallow-lava floors (`floor_is_deadly` port) — pak-gated red→green regression test on q2dm6.
2. `find_drops`/`add_drops` validate drop landings + overshoot strips against lava (`landing_strip_deadly` semantics).
3. `NavmeshDriver` fallback steer target validated (no unvalidated funnel-vertex aim).
4. xon keyboard-emu stale-key hazard veto (the one xon-specific ungated mover).
5. Live q2dm6 A/B: baseline vs post-fix `competition --brains q3,xon --navmodes nm,sg` soak; acceptance gate **lava+slime env_suicides ≤ 2 per bot per 5 min and < 5% of deaths** (near zero; Plan 50's residual walk-off tail is the only accepted source).

**Estimated effort**: Medium (1 day)

## Context

### Why bots die in lava on q2dm6 with `nm`/`sg` (audited 2026-07-12)

The user's run (`competition --brains q3,xon --count 2 --navmodes nm,sg --chars … --xonchars …`) uses **navmesh-backed navmodes only**: `nm` routes exclusively through the navmesh; `sg` (hybrid-segment) is navmesh-primary and uses A* only for jump/swim link segments (`crates/brain/src/hybrid/segment.rs:38,91-111`). All the Plans 48/50 lava work landed in the **A\* graph builder** (`navgraph.rs`, cache v21–v23) and in brain-side hazard gates. The navmesh builder got none of it.

### Pre-identified bugs (confirmed by code audit, file:line)

- **B1 — shallow lava spans are walkable.** `column_floors` (`crates/world/src/navmesh/heightfield.rs:258`) is the *only* liquid check in the whole navmesh build: one `point_contents` sample at `floor_z + 24`. `MASK_SOLID` (`collision.rs:22`) excludes lava/slime, so the down-trace passes through the pool and lands on the solid bed → for pools **≤ 24u deep** the sample point sits in air above the surface and the span is accepted as walkable floor. This is exactly the "dry node hovering over lava" bug the A* sampler fixed with `floor_is_deadly` (`navgraph.rs:2067-2070`, used at `navgraph.rs:1833`, Plan 48 L1 / cache v21) — never ported.
- **B2 — drop links land in lava.** `find_drops` (`heightfield.rs:158-193`) validates the fall with a `MASK_SOLID` trace only (line 181); `add_drops` (`polymesh.rs:344-360`) wires nearest-poly with zero liquid checks. The A* equivalents call `landing_strip_deadly` (`navgraph.rs:2078-2092`, wired at `1660` and `2292`, Plan 50 E3 / v23) which also rejects the 0–48u momentum-overshoot strip.
- **B3 — driver fallback aims at unvalidated vertices.** `NavmeshDriver::pursue_target_safe` (`crates/brain/src/navmesh_driver.rs:130-142`) validates the look-ahead line with lava-aware `segment_has_floor`, **but on failure falls back to the raw funnel vertex `path[nxt]` with no check** (lines 138-141). Raw `pursue_target` (:126-128) validates nothing. With B1/B2 in the mesh, the fallback aims straight at lava polys.
- **B4 — xon stale keyboard keys.** xon's `KeyboardEmu::quantize` runs *after* every hazard check and holds keys across ticks at a skill-gated re-key cadence (`xon/mod.rs:530-543`, `xoncore/keyboard.rs:184-203`) — a held forward key can point into lava on a later tick with no re-probe. Unique to xon; q3 has no equivalent.

### Key facts

- Runtime steering safety is **by design** dependent on the nav data being lava-clean (`hazard.rs:1-12` module header): `creep_scale` slows but never vetoes; `pursue_target_safe` validates the interpolated line but falls back to the raw node. Fixing the *mesh* is the root-cause fix; driver guards are defense-in-depth.
- The navmesh is built **in-process per run** (`supervisor.rs:131-159`, `OnceLock` cache) — no disk-cache version bump needed (unlike the A* `.qnav` v-bumps).
- xon/q3 both already have: `escape_from_lava` survival override (`xon/mod.rs:553`, `q3/mod.rs:918`), creep governor, safe_strafe, hazard-gated dodge. xon lacks `safe_combat_dir` *by architecture* (it re-expresses path legs instead of synthesizing combat dirs) — **not** a gap.
- Measurement now exists: `EVT env_suicide kind=lava` (WARN) + per-bot/per-cause `FleetStats` tallies + scoreboard `env=` column (committed `2ec2e3ef4`). **Instrument-first is the Plan 50/51 core lesson — get the baseline soak before touching the mesh.**
- Plan 50's accepted residual: ~15 short walk-off falls / 5 min on q2dm3's basin walkways (10 Hz movement noise; humans clip these too). The near-zero gate below budgets for this class, not for routing-into-lava.

## Step-by-Step Tasks

### T1: Baseline + offline red tests (instrument first)

**Files**: `crates/world/tests/lava_navmesh_q2dm6.rs` (new), `crates/tools/src/bin/navinspect.rs` (only if a mesh-dump lens is missing)

**What to do**:
1. Run the user's exact q2dm6 command for a timed 5-min soak; record the `env=` scoreboard + per-bot `[lava:N]` breakdowns in the tracker as **baseline**.
2. Pak-gated (skip-if-no-baseq2, pattern: `world/tests/lava_q2dm3.rs`) red tests against the **built q2dm6 navmesh**: (a) assert **zero** walkable polys whose floor surface is deadly (sample each poly center + vertices with the `floor_is_deadly` probe); (b) assert **zero** drop links whose landing or 0–48u overshoot strip is deadly. Both must FAIL before T2/T3 and pass after.

**Commit**: `task(T1): q2dm6 lava baseline + red navmesh-lava regression tests`

### T2: Heightfield deadly-floor span rejection (B1)

**File**: `crates/world/src/navmesh/heightfield.rs`

**What to do**: In `column_floors`, after the existing `oz` liquid check, reject the span when the floor **surface** is deadly — the same two-part test as `navgraph.rs:1833`: `floor_is_deadly(cm, &down.endpos)` (export it from `navgraph.rs` or move to a shared spot in `world`). Keep plain water walkable (water spans are how the mesh approaches swims).

**Verify**: T1 test (a) goes green; q2dm3 + q2dm6 mesh poly counts logged before/after in the tracker (expect a drop only around lava).

**Commit**: `task(T2): heightfield rejects deadly-floor spans (floor_is_deadly port)`

### T3: Drop-link landing validation (B2)

**Files**: `crates/world/src/navmesh/heightfield.rs` (`find_drops`), `crates/world/src/navmesh/polymesh.rs` (`add_drops`)

**What to do**: Validate each drop's landing with `landing_strip_deadly` semantics (landing point + 16/32/48u strip along the drop's horizontal direction). Reject deadly landings in `find_drops`; `add_drops` asserts/filters defensively.

**Verify**: T1 test (b) green.

**Commit**: `task(T3): navmesh drop links validate landings against lava`

### T4: NavmeshDriver fallback guard (B3)

**File**: `crates/brain/src/navmesh_driver.rs`

**What to do**: In `pursue_target_safe`'s fallback, validate `path[nxt]` (e.g. `segment_has_floor(cm, pos, path[nxt])` or a `floor_is_deadly` probe at the vertex); if that fails too, return the current position-adjacent safe point (hold/creep) instead of an unvalidated vertex — mirror what `nav.rs` does for A*, and note the same fallback exists there if trivial to share.

**Verify**: unit test with a synthetic mesh path over a deadly vertex.

**Commit**: `task(T4): navmesh driver never falls back to an unvalidated funnel vertex`

### T5: xon keyboard stale-key veto (B4)

**File**: `crates/brain/src/brains/xon/mod.rs` (quantize call site)

**What to do**: Before applying held keys, re-probe the resulting world direction with `hazard::dir_is_hazardous`; on hazard, force a re-quantize this tick (drop held keys) so the fresh legs (already gated upstream) take over. Cheap: only probe when keys are held stale (not re-keyed this tick).

**Verify**: unit test — a held forward key into a synthetic lava ledge is dropped.

**Commit**: `task(T5): xon keyboard emu re-keys instead of holding into a hazard`

### T6: Live q2dm6 A/B + docs + closeout

**Files**: `context/brain_notes.md`, `context/acceptance.md`, `context/pitfalls.md`, SERIES, plan+tracker

**What to do**: Re-run the T1 baseline command post-fix (same duration, N=2+ runs given the Plan 47 noise floor — env_suicide counts are much less noisy than K/D, one run each direction may suffice; record both). Gate: **lava+slime env_suicides ≤ 2 per bot per 5 min AND < 5% of deaths** across brains/navmodes. Dated brain_notes entry; pitfalls entry ("navmesh backend never had the lava fixes — new floor probes must land in BOTH builders"); acceptance.md counter note; SERIES → done; `git mv` plan+tracker to `completed/`.

**Commit**: `task(T6): q2dm6 lava A/B verified; close Plan 63`

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/world/src/navmesh/heightfield.rs` | `column_floors` deadly-floor rejection; `find_drops` landing validation | P0 |
| `crates/world/src/navgraph.rs` | export/share `floor_is_deadly`, `landing_strip_deadly` | P0 |
| `crates/world/tests/lava_navmesh_q2dm6.rs` | pak-gated red→green regression tests | P0 |
| `crates/world/src/navmesh/polymesh.rs` | `add_drops` defensive filter | P1 |
| `crates/brain/src/navmesh_driver.rs` | fallback vertex validation | P1 |
| `crates/brain/src/brains/xon/mod.rs` | stale-key hazard veto | P2 |
| `context/{brain_notes,pitfalls,acceptance}.md` | findings + counters | P1 |

## Open Questions / Risks

1. **Poly loss around lava rims could disconnect navmesh routes** (the A* v21 fix shrank node coverage too). Mitigation: T2 logs poly-count deltas per map; if q2dm6/q2dm3 mesh reach regresses (T1 red tests include a reach smoke check via `query::path` between two known-good points), widen only the *surface* test, never re-admit deadly floors.
2. **Deep-lava columns are already rejected — is q2dm6's lava shallow?** Unverified assumption behind B1's impact. Mitigation: T1's poly audit answers it empirically before any fix lands; if q2dm6's pools are deep (already rejected), the mesh audit will pass and the hunt moves to B2/B3 + live `EVT lava_escape` vz forensics (Plan 50 method).
3. **`escape_from_lava` fires but bots still die** (fatal-per-entry was 55% post-Plan 50). This plan reduces *entries*; the escape override is unchanged. If post-fix deaths persist with low entry counts, that's a separate escape-tuning follow-up — don't scope-creep here.
4. **K/D noise** (Plan 47): env_suicide counts are the gate, not K/D. Don't chase kd movement in T6.

## Verification Checklist

- [ ] T1: baseline env_suicide numbers recorded; both pak-gated tests RED; committed
- [ ] T2: test (a) green; poly-count deltas logged; `cargo build`/`clippy`/`test` clean; committed
- [ ] T3: test (b) green; committed
- [ ] T4: synthetic fallback unit test green; committed
- [ ] T5: stale-key veto unit test green; committed
- [ ] T6: live q2dm6 post-fix soak meets the gate (lava+slime ≤ 2/bot/5min, < 5% of deaths); brain_notes + pitfalls + acceptance updated; SERIES marked done; plan+tracker `git mv`'d to `completed/`; committed
