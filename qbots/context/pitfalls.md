# Pitfalls & Wire-Format Gotchas

Read before new work. Every bug/gotcha, **especially** multi-attempt fixes.
Template: `# Title → Problem → Fix → Source`.

---

# `delta_angles` rotates every usercmd view angle — aim AND movement

## Problem

An external bot that does **absolute world-space** aiming (compute a world yaw
from `origin → target`, encode it into `usercmd.angles`) will have its shots
**and** its movement consistently rotated away from the target — bots walk into
walls in the wrong direction and freeze. The symptom looks like a nav-graph or
movement-vector bug, and chasing it through nav direction / forwardmove / yaw
conventions wastes days (it took 5 debug commits here before the real cause).

Root cause: the server does **not** use `usercmd.angles[i]` directly. In pmove it
computes the player's real view angle as:

```c
// pmove.c:1255  (PM_SetAngles, the per-pmove angle resolution)
temp = pm->cmd.angles[i] + pm->s.delta_angles[i];   // i16 wraparound
pm->viewangles[i] = SHORT2ANGLE(temp);
AngleVectors(pm->viewangles, pml.forward, pml.right, pml.up);
```

`delta_angles` is seeded by the gamecode on **every spawn/respawn** and persists:

```c
// game/player/client.c:1675  (ClientRespawn / PutClientInServer)
client->ps.pmove.delta_angles[i] = ANGLE2SHORT(spawn_angles[i] - resp.cmd_angles[i]);
```

So if the bot spawns facing `spawn_angles[YAW]` (often 90°/180° from the spawn
point) and its last `cmd_angles` was 0, `delta_angles[YAW]` becomes a non-zero
constant. Every frame the server adds it to whatever yaw we send. Our desired
world yaw `Y` sent as `ANGLE2SHORT(Y)` becomes `SHORT2ANGLE(ANGLE2SHORT(Y) +
delta) = Y + spawn_yaw_offset`. Constant rotation → wrong aim **and** wrong walk
direction (because pmove builds the movement frame from `AngleVectors` of that
same offset angle).

A **human** client never hits this: it maintains `cl.viewangles` as its own
relative coordinate and turns relatively, so the constant offset cancels. The
offset only matters for a client that targets a **specific world angle** — i.e.
a math-aiming bot.

## Fix / How to avoid

When encoding a desired **world-space** angle `Y` into `usercmd.angles[axis]`,
**subtract** `delta_angles` in i16 modular space, matching `ANGLE2SHORT` and the
server's `short` wraparound:

```rust
let desired = ((yaw_deg * 65536.0 / 360.0).round() as i32).rem_euclid(65536);
let delta   = (delta_angles[axis] as i32).rem_euclid(65536);
let val     = ((desired + 65536 - delta) % 65536) as u16;  // then `as i16`
```

Feed `delta_angles` from the latest `playerstate.pmove.delta_angles` into the
movement controller **every tick** (it changes on respawn/teleport/knockback).

Verify with a round-trip test: `SHORT2ANGLE(encoded + delta)` must equal the
input world yaw (test at offsets 0/90/180°). Done in
`brain/src/move_ctrl.rs` (`MovementController::angle_short`,
`set_delta_angles`).

## Sources
- qbots: `crates/brain/src/move_ctrl.rs` (`MovementController::angle_short`,
  `set_delta_angles`, `build_cmd`)
- qbots: `crates/qbots/src/main.rs` (tick loop feeds `frame.playerstate.pmove.delta_angles`)
- vendor: `vendor/yquake2/src/common/pmove.c:1243-1270` (server angle resolution)
- vendor: `vendor/yquake2/src/common/header/shared.h:1184` (`ANGLE2SHORT`)
- vendor: `vendor/yquake2/src/game/player/client.c:1675` (delta_angles seeding)

---

# FOV-only targeting shoots and chases through walls

## Problem
`view.nearest_enemy(fov)` filtered by view cone but **not** by geometry. The bot would
select the nearest enemy, set `nav_goal = NavGoal::Entity(enemy.origin)`, and fire — even
when a solid wall separated them. Result: bot walks face-first into a wall for 8 s (the
give-up watchdog), fires into geometry, wastes ammo, and ignores reachable enemies.

## Fix
Add a BSP trace (`CollisionModel::trace` with zero-size box and `MASK_SOLID`) from eye to
enemy chest and feet (`has_los_player`). Gate BOTH the nav override (`nav_goal = Entity(...)`)
AND `should_fire` on this check. A 2-frame grace period (`SIGHT_GRACE_FRAMES=2`) keeps the
target alive after momentary occlusion (thin pillars, enemy strafing behind cover), then drops
it. The FSM transitions to Hunt with the last-known position.

