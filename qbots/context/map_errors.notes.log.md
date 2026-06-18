# q2dm1 nav connectivity investigation
# Started: 2026-06-16

## Observed failure (generate-map-cache q2dm1)
- nodes=7377 edges=110337 total_components=5
- Component sizes: 7126, 143, 69, 27, 12
- Only 3/10 spawns in largest (component 0)
- 7 spawns in minority components:
  - C1 (143 nodes): spawn[4](1488,-48,664), spawn[5](1888,464,920),
                     spawn[6](2016,-224,664), spawn[7](1512,-24,536)
  - C2 (69 nodes):  spawn[0](544,352,482)
  - C4 (12 nodes):  spawn[1](1552,600,352), spawn[2](1888,736,536)
- C1 is LARGE (143 nodes, high-x area x=1488-2016) — whole right side of map disconnected
- C4 is tiny (12 nodes) despite 2 spawns having navigable BSP positions

## History
- Previous fix (walkable_stair): main comp grew from 3495 → 7126 nodes.
  Stair-riser clipping was the culprit then. Now something else blocks the last gaps.

## Hypotheses (ranked by likelihood)
A. STAIR_MAX=42 too small for steep geometry in q2dm1.
   Diagonal grid neighbors 33.9u apart can have dz = 42 at 51° slope.
   Any slope >51° diagonal => skipped. STAIR_MAX=42 is tight.
   Fix: increase to 128u (safe — stair trace still rejects actual walls).

B. Stair trace failing for covered/indoor staircase (ceiling blocks UP trace).
   If the staircase going to the upper right area is under a ceiling,
   our vertical UP sub-trace hits the ceiling and returns false.
   Fix: needs a different trace strategy for covered stairs.

C. Dynamic entity (func_plat elevator) is the only connection to C1.
   The upper area (x>1488) might only be reachable via an elevator.
   Our static collision model doesn't include inline BSP model brushes in its tree.
   Fix: detect func_plat entities and add elevator edges.

D. Grid sampling gap — no floor nodes in the narrow passage connecting regions.
   Fix: smaller grid spacing (expensive) or targeted bridge generation.

## Next actions
1. Add NavDebug command: show boundary node pairs and why trace fails.
2. Try STAIR_MAX=128 as first cheap fix.
3. Run nav-debug on q2dm1 to confirm which hypothesis is correct.
4. Fix root cause and verify all q2dm* maps pass.

---

## 2026-06-16 Session 2 — Continued investigation

### bridge_pass ONLY-NEAREST bug (FIXED)
- bridge_pass tracked a single `best` (nearest cross-comp node) per source node.
  If that one walkable_link trace failed (wall between them), added ZERO edges
  and `if added==0 { break; }` exited early — so entire bridge_components did NOTHING.
- Fix: enumerate ALL cross-component pairs within range (j>i to avoid duplicates).
  Try walkable_link on each. This collapses 38→25 components on q2dm2 and allowed
  BRIDGE_HDIST=512 to fully fix q2dm2/5/6.

### BRIDGE_HDIST=96 was too small for q2dm2/5/6 (FIXED: bumped to 512)
- q2dm2 has wide elevator shafts with no sampled floor nodes across 250+ unit gaps.
  Required BRIDGE_HDIST≥512 to cross. Performance hit: ~30s per map for generation
  (cached; acceptable for one-time cost).

### q2dm7 spawn[0] is a WATER PIT (KNOWN LIMITATION)
- spawn[0] at z=-296, all 206 nodes in comp2 at exactly z=-296 (flat floor).
  Nearest other-comp nodes (comp3, z=16) have dz=312 > STAIR_MAX=128.
  The func_door lift only reaches z=-153 (player origin), not z=-296.
  q2dm7 "The Edge" has a water pit in the lower area. Players swim up.
  Our nav graph has no water-swimming support — this spawn is permanently
  disconnected from the walking nav. NOT a bug in BSP/collision/nav generation;
  it's a genuine water-access limitation.

### q2dm3 inline-model transparency causes lift shaft blindness (OPEN)
- Floor probe can't see inline model surfaces (func_plat, func_door platforms).
  The probe falls through the platform and records the world floor below.
  Result: NO floor nodes exist at the upper floor level accessed by the platform.
  connect_node_to_nearby for the top node finds nothing nearby in the upper area.
  So the func_plat/func_door lift nodes go into comp1 (lower floor), failing to
  bridge comp1↔comp2.
- nav-debug shows: C1:(-608,512,83) ↔ C2:(-601,511,152) → hull=BLOCKED, stair=FAIL.
  The shaft walls (world geometry) block the trace between lift top and upper corridor.
- Potential fix: probe world-floor at XY positions just OUTSIDE the shaft at each
  lift level to get anchor nodes in the correct component, then connect_node_to_nearby
  links them to the lift nodes.

### Current q2dm* status (BRIDGE_HDIST=512, all-candidates bridge_pass)
- q2dm1: PASS
- q2dm2: PASS
- q2dm3: FAIL 3/7 — lift transparency + shaft wall trace failure
- q2dm4: PASS
- q2dm5: PASS
- q2dm6: PASS
- q2dm7: 5/6 — spawn[0] water pit (known limitation)
- q2dm8: PASS

## 2026-06-16 Session 2 continued — STAIR_MAX fix

### STAIR_MAX=128 too small for q2dm3 staircase (FIXED: bumped to 160)
- nav-debug showed pairs like (815,295,24)↔(815,295,168) with dz=144 at the SAME XY.
  The multi-floor probe found two floor levels at that column (bottom and top of a
  staircase flight). With STAIR_MAX=128, dz=144 > 128 so bridge_pass SKIPPED those pairs.
- With STAIR_MAX=160: the stair trace IS attempted; walkable_stair succeeds for the
  actual staircase geometry, and q2dm3 goes from FAIL(3/7) → PASS(7/7).

