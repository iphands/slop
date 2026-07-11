# Plan 48 — q2dm3 Lava Deaths + zb2 Wall-Running Fixes

> **Status**: done
> **Created**: 2026-07-10
> **Depends on**: Plan 47
> **Goal**: Bots stop steering themselves into q2dm3's lava, and the zb2 brain stops grinding walls instead of engaging.
> **Agent**: implementation agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Fix five verified bugs — one in the world crate's floor-continuity probe, three in
the zb2 brain, one in shared/main combat steering — that together cause the observed q2dm3
lava deaths and zb2 wall-running.

**Deliverables**:
1. Lava/slime-aware `segment_has_floor` + lava-covered floors excluded from node sampling (cache v21).
2. A shared ground-hazard probe (`brain::hazard`) gating combat strafe/backpedal/kite, projectile dodge, and stuck-strafe.
3. zb2 shortcut skips validated for walkability (hull + floor continuity), not just LOS.
4. zb2 no-route branch that actually engages (aim/fire/steer) and a hard-stuck replan that rotates the roam goal instead of recommitting the identical route.
5. Pitfall + brain-notes documentation of every bug.

**Estimated effort**: Medium (1 day)

---

## Context

Live q2dm3 matches show two symptoms: (a) bots of every brain repeatedly dying in the central
lava, (b) zb2 bots pressing into walls instead of fighting. A deep code audit (two fan-out
searches + manual verification of every claim) confirmed five root causes.

### Pre-Identified Bugs

**BUG L1 — `segment_has_floor` treats the lava bed as floor** (`world/src/navgraph.rs:1736-1763`).
The probe traces down 96 u with `MASK_SOLID` only. Over a shallow lava pool the *solid bed
under the lava* is within 96 u, so `fraction < 1.0` → "has floor". Every caller —
`nav.rs:428` (`pursue_target_safe`, the corner-cut guard for main/q3) and
`navmesh_driver.rs:135` — therefore approves straight-line shortcuts across lava.
Deep pits (bed > 96 u down) are already rejected; shallow pools are the killer.

**BUG L2 — combat movement has no ground-hazard check** (`brain/src/brains/main.rs:588-671`,
`:755-760`). Backpedal (`d < backup_dist`), kite, circle-strafe, chase-tangential, and the
projectile dodge all emit raw world-space move directions decomposed via
`move_from_world_dir(.., face_then_go=false)` — nothing probes the ground along that
direction. On q2dm3 the classic death is backpedaling away from an enemy straight into the
pool while aiming at them. `RecoveryAction::Strafe` (`recover.rs:279`) is equally blind.
The only existing lava awareness at runtime is `find_best_direction` (`recover.rs:158-162`),
which never runs during normal steering or combat.

**BUG L3 (found during T2) — main and q3 never use `pursue_target_safe`**
(`brains/main.rs` — six raw `pursue_target` calls; `brains/q3/mod.rs:256/268/275/289`).
The corner-cut-safe look-ahead (hull trace + floor-continuity — the exact guard T1 made
lava-aware) existed but only `runtester` called it. main/q3 steered at the RAW interpolated
look-ahead, which legally cuts inside corners and across lava pools. Fixed in T2: both brains
compute one `pursue_pt` per tick via `pursue_target_safe` (falling back to raw only when no
collision model is loaded) and every steering consumer reads it.

**BUG Z1 — zb2 `nearly_pod_skip` skips on LOS + dz only** (`brain/src/brains/zb2.rs:137-159`).
The doc comment even says "LOS ≠ walkable (the classic bot trap)" but only gates |dz| ≤ 32.
On q2dm3, path polylines curve *around* the lava; a node further along the route on the far
side of the pool is eye-visible and near-level → the cursor skips → the bot steers straight
across the lava. Same mechanism grinds walls when LOS passes over a railing/ledge the hull
cannot cross.

**BUG Z2 — zb2's no-route branch neither engages nor steers** (`brain/src/brains/zb2.rs:417-420`).
When `route == None` (plan failure — e.g. `graph.nearest(pos)` lands in a different component
after a fall/ride, so every roam goal is unpathable) the brain either (should_fire=true) does
*nothing at all* — no aim, no attack, no movement — or (should_fire=false) blind-runs
`move_forward(1.0)` with no steering, no recovery, no look_at. That is literally "running
into a wall instead of engaging".

