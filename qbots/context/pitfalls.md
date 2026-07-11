# Pitfalls & Wire-Format Gotchas

Read before new work. Every bug/gotcha, **especially** multi-attempt fixes.
Template: `# Title → Problem → Fix → Source`.

---

# usercmd `msec` hardcoded → bots move at 1/3 human speed (and it masks nav bugs)

## Problem

`MovementController` hardcoded `msec = 33` (a "~30 Hz client rate" assumption).
But the bot loop runs at the **server frame cadence of 10 Hz** (one usercmd per
`svc_frame`, 100 ms apart). The Q2 server runs `PM_Move` **once per received
usercmd**, using `cmd.msec` as the physics timestep
(`pmove.c`: `pml.frametime = pm->cmd.msec * 0.001`). So sending `msec=33` for a
100 ms tick advanced physics only 33 ms — bots ran at **one-third of
`pm_maxspeed`** even though `forwardmove` was a full 320.

A real client at 60 fps sends ~6 usercmds per server tick (each `msec≈16`),
totalling ~96 ms of physics per tick. Our single `msec=33` usercmd is the bug:
the human covers 3× the ground per server tick.

This is insidious because it **masquerades as a navigation/pathing problem.**
For weeks the symptom ("bots are slow, path_efficiency is low, they get stuck")
was chased through nav-graph quality, orbit/giveup tuning, LOOKAHEAD, and
face_then_go throttling — none of which were the root cause. The slow speed also
*inflated* the apparent value of timeout/giveup constants (a bot that crawls
looks "stuck" long before a full-speed bot would).

## Fix / How to avoid

Set `msec` from the **measured server-frame delta** each tick, not a constant:
`move_ctrl.set_msec(dt)` where `dt = (serverframe_delta * 0.1).clamp(...)`.
`set_msec` does `(dt_secs * 1000).clamp(1, 250) as u8`. Call it in **both** the
scenario loop and the main bot loop, right before `build_cmd`.

Result: spawn-to-spawn 17→24/32 (53%→75%); per-frame mean_speed 95→150-250 u/s;
bots visibly match a real player's run speed.

**General lesson:** when "bots are slow / stuck", FIRST verify the physics
timestep the server actually integrates (`cmd.msec`) before touching nav/steer
constants. A wrong `msec` is a single fundamental bug that corrupts BOTH speed
AND pathing metrics simultaneously.

## Sources
- qbots: `crates/brain/src/move_ctrl.rs` (`MovementController::set_msec`)
- qbots: `crates/qbots/src/scenario.rs`, `crates/qbots/src/main.rs` (call sites)
- vendor: `yquake2/src/common/pmove.c` (`pml.frametime = cmd.msec * 0.001`)
- vendor: `yquake2/src/server/sv_user.c` (server runs pmove per usercmd)

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

---

# STAIR_MAX too small for multi-flight staircases — q2dm3 3/7 spawns reachable

## Problem

`STAIR_MAX=42` (original) then `STAIR_MAX=128` (raised in prior fix) were both
too small for q2dm3 ("The Frag Chamber"). The multi-floor column probe finds two
floor surfaces at the **same XY** that are 144u apart vertically — the bottom and
top landings of a staircase flight. In `bridge_components`, pairs with `dz > STAIR_MAX`
are skipped entirely (the check is cheap, before any trace). With `STAIR_MAX=128`,
dz=144 pairs were silently dropped, leaving comp1 and comp2 permanently disconnected
(q2dm3 showed 3/7 spawn points in the largest component — a false NAV BUG report).

Raising to 128 (from 42) fixed q2dm1/2/5/6 but still missed q2dm3's dz=144 pairs.
This required a second adjustment to 160 before q2dm3 passed.

## Fix

Set `STAIR_MAX = 160.0`. The `walkable_stair` trace is the real gatekeeper: actual
walls block horizontal sub-traces; stair-riser vertical clearance is not an issue
(the vertical portion of each step goes through open air in the staircase column).
A larger STAIR_MAX only causes **more** traces to be attempted — it never creates
false edges. To determine whether any future map has pairs beyond 160u: run
`cargo run -p qbots -- nav-debug <map>` and look for `dz=N > STAIR_MAX` lines.

## Sources
- qbots: `crates/world/src/navgraph.rs` (`STAIR_MAX`, `bridge_pass`, `walkable_stair`)
- qbots: `context/map_errors.notes.log.md` (q2dm3 session 2 analysis)

---

# BackOffThenRepath nav-override: recovery backward motion silently cancelled

## Problem

In `scenario.rs`, the recovery action match runs BEFORE the forward-nav block:

```rust
match rec_action {
    RecoveryAction::BackOffThenRepath => { mv.move_forward(-0.5); nav.force_replan(); }
    ...
}
// ⚠️ This runs AFTER the match and overwrites -0.5 with positive fwd:
if fwd > 0.0 || side.abs() > 0.0 {
    mv.move_forward(fwd * arrive);  // ← cancels the backward motion
}
```

`mv.move_forward()` is a setter (not an adder), so the last call wins. Because the
nav-forward block comes AFTER the recovery match, BackOffThenRepath never actually
moved the bot backward — the bot stayed glued to the wall, triggering BackOff again
5 seconds later in an endless loop. `main.rs` doesn't have this bug because its nav
block is BEFORE the recovery match (correct order), but scenario.rs was written with
the opposite ordering and the bug went unnoticed since symptom looks like "general
stuck" rather than "recovery not working."

## Fix / How to avoid

In scenario.rs, add a `backoff_ticks` counter. When `BackOffThenRepath` fires, set
`backoff_ticks = 8` (≈0.8 s). In the nav-forward block, gate on `!backing_off`:

```rust
let backing_off = backoff_ticks > 0;
if backing_off { backoff_ticks -= 1; mv.move_forward(-1.0); }
else if fwd > 0.0 { mv.move_forward(fwd * arrive); ... }
```

This ensures the bot sustains backward motion for ~0.8 s before resuming forward nav.
Whenever you add a recovery action that sets movement, ensure the nav-motion block
either runs BEFORE (so recovery can override it) or is gated to skip during recovery.

## Sources
- qbots: `crates/qbots/src/scenario.rs` (BackOffThenRepath match arm + nav-forward block)
- qbots: `crates/qbots/src/main.rs` (correct ordering — nav fwd set before recovery match)

---

# GOAL_GIVEUP infinite loop: giveup fires → replans same blocked path → loops

## Problem