## Sources
- qbots: `crates/brain/src/los.rs` (`has_los`, `has_los_player`, `eye_origin`)
- qbots: `crates/brain/src/combat.rs` (`select_target_entity`, `sight_grace_remaining`)
- qbots: `crates/qbots/src/main.rs` (nav-to-enemy LOS gate, Plan 11 T4)

---

# Two divergent stuck detectors + blind reverse caused stall grinding

## Problem
qbots had **two independent stuck detectors** that disagreed:
1. `NavigationDriver.stuck_ticks` in `nav.rs`: flagged stuck at `<16u` movement over 30 ticks
   (3 s), called `is_stuck()`.
2. `stuck_frames` counter in `bot_task` (`main.rs`): flagged stuck at `<1u` movement over 50
   ticks (5 s) — only logged a warning, never acted.

Both had wrong thresholds (Eraser uses **4u / 1s**). When `nav.is_stuck()` fired, the recovery
was a blind view-relative reverse (`mv.move_forward(-1.0)`): this backed the bot *toward
whatever wall it was facing*, then `force_replan` re-ran A* to the **same** goal on the **same**
wedged route. The bot would stall against geometry for 8 s, briefly reverse into the same wall,
and re-wedge — in an infinite loop on tight corners. There was also no lateral scan: if a gap
existed 45° to the side, the bot would never find it.

