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