**BUG Z3 — zb2 hard-stuck replan recommits the identical route** (`brain/src/brains/zb2.rs:374-377`).
`BackOffThenRepath` sets `route.dirty`; the next tick replans from `graph.nearest(pos)` to the
*same* goal node → the same polyline → stuck at the same spot → loop forever.
`Zb2Route::blacklist_waypoint_if_blocked` is a deliberate no-op and nothing rotates the goal.
(Related: `recovery.evaluate` receives `combat_dec.should_fire` as `engaging`
(`zb2.rs:369`), so a firing bot hard-stuck on a wall only ever strafes — acceptable while the
goal-rotation fix breaks the outer loop, but noted.)

### Key Facts
- `MASK_WATER = CONTENTS_WATER | CONTENTS_LAVA | CONTENTS_SLIME` (`collision.rs:23`); walking
  through shallow *water* is legal and must stay legal — only LAVA|SLIME are deadly.
- Dry node sampling already rejects nodes *inside* lava (`navgraph.rs:1590`), but a floor whose
  lava is shallower than 24 u yields a node hovering just above lava — excluded by T1's
  floor-surface check. Node changes ⇒ `mapcache.rs:69` `VERSION` 20 → 21.
- Integration tests are pak-gated (`vendor/baseq2/pak0.pak` or `QBOTS_BASEQ2`), pattern in
  `crates/world/tests/ride_q2dm3.rs`.

---

## Step-by-Step Tasks

### T1: Lava-aware floor validation in `world`

**File**: `crates/world/src/navgraph.rs`, `crates/world/src/mapcache.rs`

**What to do**:
1. In `segment_has_floor`: import `CONTENTS_LAVA`/`CONTENTS_SLIME`. For each sample point,
   (a) if `point_contents(p)` has LAVA|SLIME → return false (path passes through a lava
   volume); (b) after the down-trace hits (`fraction < 1.0`), check
   `point_contents([endpos.x, endpos.y, endpos.z + 1])` — LAVA|SLIME → the "floor" is a lava
   bed → return false.
2. In `floor_waypoints_multi`: after computing `floor_z`, if
   `point_contents([x, y, floor_z + 1.0])` has LAVA|SLIME → do not emit a node for this floor
   (still continue probing lower floors).
3. Bump `mapcache::VERSION` 20 → 21.
4. Pak-gated regression test (q2dm3): scan a coarse grid for a column whose down-trace floor
   is lava-covered; assert `segment_has_floor` across it is false. Skip-and-pass without pak.

**Commit**: `task(P48-T1): lava-aware segment_has_floor + node sampling (cache v21)`

### T2: Shared ground-hazard probe + gate combat/dodge/stuck strafing

**File**: `crates/brain/src/hazard.rs` (new), `crates/brain/src/lib.rs`,
`crates/brain/src/brains/main.rs`, `crates/brain/src/brains/zb2.rs`

**What to do**:
1. New module `hazard.rs`:
   ```rust
   /// True if walking from `pos` along `world_dir` (XY, normalized) is deadly or a
   /// blind drop: samples at 24 u and 48 u ahead (stepped up STEPSIZE), probes down
   /// 128 u with MASK_SOLID; hazard when the sample point or the floor surface is
   /// LAVA|SLIME, or no floor exists within the probe (ledge).
   pub fn dir_is_hazardous(cm: &CollisionModel, pos: Vec3, world_dir: Vec3) -> bool
   ```
   Startsolid samples are non-hazard (walls stop the bot; the wall probe owns that case).
2. `main.rs`: for every `face_then_go = false` world dir (flee-while-firing, kite, backpedal,
   hold-strafe, chase+tangential): if `dir_is_hazardous`, retry with the tangential sign
   flipped; if still hazardous, fall back to `(nav_dir, true)` when available else `Vec3::ZERO`
   (stand and fight beats swimming in lava).
3. `main.rs` dodge block (~:755): if the dodge strafe dir is hazardous, negate it.
4. `RecoveryAction::Strafe` application in `main.rs` and `zb2.rs`: world dir =
   `view_right(view_yaw) * dir`; if hazardous, use `-dir`.
5. Unit-test the pure geometry (pak-gated q2dm3 test: a point at the lava edge reports
   hazardous toward the pool, non-hazardous away).

**Commit**: `task(P48-T2): ground-hazard probe gates combat strafe/backpedal/dodge`

