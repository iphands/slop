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
- qbots: `context/map_errors.notes.log` (q2dm3 session 2 analysis)

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
- qbots: `context/map_errors.notes.log` (2026-06-18 Session 4 analysis)
