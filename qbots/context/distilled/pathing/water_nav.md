# Water navigation — swim nodes, swim edges, swim movement (Plans 39–40)

qbots' A* `NavGraph` and the brain now traverse water. Before this, every position inside a
water volume was discarded during nav gen, so any **water-only** route had no nodes/edges. On
q2dm1 the `weapon_railgun` (`240 -384 464`) is reachable **only** by swimming, so its dry floor
was an isolated component and A* returned NO PATH from every spawn.

## Nav graph (Plan 39, `world/navgraph.rs`)

- **Water node sampling** (`water_waypoints_multi`): per `(x,y)` column, scan Z for contiguous
  `CONTENTS_WATER` spans (sample `CONTENTS_WATER` only — lava/slime are deadly). Emit a
  **surface** node at the span top + a **submerged lattice** every `SWIM_SPACING=32u` down to the
  pool floor. Validate each with a **reduced** hull point-trace (`±12`, not the full `±16/-24..+32`
  standing hull — too strict for tight tunnels). Tag nodes in `water_nodes: HashSet<usize>`.
- **Swim edges** (`EdgeKind::Swim`, `try_swim_edge`): when either endpoint is a water node,
  connect in **3-D with no STEP/STAIR gate** (a submerged bot isn't floor-constrained), via a
  reduced-hull `MASK_SOLID` trace. Cap `|dz| <= WATER_VLINK=48` so one edge can't span a whole
  pool; include **same-column vertical** neighbours (the XY grid skips `(0,0)`). Cost =
  `3d_dist × SWIM_COST_FACTOR(2.0)` for swim↔swim (water move is 0.5× ground, `pmove.c:579`), but
  raw distance for **entry/exit** (one dry endpoint — walking/falling in/out is cheap).
- **Entry/exit**: dry-floor→water and **water-surface→dry-ledge**; the exit edge is the critical
  bridge that fuses the railgun room into the main component. Bidirectional swim edges store both
  `(a,b)` and `(b,a)`.
- **Prune protection**: `prune_long_blocked_edges` must treat swim edges as trustworthy — its
  walk/stair hull trace falsely flags the legitimate 3-D vertical link.
- **Cache**: mapcache `VERSION=13` + 2 fingerprint fields (`SWIM_SPACING`, `SWIM_COST_FACTOR`);
  swim edges + water tags serialized after the jump edges.
- **Scope**: A* graph only. The Recast-style navmesh is a 2.5-D walkable surface and can't
  represent a 3-D water volume without a redesign → pure-`navmesh` modes still can't reach the
  railgun (intended, documented in the navmode ranking).

## Brain (Plan 40, `brain/water.rs` + `brains/{runtester,main}.rs`)

- **Detection** (`water_level`): waterlevel isn't on the wire, so recompute it like
  `PM_CategorizePosition` (`pmove.c:765`) — sample `CONTENTS_WATER` at feet/mid/eye → 0..3.
  `>=2` = swimming.
- **Movement**: on a swim edge / when `waterlevel>=2`, set `intent.up = clamp(dz/SWIM_VERT_SCALE,
  -1,1)` (**sustained** thrust — never `mv.jump()`, which is a one-shot launch) and pitch toward
  the 3-D target so `PM_WaterMove`'s `pml.forward` (full view vector, includes pitch) carries the
  vertical component. Skip the narrow-ledge `speed_scale` damping (water is open volume).
- **Climb-out** (Q2 water-jump, `pmove.c:414`): to exit onto a ledge, look up (`pitch<=-15`) +
  forward + hold up; a few-tick hysteresis prevents oscillation at the lip.
- **Recovery**: SUSPEND stuck recovery while swimming — `find_best_direction` steers away from
  water and the 4u/1s `StuckDetector` false-fires on 0.5× swim speed.
- **Recorder**: `S` flag when `waterlevel>=2`.

## Result (q2dm1, live, 2026-06-19)

`spawn-to-weapon railgun --navmode astar` reaches in ~11–27 s (varies by spawn), 46/93 frames
`S`-flagged, z 238→434 = dive → swim tunnel → surface → climb onto the ledge. A*-backed hybrids
also reach; pure-navmesh does not (no navmesh water). See `context/mode_perf.md` for the table.