### T3: zb2 shortcut skip must be walkable, not merely visible

**File**: `crates/brain/src/brains/zb2.rs`

**What to do**: Add a `walkable: &dyn Fn(Vec3) -> bool` parameter to `nearly_pod_skip`
(checked alongside `visible`). At the call site it is a hull trace `pos → node`
(`HULL_MINS/MAXS`, `MASK_SOLID`, not startsolid, fraction ≥ 1) **plus**
`segment_has_floor(cm, pos, node)`. Update the unit tests (all-true closure keeps old
behavior; add a case where a mid-line hazard blocks the skip).

**Commit**: `task(P48-T3): zb2 shortcut skip requires hull+floor-valid straight line`

### T4: zb2 no-route engagement + goal rotation on repeated hard-stuck

**File**: `crates/brain/src/brains/zb2.rs`

**What to do**:
1. Replace the `else if !combat_dec.should_fire` tail with a real fallback branch:
   - Always steer via `recovery.evaluate(...)` (`has_nav_target=false` → `UseHeading` from
     `find_best_direction`, which already avoids lava/ledges) instead of blind
     `move_forward(1.0)`; decompose the heading with `move_from_world_dir`.
   - When `should_fire`: `look_at(aim_yaw, aim_pitch)` + `attack()` so a route-less bot still
     fights.
2. Add `hard_replans: u32` to `Zb2Brain`. On `BackOffThenRepath` increment; when ≥ 2 and no
   `goal_override` is active, call `next_roam_goal()` and reset. Reset on route finish or goal
   change.

**Commit**: `task(P48-T4): zb2 engages without a route; hard-stuck rotates the roam goal`

### T5: Documentation

**File**: `context/pitfalls.md`, `context/brain_notes.md`

**What to do**: Pitfall entries — "MASK_SOLID floor probes see lava beds as floor",
"LOS ≠ walkable: validate shortcut skips", "brain fallback branches must still aim/steer".
Dated brain-notes section summarizing the five bugs + fixes. Move plan + tracker to
`completed/`, mark SERIES.

**Commit**: `docs(P48): pitfalls + brain notes; close plan`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/world/src/navgraph.rs` | Lava-aware `segment_has_floor` + node sampling | P0 |
| `crates/world/src/mapcache.rs` | VERSION 20 → 21 | P0 |
| `crates/brain/src/hazard.rs` | New ground-hazard probe | P0 |
| `crates/brain/src/brains/main.rs` | Gate combat strafe/backpedal/dodge/stuck-strafe | P0 |
| `crates/brain/src/brains/zb2.rs` | Walkable shortcut, no-route engage, goal rotation | P0 |
| `context/pitfalls.md`, `context/brain_notes.md` | Documentation | P1 |

## Open Questions / Risks

1. **Hazard probe false positives on intended drop-offs** — jump-down edges are traversal-owned
   (gates suspend combat strafing already), and the probe only gates *combat/dodge/stuck*
   world dirs, never route pursuit. Mitigation: 128 u no-floor threshold tolerates step-downs.
2. **Cache v21 rebuild cost** — one-time regeneration per map; acceptable (previous bumps v16→v20).
3. **`segment_has_floor` extra `point_contents` calls** — 2 per 16 u sample; negligible vs the
   existing trace.
4. **Live verification needs a running server** — offline pak-gated tests cover geometry; a
   live q2dm3 soak (death-by-lava count before/after) is recommended post-merge.

## Verification Checklist

- [x] T1: pak-gated q2dm3 test proves `segment_has_floor` rejects lava crossings (verified red pre-fix via stash); `cargo test -p world` green; cache v21
- [x] T2: hazard-probe pak test green (self-locating rim); main + q3 strafe/dodge/stuck sites gated; L3 fixed (both use `pursue_target_safe`)
- [x] T3: `nearly_pod_skip` walkable-gate unit test added (`shortcut_respects_the_walkable_gate`)
- [x] T4: zb2 no-route branch aims/fires/steers via `find_best_direction`; 2 consecutive hard-stuck replans block the goal 20 s
- [x] T5: pitfalls (3 entries) + brain notes on disk; plan moved to `completed/`; SERIES updated
- [x] All: `cargo build` + `cargo clippy` zero warnings, `cargo fmt`, 392 workspace tests green at every commit