When a bot is stuck at waypoint N for too long (GOAL_GIVEUP_TICKS), the first
implementation:
1. Cleared `current_path`, `current_waypoint`, `last_goal_node`
2. Called A* again on the next tick with the same nav graph → same path → same waypoint N → fires again in 4 seconds → infinite loop

The bot oscillated between: stuck at N → giveup → replan → same N → stuck → repeat.
Each cycle wasted 4-8 seconds (GOAL_GIVEUP_TICKS × tick_dt). With 30+ waypoints
ahead in the path, if the first one is blocked by geometry or another player, the
bot never makes progress. Symptom: bot remains stationary for the entire 60s test,
giveup fires ~8 times per minute, speed=0, endless `goal give-up: replanning` logs.

## Fix / How to avoid

On giveup, push the stuck waypoint index into a `waypoint_blacklist: VecDeque<usize>`
(max 8 entries). Then use `path_excluding()` (A* with 1e6 penalty on blacklisted
nodes) so the next plan avoids the same node. Clear the blacklist ONLY when the goal
is successfully reached (not on force_replan or giveup — those must preserve the
blacklist so alternatives accumulate). GOAL_GIVEUP_TICKS was also reduced 80→30 so
each reroute attempt costs only 3s instead of 8s.

## Sources
- qbots: `crates/brain/src/nav.rs` (`GOAL_GIVEUP_TICKS`, `waypoint_blacklist`, `plan_path`, `force_replan`)
- qbots: `crates/world/src/navgraph.rs` (`path_excluding`, `path_inner`)

---

# False walk-edge: open staircase interior passes walkable_stair

## Problem

`walkable_stair` moves UP then FORWARD in STEP-sized increments using hull traces.
In Q2, staircase volumes are hollow (open air between tread and ceiling). Two nav
nodes on DIFFERENT FLOOR LEVELS (e.g. z=792 and z=912) that happen to be adjacent
in the XY grid (64 u apart) pass `walkable_stair` because all vertical/horizontal
traces go through open staircase air — no wall is ever hit. This creates a false
bidirectional walk edge. The bot then targets the upper node, walks horizontally
toward it at the lower level, reaches the platform edge, and falls off.

Symptom: orbit-timeout fires with large dz (e.g. dz=127.9, horiz=33). Bot cycles
endlessly: navigate to false upper waypoint → fall off ledge → renavigate → repeat.

## Fix / How to avoid

Four independent guards collectively reduce false edges:

1. **seed_spawns SEED_MAX_DZ=54**: limit z-connections to ≤3×STEP=54u when seeding
   goal/weapon nodes. Cross-floor connections via seed are invalid.
2. **smooth_path MAX_SMOOTH_DZ=48**: cap apex↔candidate dz at 48u to preserve
   staircase node sequences during path smoothing.
3. **BRIDGE_HDIST=128** (was 512): adjacent grid cells are ≤64√2≈90u apart.
   128u covers single-cell staircase gaps while blocking cross-floor false bridges
   (observed hdist 146–510u). Reduced from 512 via 192; 128 leaves only 3 edges
   (dz=112, hdist=120-122, slope≈0.93 — real connections).
4. **walkable_stair floor-existence check**: at each stair step, probe downward
   STEP×2=36u. A real tread is ≤24u below the bot's origin (found at fraction≈0.67).
   A false open-air connection has its nearest floor at the lower endpoint, > 36u
   below at intermediate steps → fraction=1.0 → edge rejected. This is the most
   effective single fix: improved 32-bot 120s reach from 10→13 bots.

### Approaches that were tried but DON'T WORK:
- **Slope guard** (dz/hdist > threshold): rejects legitimate steep staircase edges,
  breaking connectivity for areas where only steep connections exist. 11→6 regression.
- **Midpoint floor probe**: works for straight edges but breaks winding staircases
  where the path midpoint XY is in open air away from actual tread geometry.
- **Cost penalty on high-dz edges**: penalty affects BOTH real paths and false ones,
  degrading working paths. The 2 reaching bots dropped to 0. Reverted.
- **GRID_SPACING=12**: 4x more nodes, 15x slower generation (492s vs 31s), no
  significant improvement (1/8 same as baseline on 8-bot test).

### Orbit-timeout discriminant (fell-off-ledge vs false-bridge):
When orbit-timeout fires with dz > LEDGE_DZ=96u, check edge_dz = |prev_z − wp_z|:
- edge_dz > LEDGE_DZ: the NAV EDGE itself goes steeply upward → FALSE BRIDGE →
  blacklist the target node and replan.
- edge_dz ≤ LEDGE_DZ: the nav edge is flat (both nodes at same z) → bot FELL OFF
  LEDGE while navigating → skip forward in the remaining path to the first node near
  the bot's current z (WP_REACH_DZ×3=72u tolerance).

## Sources
- qbots: `crates/world/src/navgraph.rs` (`walkable_stair`, `seed_spawns`, `smooth_path`, `bridge_pass`)
- qbots: `crates/brain/src/nav.rs` (orbit-timeout discriminant, ledge_blacklist)
- qbots: `crates/world/src/build.rs` (BRIDGE_HDIST)

---

# smooth_with_cm point-trace creates 600u+ platform shortcuts → ledge falls

## Problem

`smooth_path` uses a POINT TRACE (zero-size box) to test LOS between nodes. On the
z=920 platform in q2dm1, the open-air trace from one end to the other (600u+) succeeds
because the trace passes through z=920 AIR above the ledge geometry. The bot gets a
waypoint 600u away, races at 300u/s (2 seconds), overshoots the platform edge, and falls.
Symptom: bot from spawn[5] commits a smooth 605u first waypoint and falls off ledge at t=28s.

Using a HULL TRACE instead was tried: hull top at z=952 hits ceiling geometry in narrow
areas (startsolid=true), preventing ALL shortcuts on the z=920 platform → worse navigation.

## Fix / How to avoid

Cap MAX_SMOOTH_HDIST=120u in smooth_path. The cap is checked BEFORE the trace:
if hdist from apex to candidate > 120u, break. This limits shortcuts to 5 grid cells
(120u at 24u spacing), preventing 600u+ dangerous shortcuts while still allowing useful
corner-cutting within 120u. Tests that used 100u spacing needed updating to 50u spacing
(so 2 nodes fit within the 120u cap from apex; see `smooth_path_straight_run_collapses`).

## Sources
- qbots: `crates/world/src/navgraph.rs` (`smooth_path`, `MAX_SMOOTH_HDIST`)

---

# Above-waypoint orbit: bot climbs slope-roof, force-advances off platform

## Problem

