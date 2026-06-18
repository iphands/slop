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

## A/B results (q2dm1, 24 bots, 60s)

| Scenario | astar | navmesh |
|---|---|---|
| spawn-to-spawn | **24/24** | 7/24 |
| spawn-to-weapon rocketlauncher | 5/24 | 0/24 |

Single-bot navmesh is **high-variance and route-dependent**: flat/simple routes succeed
(e.g. a 3885u run reached, hindered_frames=39 — far better than astar's fine-grid ~250),
but complex routes (stairs, the bridged z=920 platform, the RL) fail.

## Honest conclusion: approach validated, driver not yet at parity

The navmesh **path geometry is good** — the funnel gives smooth, wall-clear, density-
independent paths, and connectivity (incl. the RL platform) is solved. The **driver does not
yet meet the parity gate** (24/24 s2s + RL improvement). The gap is path-*following*
robustness, which the astar `NavigationDriver` accumulated over many iterations and the
prototype `NavmeshDriver` lacks:

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