## Fix / How to avoid
Unify into a single `StuckDetector` (in `brain::recover`) with **4u deadband on a 1s cadence**
matching Eraser's `botRoamFindBestDirection` reference. Return a typed `StuckLevel { None, Mild,
Hard }` and escalate: Mild → jump (clear step/ledge); Hard → back off + `force_replan` (but
only when `!engaging`, to avoid abandoning a live duel). Add a **6-direction fan-out hull trace**
(`find_best_direction`) to pick a clear yaw when no nav node is near. Replace the view-relative
reverse with a **world-space lateral strafe** (decomposed via `move_from_world_dir` so it stays
correct even when view yaw is on an enemy). Hull traces for the fan-out use `HULL_MINS/HULL_MAXS`
matching the player bounding box, lifted by `STEPSIZE=24` to clear ground clutter. Wall-ahead
check uses a 32u forward probe to distinguish "step/ledge" (Jump) from "solid wall" (Strafe).

## Sources
- qbots: `crates/brain/src/recover.rs` (`StuckDetector`, `find_best_direction`, `Recovery`)
- qbots: `crates/brain/src/nav.rs` (removed `stuck_ticks`/`is_stuck`; `force_replan` kept)
- qbots: `crates/qbots/src/main.rs` (Plan 13 T4 steering step 6)
- vendor: `vendor/Quake2BotArchive/research/bots/eraser.md` (§3 stuck/give-up; §9 fan-out)

---

# Missing -1/+1 model-bounds margin made collision "too tight" — nodes rejected as solid

## Problem

`Bsp::parse_models` read `dmodel_t.mins/maxs` straight off the wire. yquake2's loader does
not: `collision.c:1220-1223` applies a 1-unit margin in both directions —
`out->mins[j] = LittleFloat(in->mins[j]) - 1; out->maxs[j] = LittleFloat(in->maxs[j]) + 1;`
(comment: `/* spread the mins / maxs by a pixel */`). Without it our collision model was a
pixel tighter than the real game's on every axis of every model — small enough to look like a
rounding non-issue, but it meant nav-graph waypoint sampling (which traces against this exact
boundary) intermittently classified legitimately walkable floor as `startsolid`, rejecting
nodes near model edges. Symptom: nodes missing at the "wrong" Z levels, spawn points
unreachable, and paths that looked like they should clear geometry getting blocked instead.
It read like a nav-graph or pathfinding bug and took three separate analysis passes
(`bsp_bug_analysis.md`, the original `16_bsp_parsing_fix.md` plan, and its summary doc) before
the 1-pixel margin was identified as the actual root cause — exactly the kind of multi-attempt
fix this file exists to capture, just never written down until now (commit `b72600ae2`).

## Fix / How to avoid

Apply the same `-1`/`+1` margin to `mins`/`maxs` when parsing `LUMP_MODELS`, matching
`collision.c:1220-1223` exactly (don't "round" it away as insignificant — it changes whether
boundary traces hit `startsolid`). Added `model_bounds_have_margin` test to lock this in
(`crates/world/src/bsp.rs`). When a BSP-derived geometry value disagrees with the real game by
a suspiciously small amount, check the vendor loader for an undocumented fudge-factor before
assuming it's noise — Q2's collision code has several ("spread by a pixel" here; `DIST_EPSILON`
elsewhere).

## Sources
- qbots: `crates/world/src/bsp.rs` (`parse_models`, `model_bounds_have_margin` test)
- vendor: `vendor/yquake2/src/common/collision.c:1220-1223`
- qbots: `context/plans/completed/16_bsp_parsing_fix.md`, `16_bsp_parsing_fix_summary.md`,
  `bsp_bug_analysis.md` (the three docs it took to find this)

---

# Nav graph fragmentation: grid-sampling creates disconnected components on multi-level maps

## Problem

The nav graph generated by `NavGraph::generate()` samples waypoints on a 64u grid and connects
8-neighbors if the hull trace clears and height difference ≤ STEP (24u). On multi-level maps
like q2dm1, this naturally creates **disconnected components** (6 components in q2dm1: 3495,
540, 112, 54, 43, 15 nodes). Spawns scattered across these components are **unreachable** from
each other — pathfinding returns `None`, bots get stuck at spawn, or orbit waypoints indefinitely.

Symptom: bots spawn in component 3 (43 nodes) but the farthest goal is in component 0 (3495
nodes). The bot cannot path between them, resulting in 0/8 bots reaching goals. The issue looks
like a movement bug, but it's actually a **connectivity** bug in the nav graph.

## Fix / How to avoid

**Two-part solution:**

1. **Bridge disconnected components** (`NavGraph::connect_components()`):
   - For each pair of components, find the closest node pairs
   - Add bidirectional walk edges if:
     * Horizontal distance ≤ max_bridge_dist (512u)
     * Height difference ≤ STEP (24u)
     * Hull trace is clear
   - Add up to 3 bridges per component pair for redundancy
   - Call after `generate()`, `detect_jump_edges()`, and `seed_spawns()`

2. **Select reachable goals** (`farthest_reachable_spawn()`):
   - Find which component the bot is in
   - Filter spawns to those in the same (or connected) component
   - Pick the farthest among reachable spawns
   - Fall back to nearest spawn if no reachable spawns exist

**Why this works:** Bridges create artificial walkable paths between components that are
geometrically close but not connected by the grid sampler. Reachable goal selection ensures
bots don't attempt impossible paths.

**Trade-offs:**
- Bridges may create paths that are "technically" walkable but suboptimal (bots might take
  longer routes)
- Some components are too far apart or at incompatible heights — they remain disconnected
  (this is correct; forcing bridges would create invalid paths)
- Reachable goal selection means bots may not reach the "true" farthest spawn, but they will
  reach a valid goal

**Results:** Before fix: 0/8 bots reaching goals, stuck at spawn. After fix: 2-3/8 bots reaching,
others traveling 1000-2000 units before getting stuck near goal (path quality issue, not
connectivity).

## Sources
- qbots: `crates/world/src/navgraph.rs` (`NavGraph::connect_components()`, `components()`)
- qbots: `crates/qbots/src/scenario.rs` (`farthest_reachable_spawn()`)
- qbots: `crates/qbots/src/supervisor.rs` (bridge call in nav graph setup)

---

# Diagonal hull trace clips stair risers — only 3/10 spawns reachable

## Problem

`NavGraph::generate()` Phase 3 edge-building had two bugs that together prevented
staircase connectivity on q2dm1 (only 3/10 spawn points in the largest component):

1. **Height-diff gate too tight**: `if (a[2]-b[2]).abs() > STEP { continue; }` — with
   `STEP=18.0` and `GRID_SPACING=24.0`, adjacent floor probes on 8u×8u Q2 stairs can
   differ by ~24u in Z (> STEP), so all stair edges were silently skipped.

2. **Diagonal trace clips stair risers**: Even when dz ≤ STEP, the diagonal hull trace
   from node A `(x, y, floor+24)` to node B `(x+24, y, other_floor+24)` travels through
   the stair riser (the vertical wall between treads). The hull bottom at intermediate X
   positions overlaps the riser brush → `fraction < 1.0` → edge rejected.

The same bugs existed in `seed_spawns()`, preventing spawn nodes from connecting to
staircase grid nodes at height differences just above STEP.

## Fix

Add `walkable_stair(cm, lower, upper) -> bool` — iterative step-climb trace mirroring
Q2 pmove's up→forward movement. For height diff in `(STEP, STAIR_MAX=42u]`, instead of
a diagonal trace, step up by STEP vertically at current XY, then advance horizontally by
the proportional XY fraction toward the target, repeating `ceil(dz/STEP)` times. Actual
walls and cliffs block horizontal sub-traces; stair risers don't block vertical ones.
Apply to both `generate()` Phase 3 and `seed_spawns()`. Add `STAIR_MAX` to the cache
fingerprint so stale caches auto-invalidate. After the fix, regenerate with
`cargo run --bin qbots -- generate-map-cache --map 'q2dm*' --jobs 4`.

## Sources
- qbots: `crates/world/src/navgraph.rs` (`generate()`, `seed_spawns()`, `walkable_stair()`)
- qbots: `crates/world/src/mapcache.rs` (`Fingerprint::stair_max_bits`)