On q2dm1 near node 8694 (1351,1215,920), a bot approaching from the south hits a slope
at y≈1140 that pushes it UP to z=1006 (onto the roof geometry). The bot is now ABOVE
the waypoint (dz=86u < LEDGE_DZ=96u threshold), so the old orbit code treated it as a
"normal force-advance" → advanced to next node (SE direction) → bot ran off the platform
edge. Symptom: bot from spawn[6] reaches z=920, climbs to z=1006, falls.

## Fix / How to avoid

In the orbit-timeout handler, ADD A CHECK before the dz > LEDGE_DZ branch:
if `position.z > wp_z + LEDGE_DZ` (bot is significantly ABOVE the waypoint), force
an immediate replan instead of force-advancing. The bot at z=1006 trying to reach
a node at z=920 needs a new A* path from its current elevated position, not a push
to the next waypoint in a direction that leads off the edge.

## Sources
- qbots: `crates/brain/src/nav.rs` (`orbit-timeout: bot above waypoint — replanning`)

---

# ORBIT_RADIUS=80u fires for bots navigating corners → wrong force-advance direction

## Problem

With ORBIT_RADIUS=80u, the orbit timeout fires when a bot is within 80u of a waypoint.
A bot at (1357, 1136) navigating to node 8694 at (1351, 1215) is 85u away — just
barely outside the radius. But after any position jitter, it enters the 80u zone
(horiz=79u) and orbit fires after 1.5s. The "normal" force-advance sends it to the
NEXT node in the path (SE direction) which is wrong for navigating around the corner.
High wrong_turns (50-72) and poor path efficiency indicate premature force-advances.

## Fix / How to avoid

Reduce ORBIT_RADIUS from 80u to 48u (2 grid cells). At 48u, the orbit only fires when
the bot is genuinely unable to reach a very close waypoint — not when navigating a
corner. Let StuckLevel::Hard (5 seconds of stuck) handle corner navigation via
BackOffThenRepath which does a full replan. Also reduces false orbit timeouts for
bots correctly navigating around adjacent-grid walls.

## Sources
- qbots: `crates/brain/src/nav.rs` (`ORBIT_RADIUS = 48.0`)

---

# WP_REACH_HORIZ=16u too tight: 300u/s bots overshoot, accumulate wrong_turns

## Problem

At 300u/s (30u/frame at 10Hz), a bot overshoots a 16u-radius waypoint every tick unless
it decelerates. Q2's actual pmove doesn't decelerate instantly (friction takes ~3 ticks).
The bot passes through the waypoint but doesn't register "reached" (horiz=18u > 16u),
continues forward, and the recorder logs a wrong_turn (moved AWAY from waypoint).
With dozens of waypoints per path, each overshoot accumulates wrong_turns and wastes time.

## Fix / How to avoid

Increase WP_REACH_HORIZ to 24u (one grid cell = one Q2 unit of nav resolution). At
24u radius, a bot traveling 30u/frame registers "reached" when it's within one step of
the waypoint. Setting it to 32u was tried but caused pathological skips (bot skipped
waypoints near wall edges, ended up in wrong areas). 24u is the sweet spot.

## Sources
- qbots: `crates/brain/src/nav.rs` (`WP_REACH_HORIZ = 24.0`)

---

# BackOffThenRepath waypoint blacklisting: too aggressive → valid nodes blacklisted

## Problem

Adding `force_replan_with_blacklist()` to BackOffThenRepath (blacklist current waypoint
unconditionally on every stuck recovery) worked for spawn-to-spawn (5-8/8) but gave
0/8 on spawn-to-weapon. The weapon goal has ONE efficient route through z=920 platform
nodes. With HARD_REPATH_SECS=3s and up to 20 replans in 60s, the blacklist of 8 nodes
filled with critical route waypoints. A* was forced to take absurd detours or fail.

HARD_REPATH_SECS=3s also caused 0/8 on its own (too many replans even without blacklist).

## Fix / How to avoid

