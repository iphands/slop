# Navmesh backend spike — outcome (2026-06-18)

Built a full Recast-style **navmesh navigation backend** alongside the waypoint-graph
("astar") backend, selectable at runtime with `--mode astar|navmesh`. Both are first-class
and coexist long-term (shared `brain::Navigator` trait). This documents what was built, the
A/B numbers, and the honest gap that remains.

## What was built (all committed; astar backend untouched, still 24/24)

Pipeline, voxelization-based (BSP faces were dropped at parse time):
- **`Navigator` trait** (`brain/src/nav_mode.rs`) — the steering-loop contract both backends
  implement. `NavigationDriver` (astar) implements it unchanged; the scenario loop drives a
  `Box<dyn Navigator + Send>`. `--mode` threads through `run_scenario` like `--spacing`.
- **Heightfield** (`world/src/navmesh/heightfield.rs`) — rasterize the collision model into a
  grid of walkable spans (floor + 56u headroom, multi-level). **No hull erosion** (it severed
  Q2 doorways); wall clearance is moved to the funnel.
- **NavMesh** (`polymesh.rs`) — one convex quad per walkable cell-span + portal adjacency
  (4-neighbour within `STEP`=18). `bridge_components` stitches stair/ramp gaps via the proven
  `walkable_stair` (rayon-parallel). q2dm1 cell=16: all 10 spawns + the RL platform in ONE
  component.
- **A\* + funnel** (`query.rs`) — polygon A* then the Simple Stupid Funnel Algorithm, portals
  inset by the agent radius so paths thread doorways centrally and clear walls. Bridged links
  become "pinch" waypoints.
- **NavmeshDriver** (`brain/src/navmesh_driver.rs`) — plans a funnel path on goal change,
  follows it with **projection-native** pure-pursuit (`brain/src/pursuit.rs`) — no
  reach/orbit/give-up density coupling.
- **Tooling**: `navinspect heightfield|navmesh|navpath` modes; process-wide navmesh cache so
  N bots share one build.

`navpath` proves the **geometry is correct**: spawn0→RL (704,104,920) reaches the launcher
via the bridged z=920 platform; spawn0→spawn3 reaches across the map. Funnel unit tests pass
(straight corridor → a line; L corridor → cuts the inside corner).

## A/B results (q2dm1)

| Scenario | astar | navmesh (initial spike) | navmesh (after maturation) |
|---|---|---|---|
| spawn-to-spawn (24 bots) | **24/24** | 7/24 | ~10–14/24 (pile-up limited) |
| spawn-to-weapon rocketlauncher (12 bots) | ~6/24 | 0/24 | **10–11/12** ✅ |

**The RL northstar is solved on the navmesh and it BEATS astar there** (~11/12 vs ~6/24): the
bot executes the full route — climb to the grenade-launcher (z=1256) → **drop onto the z=920
RL ledge** → follow the thin ledge to the launcher. This is exactly the route the whole spike
was for, and the waypoint graph never did it well.

### What got the RL working (this maturation pass — all committed)
- **Distance-field erosion** (`Heightfield::erode`, Recast-style): drop the near-wall ring
  where the ±16 hull jams, at cell=8 / radius 1 (8u) so 32u doorways + the ~33u RL ledge survive.
- **Rectangle-merged polys + cell-step adjacency** (`climbable_walk`-validated) — replaces the
  per-cell quads; wide portals so the funnel inset doesn't collapse.
- **One-way drop links** (`find_drops` on the FULL heightfield → `add_drops` onto the eroded
  mesh): directed high→low edges where the bot walks off a clean ledge. Connects drop-only spots
  (the RL ledge / spawn5). All 10 DM spawns now in ONE component.
- **Thin-ledge funnel pinning** (`is_narrow_ledge`): pin narrow drop-edge polys through their
  center so the funnel can't straighten across and cut the bot off a winding ledge. **This is
  what took RL 0→11/12.** Stairs/corridors (no drop on the side) still straighten.
- **Removed a false-positive stall-clear** in `NavmeshDriver::update` (path-progress < 4u/tick
  cleared the path on a legitimate corner-slow → bot stopped in the open). Real stalls are
  caught by the scenario's position-based StuckDetector.

### The remaining spawn-to-spawn gap = pile-ups (well-understood, see map_errors)
Single bots usually reach; the count is dragged down by **same-goal pile-ups**: identical
deterministic funnel paths funnel every bot into the same tight doorway, where the **lead's
hull jams in a dead pocket** (startsolid all directions — it overshoots the straightened
approach into a corner). astar threads these via centered waypoint approach. De-jamming is a
real dilemma: erode=2 kills the RL ledge, erode=1 leaves jams, and a hull trace can't tell a
jam-pocket from a thin-ledge cell. Pinning doorways / speed cuts both regressed RL. Next lever
is local bot-avoidance (cap pile-up damage) — a substantial new subsystem, deferred.

## Conclusion (updated): approach validated, RL goal MET, s2s pile-up-limited

The navmesh **beats astar on the RL** (the thing the spike existed to fix) and is the better
backend for drop-route weapon goals. astar remains the better backend for crowded
spawn-to-spawn (24/24 vs ~12), so the **two backends are complementary and both ship** behind
`--mode astar|navmesh` (astar default) — exactly the long-term plan. The navmesh s2s gap is
pile-ups at tight doorways (above), not the path geometry.

### Original parity-gap notes (mostly addressed this pass — kept for history)
The navmesh **path geometry is good** — the funnel gives smooth, wall-clear, density-
independent paths, and connectivity (incl. the RL platform) is solved. Remaining driver gaps:

1. **Stair/ramp + bridge following** — bridged transitions become single "pinch" waypoints;
   the driver doesn't climb the actual stair (no per-step targets, no jump). Complex vertical
   routes (z=920 platform, RL) stall here. *Highest-value fix.*
2. **No off-mesh jump/drop links** (Phase 5 not done) — ledge drops and the func_plat
   elevators aren't modelled; some routes need them.
3. **Crowding** — the funnel threads every bot down the *same* centerline → doorway pile-ups
   at 24 bots. Needs path spreading or local avoidance.
4. **Recovery/blacklist** — `NavmeshDriver` only clears+replans (same path) on stall; it has
   no poly blacklist or corner-escape equivalent, so it loops on a bad spot.

## Next steps to reach parity (own follow-up effort)
- Densify the funnel path across bridges into per-step waypoints that follow the real stair
  surface (sample the corridor polys' heights), and press jump on drops.
- Add off-mesh links (jump-down between polys; func_plat elevators) — Phase 5 leftover.
- Port the astar driver's blacklist + corner-escape recovery to `NavmeshDriver`.
- Consider greedy rectangle merging (perf) only if A* cost matters after the above.

The two backends ship side by side; **astar stays the default and is unaffected**. The
navmesh is a working, A/B-able second backend whose paths are sound but whose driver needs
the above maturation before it can be recommended for fleets.
