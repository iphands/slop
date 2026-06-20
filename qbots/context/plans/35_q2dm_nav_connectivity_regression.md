# Plan 35 — q2dm nav connectivity regression (5/8 stock maps fail)

> **Status**: pending
> **Created**: 2026-06-18
> **Depends on**: Plan 34 (diagnostics + tooling)
> **Goal**: Restore full spawn connectivity to the stock DM maps so `generate-map-cache --map 'q2dm*'` caches all 8 (q2dm7 may stay 5/6 — the accepted pit).
> **Agent**: TBD

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: A nav-graph regression now fails `check_spawn_connectivity` on **5 of 8** stock DM
maps. Find and fix the regression so they all reach full spawn connectivity again.

**Deliverables**:
1. Root-cause the regression (bisect the suspect commits; confirm per-map).
2. Fix it without re-introducing the false edges the suspect commits were added to remove.
3. q2dm1/2/3/4/5/6/8 = full spawns; q2dm7 ≥ 5/6 (accepted pit). Regen + live spot-check.

**Estimated effort**: Medium–Large (uncertain; iterative nav work).

## Context

`qbots generate-map-cache --map 'q2dm*' --spacing 24` (Plan 34 T6, `--allow-failures`):

| map | result |
|-----|--------|
| q2dm1 | PASS |
| q2dm2 | **FAIL 3/7** |
| q2dm3 | **FAIL 3/7** |
| q2dm4 | PASS |
| q2dm5 | **FAIL 7/9** |
| q2dm6 | **FAIL 7/8** |
| q2dm7 | **FAIL 3/6** (was 5/6 accepted) |
| q2dm8 | PASS |

`context/map_errors.notes.log.md:115-123` recorded **7/8 passing** on 2026-06-16
(q2dm1/2/3/4/5/6/8 PASS, q2dm7 5/6). So this regressed broadly since.

### Suspect regressions (in order)
1. **`walkable_stair` floor-existence check** — commit `662580e69` (2026-06-17) added a
   downward `STEP*2` floor probe at each stair step (`navgraph.rs:1344`). Intended to reject
   open-air "folded staircase" shortcuts; **now also reports `stair=FAIL` on the real
   cross-floor candidate pairs** in `nav-debug q2dm3`. CANNOT simply be removed — the worst
   q2dm3 offenders are genuine same-XY `dz=144` open-air shortcuts it *correctly* rejects.
2. **`BRIDGE_HDIST` cut 512 → 128 → 256** (`build.rs:28`, commit `1c0756a85`, a
   steering/smoothing change). But restoring 512 alone **does not** fix q2dm3 (still 3/7),
   so it's at most a contributing factor.
3. **`CONNECT_RADIUS` / connection-window changes** (`66c122e6b`, `450e6cdc7`, `3cdf173c3`,
   `d23a1d411`) — re-shaped which neighbours `generate()` connects; rule in/out by bisect.

### Deeper structural finding (q2dm3, Plan 34 T3)
The real gap on q2dm3 is **no nodes sampled in the z=83..168 band** — the staircase treads
between the lower (comp1) and mid (comp2) floors aren't in the graph, so the only cross-floor
candidates are false vertical pairs. Plus a **walled-off `func_plat` pocket (comp4)** and a
`func_door` top node orphaned 69u below the upper corridor. Re-diagnose each failing map; the
cause may differ per map.

## Step-by-Step Tasks (outline — refine after bisect)

### T1: bisect / attribute the regression
Use `qbots nav-debug <map>` (live) across the suspect commits to attribute each map's failure
to a specific commit. Record per-map cause in `map_errors.notes.log.md`.

### T2: fix `walkable_stair` floor-check over-rejection
Make the floor-existence probe distinguish a real stair tread from an open-air shortcut
without failing real cross-floor stairs (e.g. probe along the actual step XY, tighten only the
vertical-shortcut case, or gate by horizontal/vertical slope). Keep the false-edge rejection
that `662580e69` was added for (re-verify the 32-bot improvement it claimed).

### T3: re-evaluate `BRIDGE_HDIST` + spawn-aware bridge
Decide global value vs a spawn-aware escalating bridge (Plan 34's old C2 idea): widen reach
only between spawn-bearing components, via strict `walkable_stair_link_orig`.

### T4: lift/plat anchoring + tread sampling (if still needed)
Anchor lift top nodes (probe floor just outside the shaft) and/or fix `floor_waypoints_multi`
inline-model blindness so stair treads + platform landings are sampled.

### T5: regen + verify all q2dm*
All stock maps full connectivity (q2dm7 ≥ 5/6); live `spawn-to-spawn`/`spawn-to-weapon`
spot-check on a previously-failing map (false-bridge guard). Bump `mapcache::VERSION` for any
gen change.

## Critical Files
- `crates/world/src/navgraph.rs` — `walkable_stair` (floor-check), `bridge_*`,
  `floor_waypoints_multi`, `connect_cells`/`CONNECT_RADIUS`.
- `crates/world/src/build.rs` — `BRIDGE_HDIST`, `generate_map_nav`, `add_lift`.
- `crates/world/src/mapcache.rs` — `VERSION`/`Fingerprint`.

## Open Questions / Risks
1. Removing/relaxing the floor-check risks re-introducing the navigation-loop false edges it
   fixed. *Mitigation*: re-run the 32-bot reach test it cited; keep the vertical-shortcut
   rejection.
2. Per-map causes may differ — don't assume one fix covers all 5. *Mitigation*: T1 bisect +
   per-map nav-debug.
3. Connectivity passing ≠ navigable. *Mitigation*: live spawn-to-* on a fixed map.

## Verification Checklist
- [ ] T1: each failing map's regression attributed to a commit + cause documented.
- [ ] T2: q2dm3 cross-floor stairs `stair=PASS` again; the `662580e69` false-edge case still rejected.
- [ ] T5: q2dm1/2/3/4/5/6/8 full spawns; q2dm7 ≥ 5/6; `generate-map-cache --map 'q2dm*'` err≤1.
- [ ] Live spawn-to-spawn on a previously-failing map traverses the restored connection.
- [ ] fmt + clippy(-D warnings) + tests green before each commit; `mapcache::VERSION` bumped.

## DEFERRED 2026-06-19: q2dm3 upper-level far-spawn route reliability
The quad RIDE is solved (Plan 43); the quad is reached from spawn3 (the board ledge). Making
FAR spawns route reliably to the board needs hull-validating + splitting bridge edges and
resampling the fragmented upper level (confirmed blocker: hull-blocked 354u 'walk' bridges).
User decision (2026-06-19): defer — stop at ride-from-spawn3. See brain_notes.md.