Only blacklist a waypoint on BackOffThenRepath if a HULL TRACE from current position
to the waypoint confirms it's physically blocked (fraction < 0.9). Call
`blacklist_waypoint_if_blocked(pos, &cm)` before `force_replan()`. Keep
HARD_REPATH_SECS=5.0 (matching Eraser's reference). The hull trace correctly identifies
walls between bot and waypoint (not just corner-stuck bots that ARE making progress).

## Sources
- qbots: `crates/brain/src/nav.rs` (`blacklist_waypoint_if_blocked`)
- qbots: `crates/qbots/src/scenario.rs` (BackOffThenRepath handler)
- qbots: `crates/brain/src/recover.rs` (`HARD_REPATH_SECS`)

---

# Orbit/giveup boundary oscillation — bot stuck at orbit threshold

## Problem

When a bot's horizontal distance to a waypoint oscillates around the `ORBIT_RADIUS`
boundary (e.g., 47u ↔ 52u with `ORBIT_RADIUS=48u`), two timers fight each other:

- The **orbit** mechanism resets `goal_age_ticks = 0` on EVERY tick where `horiz < ORBIT_RADIUS`.
- The **giveup** mechanism needs `goal_age_ticks > GOAL_GIVEUP_TICKS` (15 continuous ticks
  of `horiz >= ORBIT_RADIUS`) to fire.

If the bot dips below 48u for even 1 tick per cycle, giveup resets. Neither giveup (need
15 continuous far-ticks) nor orbit (need 25 continuous near-ticks) fires. The bot sits stuck
at the boundary until the BackOff StuckDetector fires at 3.5s (much later).

Observed in: q2dm1, z=472 staircase area. Bot at (917,723,472) stuck 3.2s because wpd
oscillated 47↔53u. The orbit-boundary reset consumed 21s of 60s budget across multiple
waypoints in debug traces.

## Fix / How to avoid

Only reset `goal_age_ticks` when `near_wp_ticks >= ORBIT_ENTRY_MIN (3)` — i.e., the bot
has been CONTINUOUSLY inside orbit range for 3+ ticks. A brief 1-2 tick dip below
`ORBIT_RADIUS` (boundary oscillation) does not reset the giveup timer. This lets giveup fire
in ~1.5s even when the bot occasionally touches the orbit boundary.

```rust
const ORBIT_ENTRY_MIN: u32 = 3;
if horiz < ORBIT_RADIUS {
    self.near_wp_ticks += 1;
    if self.near_wp_ticks >= ORBIT_ENTRY_MIN {
        self.goal_age_ticks = 0; // sustained orbit entry: orbit owns this
    }
    ...
}
```

## Sources
- qbots: `crates/brain/src/nav.rs` (orbit watchdog, `ORBIT_ENTRY_MIN`)
- qbots: `context/map_errors.notes.log.md` (2026-06-18 Session 4 analysis)

---

# func_plat elevator deadlock — bots hold the lift up and starve the queue

## Problem

Observed live: many bots pile onto a q2dm1 elevator pad; the platform stays UP and
never returns, so everyone queued at the bottom waits forever. The moment bots
disconnect, it descends.

Ground truth in `vendor/yquake2/src/game/g_func.c`:
- A `func_plat` with no `targetname` (the q2dm1 lift) rests at the BOTTOM (`STATE_BOTTOM`).
- `plat_spawn_inside_trigger` builds a trigger that spans the WHOLE shaft.
- `Touch_Plat_Center`: if the plat is at `STATE_BOTTOM` and a player touches the trigger
  → `plat_go_up` (rides up). If the plat is at `STATE_TOP` and a player is still in the
  trigger → `nextthink = level.time + 1`, i.e. it **keeps resetting its go-down timer**.
- `plat_hit_top` would otherwise auto-descend after 3 s.

So a bot that rides up and **lingers on the pad** (in the shaft trigger) pins the plat at
the top indefinitely; bottom bots can never summon it. Our nav makes this worse:
`build.rs::add_lift` places the lift's top nav node at the plat's center-top — bots steer
straight to a node *on the pad* and dwell there, and bot-on-bot collisions stop them
stepping off.

## Fix / How to avoid

Proper handling (wait at bottom for the plat, ride, step off promptly, read plat z from
entity frames) is a real feature and unbuilt. Interim fix: `add_lift` adds the vertical
ride edge with `+ELEVATOR_PENALTY (5000u)` cost, so A* routes around the lift via any
stair/ramp whenever one exists; the edge stays finite so genuinely lift-only spawns still
resolve. This removes the lift from the common path → far fewer bots crowd it → no mass
deadlock. Also run scenarios at 24 bots (not 32) to cut pad crowding while keeping signal.
Future: model the plat state machine and add wait/ride/step-off + a "don't pile on a busy
lift; back off and re-approach" behaviour (Eraser-style).

## Sources
- qbots: `crates/world/src/build.rs` (`add_lift`, `ELEVATOR_PENALTY`)
- vendor: `yquake2/src/game/g_func.c` (`Touch_Plat_Center`, `plat_hit_top`, `SP_func_plat`)

# qport collision across processes — concurrent fleets never spawn

## Problem

Two `qbots` processes on the **same host** (e.g. `run --name navmesh --count 8` and
`run --name astar --count 8`, to battle the two nav backends) leave many bots stuck in
`state=Connecting`, never spawning — even though 64 bots from a *single* binary work fine
and the server has free slots. Not a UDP-throughput or capacity issue.

Cause: the Q2 server matches an inbound packet to a client slot by **base IP + the 16-bit
qport only** — it deliberately ignores the UDP source port so a client survives NAT
remapping (`sv_main.c:225-238`: `NET_CompareBaseAdr` compares the address *without* port,
then `cl->netchan.qport != qport` is the sole other discriminator; on a source-port
mismatch it logs `fixing up a translated port` and *reassigns* the slot's port). Both
fleets read the same `config.yaml` and assigned qports from a fixed base (`28000..`), so
`navmesh_1` and `astar_1` both claimed slot `(host-IP, 28000)`. The server flip-flopped
that one slot's port on every packet → neither connection ever stabilized.

## Fix / How to avoid

Give each process a **disjoint qport range** — the ranges, not just the bases, must not
overlap, since the fleet hands out `base + i`. `run_fleet` defaults the base to
`default_fleet_qport_base()` = `(pid & 0xFF) << 8`, which spaces processes **256 apart** by
PID low byte: two fleets launched back-to-back get consecutive PIDs whose low bytes differ
by 1 → bases 256 apart → disjoint for any sane fleet size. (A naïve `pid & 0xFFFF` base is
*wrong* here: consecutive PIDs differ by 1, so `count`>1 ranges overlap and one fleet
starves.) `run --qport-base <u16>` pins it for reproducibility. The server's
`fixing up a translated port` spam is the tell that two clients share an `(ip, qport)`.

## Sources
- qbots: `crates/qbots/src/supervisor.rs` (`run_fleet`), `crates/qbots/src/main.rs` (`default_qport`)
- vendor: `yquake2/src/server/sv_main.c:225` (`SV_ReadPackets` qport/base-addr match)

# OOB `status` map key is `mapname`, not `map`
Map autodetection (`status.rs`, used by `spawn-to-*` / `connect` to pick the
server's current map) silently returned `map = None`, logging
`server status reply carried no map; pass --map to override`. Root cause: the
parser read the serverinfo key `map`, but a real Yamagi/q2pro server publishes the
level under `mapname` — `Cvar_FullSet("mapname", sv.name, CVAR_SERVERINFO|CVAR_NOSET)`
(`vendor/yquake2/src/server/sv_init.c:366`). The bug hid for a long time because the
unit-test fixtures were hand-built with `\map\q2dm1\` (the *wrong* key), so they
matched the bug instead of a real reply.

Avoid: when parsing a protocol field, build test fixtures from a *captured real
packet*, never from your own assumption of the format — a fixture that encodes the
same wrong assumption as the parser is a self-confirming bug. For Q2 serverinfo keys,
grep `vendor/yquake2/src` for `CVAR_SERVERINFO` to see exactly what the server
advertises. Fix kept `map` as a fallback but reads `mapname` first.

Related: missing/unreachable nav data must be FATAL, not a `warn!`+continue. The
combat `run` path (`supervisor.rs::build_map_nav`) used to log a warning and run bots
with no nav on a cache miss — bots flailed silently. Now it `std::process::exit(1)`,
matching the scenario path and the connectivity check.
## Sources
- qbots: `crates/qbots/src/status.rs` (`parse_status_body`), `crates/qbots/src/supervisor.rs` (`build_map_nav`)
- vendor: `yquake2/src/server/sv_init.c:366` (`mapname` CVAR_SERVERINFO)

# Water positions discarded → water-only routes appear unreachable (q2dm1 railgun)
Nav graph generation dropped EVERY position inside a water volume (`navgraph.rs`
`floor_waypoints_multi` skipped `point_contents & MASK_WATER`; the navmesh heightfield
did the same). On q2dm1 the railgun room (`weapon_railgun` at `240 -384 464`) is reachable
ONLY by jumping into water, swimming a submerged tunnel, and surfacing — so with water
excluded, the railgun's dry floor was an ISOLATED component and A* returned NO PATH from
every spawn. This violated the project invariant "all q2dm spawns/items are mutually
reachable": the map was fine, our world model had no representation of water.

Fix (Plan 39): sample submerged + surface swim nodes per water column, connect them with
3-D `EdgeKind::Swim` edges (no STEP/STAIR gate, cost ×2 for slow water move), and add
dry↔water entry/exit edges — the water-surface→dry-ledge EXIT edge is what fuses the
railgun room into the main component. Cap each water edge's |dz| (`WATER_VLINK`=48) so one
edge can't span a whole pool, and validate with a REDUCED hull (`±12`) — the full standing
hull is too strict for tight tunnels. Sample only `CONTENTS_WATER` (never lava/slime).
Protect swim edges from `prune_long_blocked_edges` (a walk/stair hull trace falsely flags
the 3-D vertical link). Then the brain must actually swim it (Plan 40, below).

Diagnose with `navinspect <baseq2> <map> gpath <sx sy sz> <gx gy gz>` (A*-graph path, not
the navmesh `navpath`) + `contents`/`watermap`.
## Sources
- world: `crates/world/src/navgraph.rs` (`water_waypoints_multi`, `try_swim_edge`, `generate`)
- world: `crates/world/src/mapcache.rs` (v13: swim edges + water tags serialized)
- vendor: `yquake2/src/common/pmove.c` (`PM_WaterMove`, `PM_CategorizePosition`)

# No brain set intent.up; recovery avoided water → swim edges weren't followed (Plan 40)
Even after Plan 39 put swim nodes/edges in the graph, bots couldn't traverse them: NO brain
ever set `intent.up` (only `jump` touched `upmove`, a one-shot 270 launch — useless for
sustained swimming), and stuck-recovery's `find_best_direction` actively `continue`s past any
water endpoint while the 4u/1s `StuckDetector` false-fires on slow (0.5×) water movement.

Fix: recompute waterlevel ourselves (`brain::water::water_level`, mirrors
`PM_CategorizePosition` — it's NOT on the wire) since we own the `CollisionModel`+origin; on a
swim edge / when `waterlevel>=2`, set `intent.up = clamp(dz/32,-1,1)` (sustained, NOT
`mv.jump()`) and pitch toward the 3-D target so `pml.forward` carries the vertical component.
For the climb-out onto a ledge, trigger Q2's water-jump: look up (`pitch<=-15`,
`pmove.c:414`) + forward + hold up, with a few-tick hysteresis so the bot doesn't oscillate
at the lip. SUSPEND recovery entirely while swimming. Result: q2dm1
`spawn-to-weapon railgun --navmode astar` reaches in ~11 s (46/93 frames `S`-flagged,
z 238→434 = dive→surface). Pure-navmesh modes still fail (navmesh has no water — by design).
## Sources
- brain: `crates/brain/src/water.rs`, `crates/brain/src/brains/runtester.rs` (+ `main.rs`)
- vendor: `yquake2/src/common/pmove.c:414` (`PM_CheckSpecialMovement` water-jump), `:545` (`PM_WaterMove`)

# Inline-model (func_train/func_plat) wire origin is `corner - mins`, not the stand-center
Detecting a moving platform from a bot client is subtle. A brush model entity (`*N`:
func_train/func_plat/func_door) is sent over the wire with `entity_state.origin` equal to its
**render origin**, which Q2's `train_next` sets to `path_corner.origin - model.mins`
(`g_func.c:2310`; "the targets origin specifies the min point of the train"). That is NOT the
platform's standable top-center — for q2dm3's `*3` it's near `(-32,0,8)`, hundreds of units from
the board ledge `(768,624,~40)`. So a brain that looks for "an entity near the board point" never
matches and waits forever. Fix: at nav-build time, store the platform's expected wire origin per
corner (`corner - model.mins`) in the ride edge (`RideInfo::board_ent`/`far_ent`) and match the
live PVS entity against THAT. Also note inline models are absent from the CollisionModel (only
world model 0), so you cannot trace the platform's surface — detection must be entity-based.
Second pitfall: a func_train's standable top-center is `corner.xy + size.xy/2`,
`corner.z + size.z` (min-corner rule), but that point is open air over a pit whenever the train
isn't there — anchor the bot's board/dismount to an existing solid-ground nav node near it
(`nearest_ground`), or the bot walks off the ledge into the pit while merely approaching.
## Sources
- world: `crates/world/src/build.rs` (`try_add_train`, `add_lift`, `nearest_ground`), `navgraph.rs` (`RideInfo`)
- brain: `crates/brain/src/ride.rs` (`platform_present`)
- vendor: `yquake2/src/game/g_func.c:2155-2362` (func_train / train_next / path_corner)

# func_train deck height ≠ brush bbox top: MEASURE, and don't trust EITHER assumption (q2dm3 *10)

**Problem.** Spent ~60 brain-tuning iterations failing to ride q2dm3's central `func_train *10` to
the quad. Guessed the deck height repeatedly. Two WRONG guesses, both refuted only by measuring +
asking the human who plays the map:
  1. First the nav used **corner-level** (z≈208) — which is actually CORRECT for *10 — but the
     brain's ride tracking + board/dismount were broken, so it looked wrong.
  2. Then, after measuring the brush bbox (`maxs.z=401`), wrongly concluded the deck was the bbox
     **TOP z≈410** and deleted the corner-level edge — also wrong.

**Measured truth.** (a) `QBOTS_OBSERVE_MOVERS=1 connect-one` logs live `*N` movers: they arrive
with **modelindex=0** (our delta-decode drops it) and `EntityClass::Unknown`, so match movers by
POSITION (`corner - mins`), not modelindex. (b) `*10` wire origin ≈ `[0,Y,0]` (confirms
`corner-mins`), Y oscillates 0..384 (corners t1 y−296 ↔ t2 y88). (c) model bbox z175..401 — but
that is a **DECK at z≈216 with TALL RAILS up to z410**, NOT a solid box. The human's route confirms:
board the deck from the **z216 spawn ledge** at the FAR corner (t1), **sit still** (Q2 trains PUSH
riders — zero input carries you; verified z holds at 216), then **jump off near the near corner onto
the quad ledge z224**. So *10's real ride edge is **board z216 → quad z224** (corner-level), and the
bbox-top z424 ledges are the WRONG anchor.

**Avoid.** (1) The standable deck is NOT derivable from the bbox alone — a func_train can be a deck
with rails (bbox top ≫ deck). `*3/*4` deck = bbox top (z9); `*10` deck = corner level (z216). Keep
the two-height search (try corner AND top, keep whichever finds adjacent ground). (2) When unsure of
a map's geometry, ASK the human who plays it — one sentence ("board at the far point, sit still, jump
off near the quad") beats 60 guesses. (3) Verify ride physics by a live zero-input capture before
tuning board/carry/dismount.
## Sources
- qbots: world/src/build.rs (try_add_train two-height search), brain/src/ride.rs, brain/src/brains/runtester.rs

# External-client bot "loses despite perfect aim": look at fire cadence, strafe period, and loadout
When tuning `main` to beat `q3`-major in the deathmatch competition (Plan 45), several
"obvious" combat fixes did NOT move the K/D, and the real levers were unintuitive. A bot with
*perfect hitscan aim and a faster reaction than the opponent* can still get farmed. Debug it by
logging OBITUARY prints (`svc_print` names the victim + killer or `None`=self/env), not by
staring at the scoreboard — the death cause is the signal.

What actually starved `main`'s damage / inflated its deaths:
1. **Fire lockouts stack.** Eraser's 0.9 s weapon-switch lockout + a 0.4 s reaction reset =
   ~1.3 s idle at the START of every engagement; the opponent (q3, 0.1 s + 0.3 s) shot first
   every time. Trim `SWITCH_LOCKOUT_SECS` (main+sentry `combat.rs` only, q3 has its own model).
2. **A long circle-strafe period is a straight line.** `STRAFE_PERIOD_SECS = 3.0 s` meant `main`
   slid predictably for 3 s — trivial to LEAD. Dropping to 0.6 s (a real juke) cut deaths 38→25.
   But randomizing the leg to [0.3,0.9]s was WORSE: legs <~0.4 s just vibrate in place (net-zero
   displacement = no dodge). Keep a fixed, ground-covering juke.
3. **Respawn-loadout death spiral.** die → respawn Blaster → lose the projectile duel → die.
   Fix with a loadout-gated posture: Blaster ⇒ evade + go grab a real gun; hitscan ⇒ stand & fight.

Avoid: (a) don't add combat JUMP-jink vs a hitscan opponent — it leads the predictable hop and
tanks your own kills. (b) Widening acquire-FOV / holding long range / full run-to-item retreat all
plateaued ~0.5 (they cut kills ~1:1 with deaths). (c) Positioning/strategy cannot close a
per-engagement combat-quality gap; if aim+reaction are already maxed, the remaining gap is
combat strength, not tactics — say so instead of grinding neutral tweaks.
## Sources
- qbots: brain/src/combat.rs (SWITCH_LOCKOUT_SECS, reaction base), brain/src/steer.rs
  (STRAFE_PERIOD_SECS), brain/src/brains/main.rs (weapon-rush kite/flee_hard), Plan 45 tracker

# Traversal extraction: movement is view-relative, so an executor must own BOTH
When extracting a shared traversal executor (Plan 46) from per-brain swim/ride/ladder copies, the
subtle trap is that `move_from_world_dir(dir, view_yaw, …)` converts a WORLD direction to view-
relative `forward`/`side` using the current view yaw. So you CANNOT let the brain keep aiming at an
enemy while the executor sets forward/side — the movement would push in the wrong world direction.
The executor must own the VIEW as well as the movement axes while a traversal is active; the brain
keeps only the fire button (the bot fires along the traversal heading). Trying to preserve combat
aim during a ride/swim silently breaks the movement direction.

Second trap: the three per-brain copies had DRIFTED — `main`'s swim was vertical-only (reused the
brain's horizontal steer) while `runtester`'s was self-contained, and `main`'s ride was stateless
while `runtester`'s had the board/carry lock. Lift the SELF-CONTAINED copy (runtester's) even if a
plan note says otherwise, and make it the regression anchor (must byte-preserve); the partial copies
then *gain* capability. Verify the anchor live BEFORE adopting it into the other brains.

Third: recovery must be gated OFF *before* it runs (it has side effects — `force_replan`,
`blacklist_waypoint`), so the executor exposes a cheap `gates()` the brain calls before recovery,
separate from `apply()` which overrides movement after. Confirm zero `R` (recovery) recorder frames
overlap `P`/`S`/`L` frames — that's the proof the gate holds.
## Sources
- qbots: brain/src/traverse.rs (TraversalExecutor), brains/{runtester,main,q3}.rs (Plan 46)

# Combat A/B on single competition runs is noise, not signal
Tuning bot combat behavior by running ONE 5-min competition and reading the K/D scoreboard is a
trap: the variance across runs dwarfs the effect you're measuring. Concrete evidence (Plan 30,
q2dm1, 3v3 main-vs-q3, q3 as an UNCHANGED control): q3's K/D came out 1.00, then 0.86, then 2.60 on
three consecutive runs of identical code. Any main-side change looked like a 0.69→0.50→0.28 "trend"
that was pure sampling noise (spawn draws, encounter luck, who-got-the-quad). We nearly reverted a
principled change based on one run.
How to avoid: never conclude a behavior helps/hurts from a single competition. Either (a) run many
samples and average (the Plan 47 acceptance harness is built for this), (b) use much longer runs /
more bots to accumulate encounters, or (c) judge changes on PRINCIPLE + deterministic unit tests and
only gate the *aggregate* behavior in a repeatable suite. Keep an unchanged control group in every
run so the variance is visible — if the control swings wildly, discard the comparison.
## Sources
- qbots: brains/main.rs (Plan 30 resource decisions), context/mode_perf.md (variance note)

# Enemy weapon is NOT inferable from modelindex2 on stock yquake2 (VWep sentinel)
Plan 28 planned to read an enemy's held weapon from the player entity's `modelindex2` (the VWep
third-person wield model, resolvable via CS_MODELS like our own `gunindex`→view-model). Live
verification (QBOTS_P28_DEBUG, 2026-07-10) killed that: EVERY player entity carries
`modelindex2 = 255` — a sentinel (CS_MODELS slot 255 is empty), not a per-weapon index. yquake2's
client resolves a player's weapon model from `ci->weaponmodel[]` (per-clientinfo, `cl_parse.c:1070`)
indexed by modelindex2, and stock gamecode does not vary it per weapon over the wire here. So the
enemy's weapon is simply not on the wire on this server.
How to avoid: don't build combat behavior that REQUIRES the enemy's weapon — gate it on
`Option<Weapon>` and fall back cleanly (Plan 28's `from_wield_model` + `PerceivedEntity.held_weapon`
are correct + unit-tested and will light up on a VWep-per-weapon server, but stay `None` here).
Positioning by YOUR OWN weapon's ideal range (known via `gunindex`) is the enemy-independent,
always-available half — build that. Fallbacks if you truly need enemy weapon: infer from observed
projectile entities (rocket/grenade models) or muzzle/fire sounds, never guess.
## Sources
- qbots: brain/src/{weapons.rs from_wield_model, perception.rs held_weapon}, qbots/src/main.rs (QBOTS_P28_DEBUG)

# func_plat elevator deadlock — CLOSED (Plan 31; history from elevator_todo.md)
The debt item `context/elevator_todo.md` is retired. The bug it tracked: Q2's `Touch_Plat_Center`
resets the plat's go-down timer EVERY tick a body is anywhere in the shaft trigger (`g_func.c`) —
so bots crowding a lift (riders dwelling on top, queuers standing at the bottom trigger) pinned it
and starved everyone. The interim hack was `ELEVATOR_PENALTY` (5000u A* cost on every lift edge,
`--lift-penalty` knob) so A* avoided lifts whenever stairs existed.
Resolution (Plan 31): the shared TraversalExecutor's vertical branch is a 3-phase machine —
**WaitClear** at a standoff OUTSIDE the trigger while the shaft is occupied or the pad is visibly
away (waiting bots no longer pin a raised plat; blind timeout ~4s because PVS can hide both
signals), **Enter** (hop on; if the pad hasn't lifted us in ~5s someone unseen is pinning it →
back off, which is exactly what lets it descend), **BackOff** (jittered 2–4s from the bot's own
standoff spot — deterministic jitter breaks two-bot yield-loop symmetry). Route continuation walks
riders off the pad at the top. The hack, its flag, and its cache-fingerprint field are deleted
(cache v20); lift edges carry honest travel cost.
Detection notes for future mover work: a func_plat's wire origin is [0,0,0] at TOP (ambiguous with
null world entities) and (0,0,-travel) at BOTTOM — only the lowered state is detectable; design
around "can't see it → wait clear, then timeout in".
## Sources
- qbots: brain/src/{ride.rs shaft_occupied/plat_at_bottom, traverse.rs LiftPhase}, world/src/build.rs add_lift

---

# MASK_SOLID floor probes see a lava bed as walkable floor

## Problem

Every "is there ground here?" check that traces down with `MASK_SOLID` only
(`segment_has_floor`, node sampling's floor probe) passes straight *through*
liquids and stops on the solid brush underneath. A shallow lava pool therefore
reads exactly like a walkable floor: the trace hits the lava **bed** within the
probe depth, `fraction < 1.0`, "floor found". On q2dm3 this let the corner-cut
guard (`pursue_target_safe`) approve straight-line shortcuts across the central
lava, and let `floor_waypoints_multi` place "dry" nodes hovering over pools
shallower than 24 u (the node's own water check samples 24 u up). Bots died in
lava constantly and it looked like a steering bug, not a contents bug.

## Fix

After any down-trace floor hit, check `point_contents(endpos + 1u above)` for
`CONTENTS_LAVA | CONTENTS_SLIME` — a deadly-covered floor is NOT support. Also
reject path samples whose own contents are lava/slime (the line passes through
the volume). Keep plain `CONTENTS_WATER` walkable — shallow water is legal.
Regression tests self-locate lava by scanning contents columns (never hard-code
coordinates; interior floors hide lava from a single top-down trace, so scan
point contents, not traces). Plan 48; cache v21.

## Sources

- qbots: `world/src/navgraph.rs` (`segment_has_floor`, `floor_waypoints_multi`, `floor_is_deadly`)
- qbots: `world/tests/lava_q2dm3.rs`

---

# Combat/dodge/recovery world-dirs bypass every nav-graph guarantee

## Problem

Path following is validated (graph edges, hull traces, floor probes) — but
combat movement is not on the path. Circle-strafe, backpedal-from-enemy, kite,
projectile dodge, and stuck-recovery side-steps all emit **enemy-relative or
arbitrary world directions** that no walkability machinery ever sees. On q2dm3
the standard death was backpedaling away from an enemy straight into the lava
while aiming at them. Separately, `main`/`q3` steered at the RAW
`pursue_target` look-ahead for months — `pursue_target_safe` existed (hull +
floor validation) but only `runtester` called it, so even path following could
cut a corner over a pool.

## Fix

One shared probe (`brain::hazard`): `dir_is_hazardous` samples 24/48 u ahead
(lifted STEPSIZE), down-probes 128 u, and flags lava/slime floors and blind
drops (walls are NOT hazards — they stop the bot). Every non-path world dir
goes through it: keep the dir, mirror the strafe/tangential component, else
stand and fight (standing beats swimming in lava). And every brain reads ONE
`pursue_target_safe` point per tick for path steering. When adding a new
movement source, ask: "did this direction come from the graph?" If not, gate it.

## Sources

- qbots: `brain/src/hazard.rs`
- qbots: `brain/src/brains/main.rs`, `brain/src/brains/q3/mod.rs` (attack_move gate), `brain/src/brains/zb2.rs`

---

# A brain's fallback branch must still aim, fire, and steer

## Problem

zb2's no-route branch (route planning failed, e.g. off-graph after a fall) did
NOTHING when an enemy was visible — no aim, no attack, no movement (a frozen
target) — and blind-ran `move_forward(1.0)` with zero steering otherwise,
grinding walls forever. Combined with its hard-stuck replan recommitting the
IDENTICAL polyline (committed routes have no waypoint blacklist), this was the
"zb2 runs into walls instead of engaging" symptom. Fallback/edge branches get
no test coverage and quietly rot: every early-return or else-arm in a brain's
tick is a full behavioral state and must handle look/fire/move like the happy
path.

## Fix

No-route: steer via recovery's `find_best_direction` free-space heading (wall,
ledge AND lava aware) and run-and-gun while relocating. Stuck-replan loop: two
consecutive `BackOffThenRepath` escalations against the same destination block
that goal node for 20 s (`goal_block`), forcing a different destination —
that's what actually breaks the loop when there is no per-waypoint blacklist.

## Sources

- qbots: `brain/src/brains/zb2.rs` (tick else-branch, `goal_block`, `hard_replans`)

---

# Jump-edge landings validated the arc, never the ground (and momentum overshoots)

## Problem

`detect_jump_edges` and `bridge_components_via_jump` validated a ledge-drop by tracing
the launch arc and the fall — never what the bot lands ON or skids ACROSS.
Two compounding holes: (1) the landing down-probe is `MASK_SOLID`-only, so a lava BED
under a channel reads as a landing floor (the nearest dry node still passes the distance
gate, and `launch_yaw` aims at the deadly probe point); (2) a bot lands with horizontal
momentum under 10 Hz control and skids 16–48 u past the landing node — a dry node beside
a lava channel is still a death trap. On q2dm3, a velocity-instrumented soak proved
EVERY lava entry was a fall (vz −240..−690) clustered on these landings — while three
plausible-sounding theories (combat strafing, walkway corner overshoot, rim knockback)
each failed to move the entry count when "fixed". Instrument before theorizing: one
`EVT` with position+velocity settled in one 3-minute soak what six blind soaks couldn't.

## Fix

`landing_strip_deadly(base, travel_dir)`: sample the 0/16/32/48 u overshoot strip at the
landing — in-lava contents OR a floor whose surface is lava/slime → reject the edge.
Applied in both jump paths (cache v23). Keep the `EVT lava_escape x= y= z= vz= hs=`
instrumentation — entry velocity classifies fall-in vs sprint-in vs knockback for free.

## Sources

- qbots: `world/src/navgraph.rs` (`landing_strip_deadly`, `detect_jump_edges`, `jump_down_link`)
- qbots: `brain/src/brains/{main.rs,zb2.rs,q3/mod.rs}` (instrumented escape EVT)

# Displacement-based stuck detectors are defeated by their own strafe recovery
The Eraser-style StuckDetector samples origin displacement per second against a small
deadband (16 u) and escalates Mild→Hard only while displacement stays under it. But its
own Mild remedy — a recovery strafe — slides a wall-pressed bot ALONG the wall at
30–100 u/s, well above the deadband, so every sample looks like "movement", stuck_secs
resets, and the Hard escalation (backoff + replan) never fires. A bot with a committed
route (zb2) then grinds the same wall indefinitely: Plan 51 soaks measured a 97.4 s
full-push stall ending in death, with the bot pinned on one waypoint the whole time.
The same blindness applies to bot-vs-bot hull blocks (player hulls aren't in the
collision model, so wall probes and free-space fans see open air).
Avoid: never measure "stuck" by raw displacement alone when the bot has a nav target —
measure PROGRESS (best distance-to-waypoint must improve by an epsilon per window;
Plan 51 uses 8 u per 2.5 s). Displacement can be gamed by sliding; progress cannot.
Related: euclidean nearest-node route starts project across thin walls, making path[0]
permanently unreachable — replans loop forever since goal-blocking only blocks
destinations. Validate the start node with a hull+floor straight-line test
(`reachable_start`, zb2.rs).
## Sources
- qbots: crates/brain/src/recover.rs (StuckDetector), crates/brain/src/brains/zb2.rs
  (RouteProgress::stalled, reachable_start), context/plans/completed/51_*

# Point-trace floor probes reject V-groove floors the player hull can stand on
`floor_waypoints_multi` found floors with a POINT trace down the column, then validated
with a stationary hull check at `point_floor + 24`. On any non-flat floor — a V-groove
drainage duct, a sloped channel bed, angled trim — the point reaches the groove seam,
deeper than the 32×32 hull can sit, so the stationary check is `startsolid` and the
column emits NO node. base64's drain duct (the ONLY exit from its GL-room spawn) sampled
zero nodes this way, and the connectivity gate blamed the spawn (45/46). A real player
stands there fine: pmove rests the hull ON the slopes, higher than the seam. The failure
is invisible in boundary-pair diagnostics — there is no near-miss edge to inspect,
because the landing surface has no nodes at all. Cross-sectioning the BSP (point-contents
x-z/y-z ASCII slices) is what exposed the un-sampled duct.
Avoid: when a stationary hull check startsolids over a point-found floor, hull-TRACE down
the column and accept the rest position if it lands within ~40 u of the flat-floor origin
(`hull_rest_z`, cache v24). Also: a `spawns not all reachable (N/M)` message names no
spawn — run `qbots nav-debug <map>` first; its spawn table prints exactly which spawn and
component. And REBUILD `target/release` tools before trusting "no fresh cache" errors —
a stale binary with an older cache VERSION rejects a perfectly good cache.
## Sources
- qbots: crates/world/src/navgraph.rs (`floor_waypoints_multi`, `hull_rest_z`)
- qbots: crates/world/src/collision.rs (`v_groove_world` test world)
- qbots: context/plans/completed/52_base64_stranded_spawn_teleporters.md

# Silent handshake rejection — a rejected bot hangs in Connecting forever
The Q2 server refuses a `connect` at `SVC_DirectConnect` (BEFORE `client_connect`, so
before any netchan) by sending a **connectionless** `print\n<reason>\n`
(`sv_conless.c` — `Server is full.`, `Bad challenge.`, `Connection refused.`, protocol/
password). The client FSM's OOB `print` arm was a no-op, so the reason was discarded and
the bot sat in `ConnState::Connecting` forever: no log, no timeout, `bot_task` never
returned, so the supervisor's reconnect/"giving up" logic never ran. Symptom: you ask for
N bots, fewer appear, no error — looks like a qbots roster cap but isn't (qbots is
uncapped; the wall is the server's `maxclients`).
Fix: classify an OOB `print` received while `state == Connecting` as a rejection
(`ConnState::Rejected` + `reject_reason`); `bot_task` returns `ConnectionRefused` on it and
`TimedOut` if it never reaches `Active` within `[fleet].connect_timeout_ms`. The fleet
supervisor treats both as non-retryable join failures: strict (default) records the reason,
fires shutdown, and the run returns `Err` → non-zero exit; `--loose-botcap` downgrades to a
per-bot warning. NOTE the live diagnosis flipped: the server was `maxclients=64` (not the
assumed 18), so making the reason VISIBLE mattered more than the exact cap. Verify the
fail-hard path live without a full server by pointing `--config` at a copy with
`connect_timeout_ms: 0` against the REAL server (its map query passes, bots time out before
Active) — strict → exit 1 with `fleet join failed`, loose → exit 0 with `dropping this bot`.
## Sources
- qbots: crates/client/src/conn.rs (`on_oob` print arm, `ConnState::Rejected`)
- qbots: crates/qbots/src/main.rs (`bot_task` reject/timeout returns)
- qbots: crates/qbots/src/supervisor.rs (`bot_supervisor_loop`, `fleet_join_result`)
- qbots: context/plans/completed/53_fail_hard_botcap.md