### q2dm7 spawn[0] in unreachable pit — NOT a water area (CONFIRMED LIMITATION)
- The 206 nodes at z=-296 are NOT in water content (probe doesn't skip them), but
  the area IS a deep pit: nearest other-comp nodes are at z=16 (dz=312 > STAIR_MAX=160).
- MAX_FALL for jump edges is 256; the 312-unit drop from z=16 to floor_z=-320 exceeds
  this limit, so no jump edge is added.
- No func_plat/func_door/func_train can reach z=-296 (deepest lift goes to z=-153).
- The pit is ONLY accessible by falling from the ledge above (one-way) — there is no
  walkable return path. Bots spawning at spawn[0] are stranded in the pit.
- This is NOT a bug. The spawn IS a deathmatch spawn (players fall into the pit, then
  eventually die and respawn elsewhere). Walk-based nav cannot connect it.
- Status: ACCEPTED LIMITATION. q2dm7 reports 5/6 spawns in largest component.

### Final q2dm* status (STAIR_MAX=160, BRIDGE_HDIST=512, all-candidates bridge_pass)
- q2dm1: PASS (10/10 spawns)
- q2dm2: PASS (7/7 spawns)
- q2dm3: PASS (7/7 spawns)
- q2dm4: PASS
- q2dm5: PASS (9/9 spawns)
- q2dm6: PASS (8/8 spawns)
- q2dm7: 5/6 spawns — spawn[0] in unreachable pit (accepted limitation)
- q2dm8: PASS

---

## 2026-06-17 Session 3 — Plan 19 T6 live verification (spawn-to-spawn + spawn-to-weapon)

### Target: 8/8 on both scenarios with --count 8 --max-secs 60

### Current best results (after multiple fixes):
- spawn-to-spawn: 5-7/8 (variable; saw 8/8 with an aggressive blacklist combo that broke weapon)
- spawn-to-weapon: 0-3/8 (0-1 when no lucky z=920 spawns; 2-3 when 2+ bots spawn at z=920)

### Root cause of remaining failures:
Bots from low-level spawns (z=352-664) need to navigate a 2000-4000u path including
floor transitions (stairs/ramps) to reach the weapon at z=920. With 60s budget and
~65-90 u/s average speed, they CAN reach in theory (2000u / 80u/s = 25s). But
path_efficiency=0.15-0.45 means bots travel 3000-6000u total while making only 500-2000u
net progress. They loop/backtrack instead of making direct progress.

### Fixes applied this session (cumulative):
1. **smooth_path MAX_SMOOTH_HDIST=120u** — prevents 600u+ platform shortcuts (point trace)
   that caused bots to race off z=920 ledge edges. Tests updated to 50u node spacing.
2. **WP_REACH_HORIZ 16→24u** — prevents wrong_turns from overshooting 16u reach radius
   at 300u/s (30u/frame). 32u tried but caused waypoint skips near walls.
3. **ORBIT_RADIUS 80→48u** — prevents premature orbit timeout for bots genuinely
   navigating corners (79u from waypoint). 5s StuckLevel::Hard handles corner cases.
4. **Above-waypoint orbit replan** — bot at z>wp_z+96u (slope-roof) replans instead of
   force-advancing in wrong direction.
5. **orbit flat-wall check** — when orbit fires (horiz<48u) AND hull trace blocked,
   blacklist waypoint + replan (prevents orbit advancing past a wall-blocked node).
6. **blacklist_waypoint_if_blocked on BackOffThenRepath** — trace-gated blacklist
   only fires when hull trace confirms waypoint is physically blocked.

### Failures attempted that made things WORSE:
- hull trace in smooth_path: ceiling at z=952 triggers startsolid → no shortcuts at all → 0/8
- HARD_REPATH_SECS=3s: too many replans (20 in 60s), blacklists valid weapon-route nodes → 0/8
- unconditional blacklist on BackOffThenRepath: works for spawn-to-spawn but 0/8 weapon

### Remaining investigation needed:
The core problem: path_efficiency is too low (15-45%) for multi-level paths.
Bot travels 4000-6000u in 60s but only progresses 600-2000u toward goal.
High wrong_turns (20-100) and bumps (40-82) indicate wall navigation failures.
The existing orbit/BackOff recovery fires but replans routes to the same walls.
The nav graph appears to have false edges near the z=920 transition zones (staircase tops).

### Node 8694 specific issue (1351,1215,920 on q2dm1):
Bots from spawn[6] (2016,-224,664) climb to z=920 near y=1136, then try to navigate
north to node 8694 at (1351,1215,920). A wall or step exists between y≈1136 and y=1215.
The hull trace in orbit-flat-wall-check detects this when horiz<48u. But bots are
typically 60-85u from node 8694 when they hit the wall (orbit doesn't fire until <48u).
The BackOffThenRepath fires after 5s, replans, but new path may route back through same wall.
Confirmed by: bot ending at (1104, 263, 694) after 60s — navigated past the problem area
but fell to a lower floor in the process.

---

## 2026-06-18 Session 4 — Plan 19 T6 continued with 32-bot testing

### Expanded to 32-bot tests per user request for better statistics.

### Additional fixes applied (cumulative from Session 4):
7. **ORBIT_FRAMES 15→25 (pub const)** — bots get 2.5s to navigate corners before orbit fires.
8. **GOAL_GIVEUP_TICKS 30→15** — 1.5s before giveup fires; debug shows 7 giveups in 60s = 21s wasted.
9. **GIVEUP_BLACKLIST_MAX 8→32** — larger blacklist so A* avoids revisited false-edge nodes.
10. **HARD_REPATH_SECS 3.5s** — faster BackOffThenRepath than 5s.
11. **Orbit/giveup interaction fix**: reset goal_age_ticks when horiz<ORBIT_RADIUS to prevent
    giveup from firing before orbit (giveup at tick 16 was racing orbit at tick 24 after GOAL_GIVEUP_TICKS=15).

### Debug trace analysis (RUST_LOG=brain::nav=debug, spawn-to-weapon, spawn[1]):
From (1543,591,352), A* routes through z=352→472→552→664→792 in a cascade of false-edge detours.
Key observation: giveup fired 7 times in 60s (at t=5s, 15s, 30s, 38s, 44s, 53s, 58s), consuming 21s.
After each giveup, A* found paths going EAST (wrong direction) when direct western paths were blacklisted.
The bot spent 20-38.8s going east to z=472-552 (wrong direction) after exhausting western path nodes.
At t=38.8s bot finally found path to z=664-792 but timed out before reaching z=920.

### Key findings for spawn-to-weapon failures:
- Only 2/10 spawn points are at z=920 (spawn[5] and spawn[8])
- spawn[5] (1879,471,920) ALWAYS reaches the weapon in ~19s (easy path, short distance)
- spawn[8] (1327,1215,920) near wall issue — sometimes reaches, sometimes loops
- spawn[0-4,6-7,9] at z=352-792 need to climb 128-560u of stairs to reach z=920
- With 32 bots: expected ~6 at z=920 (3.2× spawn cycles × 2 easy spawns × partial coverage)
- Expected weapon reaches from 32 bots: ~5-6 (z=920 spawns) + 0-2 (lucky low-level spawn navigation)
- Actual 32-bot weapon results: 5/32 (15.6%), consistent with this model

### Approaches tried in Session 4 that FAILED:
- LOOKAHEAD=192u: more wrong_turns (55-118) and bots stuck for 35s (hindered=355) → worse
- Proactive wall deflect (hull trace 48u ahead, deflect ±30-60°): more wrong_turns (79-118) → worse
- startsolid blacklisting in orbit check: blacklists legitimate slope nodes → 10/32 spawn-to-spawn
- ORBIT_FRAMES=25 + HARD_REPATH=3.5 alone: no improvement over baseline

### Current 32-bot baseline (2026-06-18):
- spawn-to-spawn: 17/32 (53%) — consistent across 3 runs
- spawn-to-weapon: 5/32 (15.6%) — mostly z=920 spawns; 1 low-level spawn barely reached at 56.78s

### Hypothesis for remaining failures:
The staircase path from low-level spawns to z=920 passes through intermediate floor areas
(z=472-664). Each intermediate area has nav graph edges to the staircase transitions, but
some transitions have false edges (walkable_stair accepts them but in-game physics can't navigate).
When false edges are blacklisted, A* routes through detours that are also invalid.
Only ~15% of bots (z=920 spawns + lucky low-level paths) successfully navigate in 60s.

---

## 2026-06-17 Session 4 (cont.) — ROOT CAUSE FOUND: usercmd msec was 1/3 speed

### THE BIG ONE: msec hardcoded to 33 → bots moved at 1/3 human speed.
MovementController::build_cmd set `msec: 33` (a stale "30 Hz client" assumption).
The bot loop runs at the server's 10 Hz frame cadence (1 usercmd per svc_frame, 100 ms
apart). Q2 server runs PM_Move once per usercmd with `pml.frametime = cmd.msec * 0.001`,
so a 33 ms timestep on a 100 ms tick = ONE THIRD of pm_maxspeed. forwardmove was full 320
the whole time, but the server only integrated 33 ms of it per tick.

### Fix: MovementController::set_msec(dt) called each tick from measured serverframe delta.
- spawn-to-spawn: 17-20/32 → 24-25/32 (53-63% → 75-78%)
- spawn-to-weapon: 5/32 → 3-6/32 (no consistent change yet — weapon failures are dominated
  by low-level-spawn stair-climb false edges, not speed)
- per-frame mean_speed: 95 → 150-250 u/s (matches a real player now)

### IMPORTANT meta-lesson for future tuning:
The slow speed MASKED nav bugs and INFLATED the apparent need for several constants.
Many timeout/giveup/orbit values were tuned against 1/3-speed behavior. Now that speed
is correct, RE-EVALUATE before trusting old tuning:
- BackOff is 8 ticks of move_forward(-1.0): was ~85u backward at 1/3 speed, now ~256u at
  full speed — may overshoot. Watch for bots backing into new problem areas.
- ORBIT_FRAMES=25 / GOAL_GIVEUP_TICKS=15: tick-based, so unchanged in TIME, but at full
  speed the bot reaches or hits-wall faster, so giveup/orbit fire less often (good).
- LOOKAHEAD=96u: covered in 3 ticks now vs 9 — still fine.

### Remaining weapon failures (3-6/32) profile (post-msec-fix):
Failing bots: hindered_frames 264-385 of ~600 (stuck against walls HALF the run),
path_efficiency 0.17-0.23, bumps 58-105. These are low-level spawns (z=352-664) whose
stair-climb route to z=920 hits false nav edges. They press into walls at full speed now
(bump count went UP because they MOVE faster into the same bad geometry). The real fix
remains nav graph quality at the staircase transition zones — speed alone can't fix a
path that routes through a wall.

### Speed-consequence audit (constants tuned against the old 1/3 speed):
At the corrected msec the bot moves 32 u/tick (was ~11). Re-evaluated every distance-based
constant:
- **WP_REACH_HORIZ 24→32**: WIN. 24 u let the bot pass a waypoint tangentially without the
  path pointer advancing → pursue_target pulled backward → wrong_turns. 32 u = one tick of
  travel. spawn-to-spawn 24-25 → 26-28/32. (Old note "32 u skips near walls" was a 1/3-speed
  artifact, no longer true.)
- **BackOff 8→4 ticks**: LOSS, reverted. Hypothesis was 8 ticks = 256 u backward at full
  speed lunges bots off z=920 ledges. But 4 ticks (128 u) is too short to clear a wall hull:
  18/32 vs 28/32. Keep 8. The ledge-lunge risk is real but clearing the wall matters more.
- **ORBIT_RADIUS 48, ARRIVE_RADIUS 80, DEADBAND 16, ORBIT_FRAMES/GOAL_GIVEUP tick-based**:
  left as-is. Tick-based timers are unchanged in TIME; arrive/deadband still have adequate
  margin at full speed. No evidence they need retuning.

### Climb-then-fall pattern (the weapon-test killer, traced 2026-06-17):
Bot from spawn[4] (1495,-57,664) REACHED z=921 at t=28s (start_pos=[1378,1104,921]) — it was
on the weapon level! But the weapon is at XY (704,104); the bot at (1378,1104) is ~1200 u away
across the top floor. The top-level nav graph near (1378,1104) does not cleanly connect to the
weapon area, so after a give-up the replan routed the bot back DOWN (z=921→536→472) and it
thrashed the rest of the run. Confirmed: this is a top-level (z=920) nav-graph connectivity
gap, not a speed or climb problem. The climb now works; crossing the top floor to the weapon
does not. NEXT: inspect z=920 nodes/edges between (1378,1104) and (704,104) for the gap.

---

## 2026-06-17 Session 5 — question-everything pass (jump/hover + corner-cutting)

User feedback (do NOT assume these away):
- Bots "fly"/hover in mid-air — jump and float in a way no real player would.
- Running into walls IS a bug — user navigates q2dm1 without touching walls.
- Bots "cut" corners which can make them fall.
- Stagger is ONLY because the server frags bots standing on a spawn when a new bot
  spawns there. Bots can collide and "stack". Some optimal paths need an elevator
  (we may not ride elevators yet; another bot may have taken it up).

### Finding 1 — POGO/HOVER: stuck bots jump in place every ~1 s.
Traced spawn-to-weapon log: bot jumps at t=5.19, 5.99, 8.09, 15.29, 17.79, 23.79…
Many jumps have spd=0-2 (stationary). At t=17.8→23.8 the bot sat at z=491 jumping
straight up and down for 6 s. Physics is CORRECT (vz starts ~270, drops 80/tick =
800 u/s² = Q2 gravity; upmove does NOT add lift in PM_AirMove — wishvel[2]=0). So the
"hover" is a BEHAVIOR bug: the StuckLevel::Mild recovery (stuck ≥1 s, no wall ahead)
fires RecoveryAction::Jump every second, and current_edge_is_jump() adds more. A stuck
bot pogos uselessly instead of solving why it is stuck. Jumping off a platform can also
drop it to a lower floor (the climb-then-fall pattern).

### Finding 2 — CORNER-CUTTING: smooth_path uses POINT traces, not hull traces.
crates/world/src/navgraph.rs smooth_path() validates shortcuts with a ZERO-width point
trace (mins=maxs=[0,0,0]). The bot hull is 32×32×56 (±16 xy, -24/+32 z). A point ray
clears a corner gap the hull cannot fit through, so smoothing collapses A→B→C into A→C
across an inside corner; pursue_target then steers straight at the cut, and the hull
clips the wall = wall-bump. Nav graph EDGES are hull-valid (built with HULL_MINS/MAXS),
but smoothing reintroduces hull-INVALID shortcuts.
Prior note claims a full-hull trace here gave 0/8 ("ceiling z=952 startsolid"). That was
at 1/3 speed — RE-TEST now; do not assume it still holds. Plan: hull-trace smoothing,
possibly with a slightly inset hull to tolerate exact-fit ceilings/floors.

### Finding 3 — FALSE BRIDGE HUB: walkable_stair false-positives on long spans.
Wrote crates/tools/src/bin/navinspect — dumps nav nodes+edges near a point with a
live hull-trace + walkable_stair re-check. Found the spawn[3] (-80,800,472) failure
cause: node 10300 (1567,543,568) is a FALSE HUB with 19 edges, ~16 of them BLOCKED by
a live hull trace. The big ones (dz=±80-96, hd=187-255) report stair=OK — i.e.
walkable_stair APPROVES them — but they are not real staircases (slope 0.32-0.38;
real q2dm1 stairs are slope ~1.0, hd ≲ 136 for one flight). They were added by
bridge_components with BRIDGE_HDIST=256 (bumped from 128 to span one real gap). A few
edges (9583, 9787, 10696) are false by BOTH tests (stair=NO, hull=BLOCKED).
A* sees 10300's cheap long edges, routes bots through it, they can't traverse → thrash.
This is THE cause of the clustered goal=(-80,800,472) failures (6 of 7 in one run).
Root: walkable_stair's step probe passes when sampled points happen to land on
surfaces across open space; BRIDGE_HDIST=256 lets it span far enough to do real damage.
FIX PLAN: post-build prune of edges that are hull-BLOCKED AND span hd > a real stair
flight (≈144). Verify spawn connectivity still holds after pruning (dense 24u grid +
legit short stairs should keep everything connected).

### Bridges — dedicated writeup + grid-size question → context/bridges.md
Documented WHAT bridges are (cross-component stitching only; zero added if generate()
is already single-component), WHY fragmentation happens (stairs/winding flights,
under-sampled doorways, lifts; one-way pits are irreducible), and the COST (BRIDGE_HDIST
creep 128→256→512 + walkable_stair false-positives = false hubs like node 10300).
Hypothesis: a finer grid (24→16→12→8) may collapse pre-bridge fragmentation and let us
shrink/remove bridging at the root. Added tools/gridscan to measure components vs spacing
pre-bridge (decisive experiment). gridscan NOT yet run — world crate was mid-refactor
(did not compile: UnionFind). Run it and fill the table in bridges.md.

---

## 2026-06-17 Session 5 summary — big wins (msec, generate, elevator)

Baseline at session start: spawn-to-spawn ~17-20/32 (53-63%), weapon ~5/32.

Fixes landed this session (each its own commit):
1. **msec=33 → dt-derived** (THE big one): bots ran at 1/3 speed because usercmd msec was
   hardcoded. Now full pm_maxspeed. spawn-to-spawn 17→24/32.
2. **WP_REACH_HORIZ 24→32**: full-speed bots overshot the 24u reach radius (a 1/3-speed
   tuning artifact). spawn-to-spawn 24→26-28/32.
3. **Removed pogo-jump recovery**: Mild stuck now strafes, not jump-in-place (the "hover").
4. **Corner-cut-safe pursue_target**: hull + floor validated steering point so bots don't
   cut corners into walls or across gaps.
5. **Connectivity-preserving prune of false bridge edges**: union-find keeps load-bearing
   bridges, drops redundant false hubs (node 10300: 19→8 edges).
6. **generate() wider connection (CONNECT_CELLS=3)**: ROOT FIX for the fragmentation the
   user suspected was a bug. compgaps proved generate missed 934 walkable links (only
   linked ±1 cell). Now ±3 cells: components 66→31, spawns-in-largest 2/10→8/10, prune
   14788→2316. Edge count tripled (524k) but A* fine.
7. **Elevator penalty + --lift-penalty switch**: func_plat deadlock (bots hold the lift up).
   Interim hack penalises lift ride edges so A* prefers stairs. TODO-marked everywhere +
   context/elevator_todo.md forcing doc. spawn[3] failures 6→1.

New tools: navinspect, gridscan, compgaps (crates/tools/src/bin/).

Current standing (24 bots, default lift-penalty=5000):
- spawn-to-spawn: 19-22/24 (79-92%, variance from bot-on-bot collisions)
- spawn-to-weapon (rocketlauncher): ~5/24 — STILL the laggard

### Remaining frontier: spawn-to-weapon (z=920 climb)
Low-level spawns (z=352-664) must climb stairs to the weapon at z=920. Bots reach the
right level then get stuck at z-transition ledges (e.g. stuck at z=792 bumping walls when
goal is z=472). This is a MOVEMENT-through-transitions problem, not graph connectivity.
Also: high run-to-run variance suggests bot-stacking collisions matter — fewer bots = higher
rate. NEXT: investigate the z-transition ledge navigation (descend/ascend smoothing) and
the weapon-specific path quality.

---

## 2026-06-17 Session 5 (cont.) — RL weapon route + grid-size experiments

### The real RL route (from the user, who plays q2dm1):
NOT stairs. chaingun room (1400,1280,784) has stairs UP to the grenade launcher
(1576,936,1040, the highest point). From the GL platform you DROP DOWN (~143u, one-way
fall) onto a NARROW ledge (~z=897, near elevator-2 top at 1378,1104). Follow the ledge
through an opening → spawn[8] (1327,1215,920) + armor shards → navigate a corner/bend →
the rocket launcher (704,104,920). The elevator is OPTIONAL (a second way onto the ledge).

### Why our graph can't do it (diagnosed with navinspect path mode + floor probe):
- The RL sits on a z=920 island. Our graph connects it to the lower map ONLY via a FALSE
  cliff edge (3972→3978, dz=128 hd=96, stair=NO, hull=BLOCKED) — unfollowable — plus the
  penalized elevator and a 36-93 hop NE detour through false edges. So low spawns can't
  reach it; only z=920 spawns + lucky bots do (~2-6/24).
- The narrow ledge is UNDER-SAMPLED: ~1 node at z~897 near elevator-2; ZERO nodes along
  the SW ledge toward the RL. The floor probe confirms the ledge FLOOR EXISTS and a hull
  fits (WALKABLE) — so it is a SAMPLING gap, not a BSP/collision gap.
- The DROP edge (GL platform → ledge) doesn't exist: detect_jump_edges probes only
  spacing*1.5=36u out and needs a SAMPLED landing node within STEP*2=36u. The narrow ledge
  has no nodes to land on.

### Grid-size experiments (answering "should we use a finer grid?"):
| grid | CONNECT_CELLS | radius | regen | spawn-to-spawn | weapon |
|------|--------------:|-------:|------:|---------------:|-------:|
| 24   | 3 | 72u | 38s   | **24/24** | 6/24 |
| 20   | 4 | 80u | 1m24s | 7/24      | 2/24 |
| 16   | 4 | 64u | 3m20s | 16/24     | 5/24 |

VERDICT: grid=24/CC=3 is the validated sweet spot. Finer grids DESTABILIZE spawn-to-spawn
and do NOT help the weapon. Likely cause: the movement constants (WP_REACH_HORIZ=32,
LOOKAHEAD=96, ORBIT_RADIUS=48) are tuned for ~24u waypoint spacing; finer grids change
waypoint density and the steering/give-up behaviour degrades. Re-tuning all of those for a
new grid is a big rabbit hole and finer sampling alone does NOT model the drop-onto-ledge.

### Conclusion: the RL needs a TARGETED feature, not grid tuning:
1. Sample the narrow ledge (adaptive finer sampling near ledges, OR seed the known route).
2. Create the one-way DROP edge from the GL platform onto the ledge (wider jump-edge probe
   + create a landing node if none is sampled).
3. Connect the ledge → RL (the bending walkable area).
This is a multi-step nav-graph feature, separate from the now-solved spawn-to-spawn work.

### RESOLVED: why grid changes broke us — the connection RADIUS, not the grid.
The load-bearing quantity is CONNECT_CELLS × GRID_SPACING (absolute connect radius ~72u),
NOT the grid spacing itself. My naive 24→16 kept CONNECT_CELLS=3 → radius fell 72→48u →
under-connection → 16/24. Holding the radius at 72u (e.g. grid=18/CC=4) restores ~20/24.
Both bigger (80u) and smaller (64u) radii hurt. FIX (committed): CONNECT_RADIUS=72.0 const +
connect_cells(spacing)=round(72/spacing); generate() derives cells from it, so GRID_SPACING
is now safe to change and the radius auto-stays correct. Fingerprint keys on CONNECT_RADIUS.
Residual: even at the correct radius, finer grid is slightly worse (grid=18 ~20/24 vs grid=24
24/24, ~2× bumps) AND does NOT sample the narrow RL ledge — so finer grid is NOT the RL fix.
grid=24 remains the best spawn-to-spawn config; the RL needs the targeted drop-onto-ledge work.

---

## 2026-06-17 Session 6 — making the grid changeable (TWO grid-coupled quantities)

Goal: 100% spawn-to-spawn at grid 16/18/20 (so we can later use a finer grid for the RL).
The user's hypothesis (CORRECT): multiple things must scale with grid, some by fractional
ratios when the grid doesn't divide evenly.

### Coupling #1 — connection RADIUS (world units), not cell count.
connect_cells=round(72/spacing) rounded to an integer → wrong radius for grids that don't
divide 72: grid=16→round(4.5)=5→80u, grid=20→round(3.6)=4→80u; only 18/24 hit 72u.
FIX (committed): connect_cells=ceil(72/spacing) to over-cover, then generate() filters
candidates to EXACTLY ±CONNECT_RADIUS (72u) per axis. Radius now identical for every grid.
Behaviour-preserving at grid=24 (±72u == old ±3 cells, pruned 7544, 24/24).

### Coupling #2 — steering constants are RATIOS to node spacing.
WP_REACH_HORIZ/WP_REACH_DZ/ORBIT_RADIUS/LOOKAHEAD were tuned for 24u spacing. Held fixed at
finer grid, the bot advances/looks-ahead too far RELATIVE to waypoint spacing → corner-cuts
+ bumps. grid=18 fixed consts = 14/24; scaled ×0.75 = 22/24. FIX (committed): derive them
from world::GRID_SPACING (×4 / ×4/3 / ×1 / ×2) so the ratios are fixed. Same 96/32/24/48 at
grid=24.

### Important caveat — VARIANCE.
spawn-to-spawn is NOT deterministic: even grid=24 swings 22-24/24 run to run, from bot-on-bot
collisions (bots stack). So "100%" really means "~24/24 typical". Single-run comparisons
across grids are noisy; need 2-3 runs each. (Full sweep results appended below once measured.)

### Still TODO for the RL (separate from grid):
Finer grid (even at correct radius+constants) did NOT sample the narrow RL ledge at the
points probed — it bends. The RL still needs the drop-onto-ledge feature (sampling + one-way
drop edge + ledge→RL connectivity). Grid flexibility is the enabler, not the fix itself.

---

## 2026-06-18 Session 6 (cont.) — parallelizing the regen pipeline (rayon, ncpus)

Motivation: fine grids make regen brutally slow (grid=12 = 10m32s), blocking experiments.
A subagent suggested parallelizing the PRUNE; we did, but per-phase timing (new `timed!`
macro in build.rs) revealed the prune was NEVER the bottleneck (0.09s). The real hogs:

grid=24 phase profile (before → after parallelization):
- generate:           0.29s (already parallel)
- bridge_components:  9.0s → 0.49s   (18.5×)
- prune:              0.09s (parallelized anyway; was a red herring)
- detect_jump_edges:  28.7s → 1.5s   (19×)  ← the real bottleneck

detect_jump_edges is O(n²): each node does 8 directional probes, each calling nearest()
(an O(n) linear scan). bridge_pass is the cross-component pair search (walkable_stair
traces). Both are read-only per-node work → same rayon pattern as the prune:
  Phase 1 (parallel): per-node classify/collect, all cm.trace/nearest calls fan out across
    cores (rayon global pool = ncpus). flat_map_iter preserves order.
  Phase 2 (sequential): apply results in order — distinct node index per list, so keys
    never collide → byte-identical graph (verified: pruned=7544, added_jumps=35597,
    in_largest=10/10 all unchanged; new test prune_classify_par_matches_seq).

RESULT: grid=12 full regen 10m32s → **41s** (15×); user=18m26s shows 32-core spread.
grid=24 pipeline phases ~38s → ~2.4s.

Remaining: detect_jump_edges is still 23s at grid=12 (O(n²) nearest()); a spatial index
(like bridge_pass's bucket hash) would make it near-linear. Future optimisation.

Caveat unchanged: finer grids still NAVIGATE worse (density), so this speeds up EXPERIMENTS
but isn't itself the fine-grid-quality fix.

---

## 2026-06-18 Session 6 — RL narrow-ledge: the density tension (KEY finding)

Goal: a GENERIC fix for narrow walkways (the RL ledge is one instance, others exist).

### Implemented: generic narrow-surface sub-sampling in generate() (Phase 2.5).
generate() only probes the exact grid point, so a walkable surface narrower than the grid,
or whose walkable centre doesn't align to a grid point, is missed (point-trace finds the
floor below; or the hull stand-check at the centre pokes into the adjacent wall → rejected).
Fix: sub-sample each cell on a spacing/3 offset grid; add any walkable surface (the hull
stand-check in floor_waypoints_multi already rejects too-narrow wall-tops) not already
covered by a node. Bounded by a `surface_covered` check + spacing-resolution dedup so only
GAPS fill, not densify. Clean, deterministic, parallel.

### Result: it did NOT work, and revealed a fundamental tension.
- Added only +210 nodes (12890→13100) — bounded, as designed.
- BUT the RL ledge's winding MIDDLE was still not sampled/connected (spawn5→RL still 23 hops;
  weapon still 4/24). The ledge ENDPOINTS (elevator-2 top, spawn8, RL platform) ARE sampled;
  the narrow middle isn't detected as a connected walkable path even at 8u sub-resolution.
- AND it HURT spawn-to-spawn: 19/21/17/24 (vs 24/24 baseline). Reverted.

### THE FUNDAMENTAL TENSION (reframes the whole RL problem):
- The RL needs MORE nodes (to sample the narrow ledge → a short direct path).
- But MORE nodes DEGRADE navigation — proven 3×: finer grid (grid=12 exact-radius=11/24),
  and now sub-sampling (+210 nodes → 17-21/24). The bot's waypoint-follower handles denser
  graphs worse (more waypoints → jaggier paths → more bumps/wander).
So the obvious "sample the ledge" approaches can't win: the nodes they add hurt navigation
more than the ledge helps. The real bottleneck is NAVIGATION'S sensitivity to graph density,
not the sampler.

### Realistic paths forward (need a decision):
A. Make path-following robust to density (waypoint advance by projection not reach-radius;
   density-independent smoothing). Fundamental; would unlock sampling solutions. Big.
B. A non-graph DIRECT nav mode: when the bot has LOS to the goal/next-far-node, steer
   straight (collision-aware) instead of node-by-node. Lets bots beeline across open areas
   like the z=920 platform once they reach it. Medium; orthogonal to the graph.
C. Accept the long path: the RL IS reachable (23 hops); the 60s weapon cap is a TEST
   artifact (real DM bots roam continuously and would arrive). Cheapest; least satisfying.

---

## 2026-06-18 Session 6 — (A) density-robust navigation: pure-pursuit

Chose path (A): make path-following density-robust so fine grids (and thus narrow-ledge
sampling for the RL) become viable.

### DONE: pure-pursuit steering (committed).
Old steering walked LOOKAHEAD from the current WAYPOINT NODE. New: project the bot onto the
path polyline (project_onto_path) and aim LOOKAHEAD ahead from the projection (point_ahead).
Geometric → density-independent. LOOKAHEAD made a FIXED 96u (was grid-scaled, which made fine
grids jaggier). Win: grid=24 now STABLE 24/24 (was 22-24 variance); path_efficiency on hard
routes 0.14→0.3-0.5.

### NOT enough for fine grids — a SECOND density effect remains.
grid=12 with pure-pursuit: still 12-15/24. Failure profile changed: path_efficiency is now
OK (0.3-0.55, steering IS smooth) but hindered_frames is huge (~250/600 = 25s grounded +
intending-to-move + speed<100). So bots are PHYSICALLY STUCK, not wandering. Tried a
projection-based waypoint-advance (give-up reset on projection progress, not reach) — helped
marginally (13-15) and broke the orbit test; reverted. So the remaining fine-grid failure is
physical sticking against geometry the denser graph routes bots into, NOT steering or give-up.

### Honest status on the RL via fine grids:
Pure-pursuit fixed the STEERING half of the density problem. The PHYSICAL-STICKING half
remains and blocks fine grids → blocks narrow-ledge sampling → blocks the RL. Cracking it
needs understanding why denser A* paths put bots where they jam (likely nodes hugging walls/
corners; the pursue_target_safe fallback aiming at a wall-adjacent node). Open.
Pure-pursuit is kept regardless (grid=24 win). The RL is still ~4/24.

---

## 2026-06-18 Session 6 — KEY: fine grid opens the RL route (but nav must be reliable)

Decisive finding via `QBOTS_SPACING=12 navinspect ... path`:
- grid=24: spawn5→RL = 23 hops, detours DOWN off z=920 (the RL platform is poorly connected).
- grid=12: spawn5→RL = 44 hops but stays ENTIRELY at z=920 — a DIRECT platform route
  (1879,423,920 → 1759,-9 → 1543,-21 → ... → 715,99 = the RL). The finer sampling connects
  the z=920 platform that grid=24 misses. So FINER GRID does open the RL (no narrow-ledge
  drop needed — the platform itself connects).

BUT the weapon test is WORSE at grid=12: RL reach grid=24=6/24 vs grid=12=3/24. Because
grid=12 NAVIGATION is worse (spawn-to-spawn 24/24 vs ~14/24). The better connectivity is
cancelled by worse nav. => The RL is gated on grid=12 nav RELIABILITY, not connectivity.

### grid=12 nav: progress + remaining
Fixes that helped: pure-pursuit steering (grid=24 stable 24/24), corner-escape recovery
(steer toward find_best_direction's open dir during backoff; grid=12 11→~14-15/24),
MAX_CONNECT_CELLS=3 (sparse fine graphs, 90→27 edges/node). Tried + reverted: projection-
sync advance (broke orbit test, marginal), forward-node fallback (neutral/slightly worse).

Remaining grid=12 failure: bots get HINDERED ~20s pressing corner/wall-adjacent nodes;
high variance (10-18/24); count=10 = 4-8/10 (vs grid=24 10/10), so it's nav quality not
crowding. The reach/orbit/give-up progress logic is reach-based (density-sensitive) and
mismatched with pure-pursuit at 32u/tick over 12u nodes. NEXT (proposed): rewrite update()
progress + give-up to be PROJECTION-based (density-robust), removing the reach/orbit logic.
Substantial + risks the grid=24 24/24 — flag before doing.

### Infra wins this session (enable fast iteration):
- --spacing CLI + per-spacing cache dirs (data/mapcache/<spacing>/).
- cached_map_nav is LOAD-ONLY (fixed N× concurrent regen bug when spawning N bots).
- parallelized prune/jump/bridge (grid=24 regen 40s→2.6s; grid=12 10m→34s).

### grid=12 nav: what works vs what doesn't (clear pattern)
ARCHITECTURAL fixes worked: pure-pursuit steering (grid=24 stable 24/24), corner-escape
recovery (grid=12 11→14). PARAMETER/HEURISTIC tweaks ALL hurt or neutral: projection-sync
advance, forward-node fallback, progress-based give-up reset (dropped grid=24 to 16!),
wall-avoidance steering (grid=12 9-13). Reason: each tweak FIGHTS the existing reach/orbit/
give-up progress logic — two progress systems disagreeing at high density. The remaining
fine-grid stall (bots press corner/wall-adjacent nodes ~20s) needs an ARCHITECTURAL fix:
REPLACE the reach/orbit/give-up advancement with a single projection-native progress model
(advance + give-up + reached all driven by the bot's arc-length projection on the path).
Surgical add-ons to the current logic backfire; only a clean replacement will work. That is
a substantial, risky rewrite of NavigationDriver::update() — best done as its own plan, not
at the tail of a huge session. grid=24 stays 24/24; the RL is gated behind it.

---

## 2026-06-18 Session 7 — navmesh driver iteration: per-cell portals are too narrow (THE blocker)

Goal: drive navmesh spawn-to-spawn to 24/24, then RL. Baseline: navmesh ~4-7/24 (astar 24/24).

### Diagnosis (conclusive)
Ran count=8 navmesh, dumped per-bot trajectories. Two failure modes:
- **Stalled** (high hindered_frames 96-134): wedged against a wall, e.g. (1359,64,664) spd=0
  for 40s — same y=64 wall the astar grid=12 bots jammed on.
- **Lost/wander** (LOW hindered 2-14): moving smoothly but ending far from goal — one bot rode
  up to the z=928 platform while its goal was z=664. NOT a stall (recovery never fires).

Root cause found via `navinspect navpath ... <cell> <radius>` (added a radius arg): a path to a
FLAT goal had **56-70 points zig-zagging cell-by-cell** — the funnel was NOT straightening.
Varying the funnel portal inset:
  radius=16 → 56 pts | radius=8 → 56 | radius=4 → 53 | radius=0 → 25 pts.
=> **Per-cell portals are `cell_size` (16u) wide; insetting them by the agent radius (16u)
collapses them to near-points, pinning the funnel to cell centers.** A per-cell-quad navmesh
fundamentally can't inset portals — the portal (a single cell edge) is always ≤ the agent
radius. Even inset=0 leaves 25 jagged points (and then no wall clearance → hull-clips).

### What was tried on the driver (all regressed or neutral — the PATH is the problem)
- Remove update()'s clear-on-stall (so recovery corner-escapes with a live target): **2/24**.
  (Removing the clear lost a roam-fallback that was stumbling bots to goals.)
- Add a poly **blacklist** + `path_excluding` A* (route around wedged polys, astar-style): 3/8.
- inset=0 (straighter funnel): 2/8.
Net: blacklist+no-clear+inset0 = **2/24**, WORSE than the baseline 7/24. Lesson: the navmesh
paths are so jaggy that *roaming beats following them*, so no driver tweak gets past ~7/24.
The astar bots follow tight Q2 corridors fine (24/24) because their waypoint paths + pure-
pursuit are tuned; the navmesh's per-cell funnel paths are too jagged to follow.

### THE fix (next): rectangle-merged polygons
Merge walkable cells into maximal **rectangles** (z-coherent within walkable_climb). Then portals
are the *overlap of two rects' touching edges* — many cells wide → the agent-radius inset works
→ the funnel straightens AND clears walls. Also cuts polys 34k→hundreds (faster A*). This is
`rcBuildPolyMesh`'s role. Requires: Poly{ix,iy,w,h,oz}; portal() = edge overlap; nearest_poly =
point-in-rect; adapt bridge/adjacency. Keep the blacklist infra (committed) for the new driver.

### Kept (committed): `NavMesh::path_excluding` (blacklist A*) + `navinspect navpath <cell> <radius>`.

---

## 2026-06-18 Session 7 (cont.) — rectangle merge WORKS; remaining issue is vertical routes

Implemented greedy rectangle merge (34k cells → 550 wide-portal rects). Key wins:
- **Funnel now straightens flat runs** (e.g. a 992u straight segment at z=792 in one path)
  because wide rect portals survive the agent-radius inset (per-cell 16u portals collapsed).
- Reworked `bridge_components` to **cell-center** walkable_stair tests (rect-edge sampling
  under-bridged): q2dm1 9/10 spawns connected (was 4 components → 2). spawn6 (2016,-224,664),
  a 2-poly pocket ~600u out, still isolated — likely needs an off-mesh jump/drop link.
- **Bridge-point pinch fix** (big one): a bridge joins two big rects whose CENTER-midpoint is
  mid-air off the stair; the funnel was pinning the path there → bot aimed at mid-air and
  wandered (distance 6733, hindered 218). Now each bridge stores the actual connection point
  (where walkable_stair succeeded) and the funnel pinches THERE. Result: a single bot glides
  **bumps=0, wrong_turns=0, hindered_frames=0** — the funnel path is followable.

### Remaining blocker: descending/vertical routes
spawn-to-spawn navmesh still ~0/24. A bot from the z=920 platform toward a z=472 goal glides
smoothly down (920→616→496) but **overshoots/falls to z=352** (below the goal) and wanders
there, unable to climb back. Causes to chase next:
1. **Off-mesh DROP links** — the z=920 platform descends via ledges (one-way drops), which
   `walkable_stair` (climb-only) doesn't model. The bot falls off-mesh; the path (on the upper
   level) is then wrong → lost. Need jump-down/drop off-mesh connections (the astar graph has
   `detect_jump_edges`; port the idea).
2. **Ledge-aware following** — `pursue_target_safe` should reject look-aheads that walk off a
   ledge (segment_has_floor), forcing the bot to the modeled down-link instead of falling.
3. Build time: the cell-center bridge is ~6.7s (cached once/process, but slow for dev) — only
   scan boundary cells of non-largest rects to speed up.

Net: the navmesh GEOMETRY + funnel are now good (smooth, wall-clear, straight). The gap is
vertical traversal (drops) + the last spawn's connectivity. astar still 24/24 (unaffected).
