# Plan 15 — Scenario Nav Parity (spawn-to-spawn reached=success)

> **Status**: pending
> **Created**: 2026-06-15
> **Depends on**: Plans 10–14 (movement harness, LOS, steering, stuck-recovery, path quality)
> **Goal**: Make `cargo run --bin qbots -- spawn-to-spawn` exit with `reached=1`.
> **Agent**: ralph-loop

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: The scenario bot fails to reach the goal because `scenario.rs` is missing every
nav-augmentation step that fleet mode (`supervisor.rs` / `main.rs`) performs. The scenario
builds the raw grid graph and stops; the fleet seeds spawn nodes, adds jump edges, smooths
paths, presses jump on jump edges, and runs stuck recovery. Closing all five gaps is the
entire plan.

**Deliverables**:
1. Scenario nav graph construction mirrors supervisor (`seed_spawns` + `detect_jump_edges` +
   `spawns_in_largest_component` diagnostic).
2. `smooth_with_cm` called in the scenario tick loop (mirrors main.rs line 671).
3. Jump-edge action wired in scenario (`current_edge_is_jump → mv.jump()`).
4. `Recovery::evaluate` integrated into scenario tick loop (stuck → jump/strafe/repath).
5. Live run: `# SUMMARY reached=1 …` appears in the log.

**Estimated effort**: Small (2–3 h) — all changes in `scenario.rs`; zero new files.

---

## Context

### Why the scenario has been failing

Two failure modes are visible in the logs under `logs/spawn-to-spawn/`:

| Run | mean_speed | hindered | Symptom |
|-----|-----------|---------|---------|
| `1781565937` | 61 u/s | 4 | Bot travels 1829 u but **away from goal** (ends 2124 u from (2016,−224,664)) |
| `1781566280` | 27 u/s | 130 | Bot orbits wp 833 at (1471,1391,920) for 25+ s, never escapes |

Both failures trace back to the **same five missing lines**:

### Gap 1 — Nav graph not seeded (root cause of wrong-direction paths)

`scenario.rs:80`:
```rust
let graph = Arc::new(world::NavGraph::generate(&cm, (model.mins, model.maxs), 64.0));
```

The fleet code (`supervisor.rs:79-85`) does:
```rust
let mut g = world::NavGraph::generate(&cm, (m.mins, m.maxs), 64.0);
g.seed_spawns(&cm, &spawn_origins);          // ← MISSING in scenario
g.detect_jump_edges(&cm, 64.0);             // ← MISSING in scenario
g.spawns_in_largest_component(&spawn_origins); // ← MISSING in scenario
let graph = Arc::new(g);
```

Without `seed_spawns`, the node "nearest" to the goal `(2016,-224,664)` may be a grid square
far from the actual spawn location. A* then plans a path that traverses the component toward
that misaligned node — potentially heading the wrong direction for the full 30-second run.

### Gap 2 — No path smoothing

`smooth_with_cm` is called every tick in `main.rs:671` but never in `scenario.rs`. The scenario
bot follows every 64 u grid node, zigzagging at each one. String-pull smoothing collapses
straight-line segments → fewer waypoints → faster transit.

### Gap 3 — No jump action on jump edges

`main.rs:827-828` presses jump when `nav.current_edge_is_jump()`. The scenario has no such
check. Even after `detect_jump_edges` is added (Gap 1), the bot never presses jump → it
halts at a ledge indefinitely.

### Gap 4 — No stuck recovery

`main.rs:790-825` runs `Recovery::evaluate(...)` and applies `Jump`, `Strafe`, or
`BackOffThenRepath`. The scenario has no recovery. When the orbit-watchdog finally forces
a waypoint advance but the next waypoint is also unreachable, the bot grinds until
`GOAL_GIVEUP_TICKS=80` fires (8 s), replans to the same bad path, and repeats.

### Gap 5 — No component/connectivity diagnostic

The supervisor logs `spawns_in_largest_component`. The scenario never does, so we have no
visibility into whether spawn points are even reachable from each other. If Q2DM1's graph
is fragmented, the scenario needs to warn and the bot needs to handle cross-component goals.

### Pre-verified fix approach

The fleet already verifies that these calls work (8-bot fleet, no crashes). The scenario
fix is purely additive — no new logic, just wiring existing APIs correctly. The only risk is
Arc<NavGraph> mutability (seed/detect need `&mut NavGraph`), which is solved identically to
`supervisor.rs`: mutate before wrapping in Arc.

---

## Step-by-Step Tasks

### T1: Fix nav graph construction in scenario.rs

**File**: `crates/qbots/src/scenario.rs`

**What to do**: Replace the single `Arc::new(NavGraph::generate(...))` call with the
four-step pattern from `supervisor.rs`. The `spawn_origins` are already available via
`resolve_goal` (returned as the fourth element). However the lazy farthest-spawn pick
happens after graph construction, so we need the spawn list early. Use `bsp.spawn_points()`
directly (same as supervisor does).

**Before** (lines 80–90):
```rust
let graph = Arc::new(world::NavGraph::generate(
    &cm,
    (model.mins, model.maxs),
    64.0,
));
tracing::info!(
    map,
    nodes = graph.node_count(),
    edges = graph.edge_count(),
    "scenario nav graph"
);
```

**After**:
```rust
let bsp_spawns: Vec<[f32; 3]> = bsp
    .spawn_points()
    .iter()
    .map(|s| s.origin)
    .collect();
let mut graph_mut = world::NavGraph::generate(&cm, (model.mins, model.maxs), 64.0);
let seeded = graph_mut.seed_spawns(&cm, &bsp_spawns);
let added_jumps = graph_mut.detect_jump_edges(&cm, 64.0);
let (in_largest, total_spawns) = graph_mut.spawns_in_largest_component(&bsp_spawns);
tracing::info!(
    map,
    nodes = graph_mut.node_count(),
    edges = graph_mut.edge_count(),
    seeded,
    added_jumps,
    in_largest,
    total_spawns,
    "scenario nav graph (augmented)"
);
if in_largest < total_spawns {
    tracing::warn!(
        in_largest,
        total_spawns,
        "some spawns not in the largest nav component — cross-component routes may fail"
    );
}
let graph = Arc::new(graph_mut);
```

**Commit**: `task(T1): scenario nav graph — seed_spawns + detect_jump_edges + component diagnostic`

---

### T2: Add `smooth_with_cm` to scenario tick loop

**File**: `crates/qbots/src/scenario.rs`

**What to do**: After the `nav_driver.set_goal(...)` call in the ticker arm, add a
`smooth_with_cm` call. The `cm` Arc is already in scope.

**Before** (lines 193–194):
```rust
nav_driver.update(pos);
nav_driver.set_goal(NavGoal::Position(Vec3::from(goal)), pos);
```

**After**:
```rust
nav_driver.update(pos);
nav_driver.set_goal(NavGoal::Position(Vec3::from(goal)), pos);
nav_driver.smooth_with_cm(&cm, pos);
```

**Commit**: `task(T2): scenario tick — add smooth_with_cm after set_goal`

---

### T3: Add jump-edge action in scenario tick loop

**File**: `crates/qbots/src/scenario.rs`

**What to do**: After the forward/side move intent is built and before `move_ctrl.build_cmd`,
check `nav_driver.current_edge_is_jump()` and call `mv.jump()` if true. Mirror `main.rs:827`.

Find the block that sets `intent_forward` and builds `cmd`. The `mv` variable is the
`MovementIntent`. Add the jump check:

**Before** (just before `move_ctrl.set_delta_angles(...)`):
```rust
// (existing: mv.look_at, mv.move_forward, mv.move_side, intent_forward computed)
move_ctrl.set_delta_angles(frame.playerstate.pmove.delta_angles);
let cmd = move_ctrl.build_cmd(mv);
```

**After**:
```rust
// (existing: mv.look_at, mv.move_forward, mv.move_side, intent_forward computed)
if nav_driver.current_edge_is_jump() {
    mv.jump();
}
move_ctrl.set_delta_angles(frame.playerstate.pmove.delta_angles);
let cmd = move_ctrl.build_cmd(mv);
```

**Commit**: `task(T3): scenario tick — press jump on jump-link edges`

---

### T4: Add stuck recovery to scenario tick loop

**File**: `crates/qbots/src/scenario.rs`

**What to do**: Instantiate a `Recovery` before the loop. In the ticker arm, after building
`world_move_dir` and `view_yaw`, call `recovery.evaluate(...)` and apply the action to `mv`.
This mirrors `main.rs:790-825`. The scenario is never "engaging" (combat disabled), so
pass `engaging: false`. Also wire `BackOffThenRepath` to call `nav_driver.force_replan()`.

**Add import at top of file** (with existing brain imports):
```rust
use brain::recover::{Recovery, RecoveryAction};
```

**Before the loop**:
```rust
let mut recovery = Recovery::new();
```

**In the ticker arm, after `let (fwd, side) = move_from_world_dir(...)` and before
`move_ctrl.set_delta_angles`**:
```rust
// ── Stuck recovery (mirrors main.rs:790-825) ──────────────────────────────
let rec_action = recovery.evaluate(
    pos,
    dt,
    Some(&cm),
    view_yaw,
    pursue_pos.is_some(),
    false, // never engaging in scenario mode
);
match rec_action {
    RecoveryAction::None => {}
    RecoveryAction::Jump => {
        mv.jump();
    }
    RecoveryAction::Strafe { dir } => {
        mv.move_side(dir);
    }
    RecoveryAction::BackOffThenRepath => {
        mv.move_forward(-0.5);
        nav_driver.force_replan();
    }
    RecoveryAction::UseHeading(yaw) => {
        mv.look_at(yaw, 0.0);
        mv.move_forward(1.0);
    }
}
if nav_driver.current_edge_is_jump() {
    mv.jump();
}
```

Remove the standalone `if nav_driver.current_edge_is_jump()` block added in T3 (it's
included here in the right place after recovery).

**Commit**: `task(T4): scenario tick — stuck recovery (jump/strafe/repath)`

---

### T5: Live verification

**What to do**: With the server running q2dm1, run:
```bash
cargo run --bin qbots -- spawn-to-spawn
```
and observe the SUMMARY line. The exit code should be `0` (SUCCESS) and the log should show
`reached=1`. Run 2-3 times to confirm consistency across different spawn locations.

If reached=0 persists, examine the new diagnostic log line:
- `in_largest < total_spawns` → graph is fragmented; the cross-component fallback in
  `set_goal` routes to the nearest reachable node but may be on the wrong side. In this case
  bump the grid spacing from 64→48u as a follow-up (makes narrow passages passable; see
  Open Questions).
- `path_efficiency` close to 1.0 but `mean_speed` low → still stuck. Check the recovery
  `R` flag count in the log.
- `path_efficiency` < 0.7 → path is still going the wrong direction. The seed_spawns fix
  didn't fully close the gap — likely a major component split. Investigate with the component
  count in the log.

**Commit**: none (verification only — but update tracker).

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/qbots/src/scenario.rs` | T1: fix nav graph construction | P0 |
| `crates/qbots/src/scenario.rs` | T2: add smooth_with_cm | P0 |
| `crates/qbots/src/scenario.rs` | T3: jump on jump edges | P1 |
| `crates/qbots/src/scenario.rs` | T4: Recovery integration | P1 |

---

## Open Questions / Risks

1. **Q2DM1 graph fragmentation**: If `in_largest < total_spawns` remains true after T1, the
   bot may still fail on cross-component spawn pairs. Mitigation: reduce grid spacing from 64→48u
   (follow-up task in this plan or Plan 16). This roughly doubles node count but should connect
   most Q2DM1 corridors.

2. **`spawns_in_largest_component` accuracy**: The diagnostic calls `nearest()` (not seeded
   nearest), so it might report a spawn as "in largest" when the nearest grid node is in the
   large component but the actual spawn isn't seeded. After T1, seeded nodes are in the graph,
   so `nearest()` will find them exactly — accurate.

3. **smooth_with_cm performance**: Called every tick on the full path. For a 30-second run at
   10 Hz = 300 ticks × O(N) trace per node. Q2DM1 paths are ~20-80 nodes after seed_spawns;
   each smooth_path call is bounded by O(N × traces). Should be fast enough, but if it's slow
   we can gate it to "only when path changes" via a hash.

4. **Recovery loop bug**: If `BackOffThenRepath` fires and `force_replan()` clears the path,
   the next `set_goal` call replans. If the replan gives the same stuck route (same geometry),
   we'll repath and re-stick. Mitigation: the `StuckDetector` has a 5-second hard repath
   threshold, so repeated stuck→repath→stuck cycles naturally slow the loop. A follow-up
   could add temporary waypoint blacklisting.

5. **Jump edges + orbit watchdog interaction**: A jump edge means the target node is below a
   ledge. The orbit watchdog (ORBIT_RADIUS=80) might force-advance before the bot has time
   to run off the ledge. This is harmless — the bot will just start heading to the next node.

---

## Verification Checklist

- [ ] T1: `cargo build` + `cargo clippy` clean after nav graph construction change
- [ ] T1: Scenario log shows `scenario nav graph (augmented)` with `seeded=N added_jumps=M in_largest=K total_spawns=L`
- [ ] T2: `cargo build` clean; path log shows fewer waypoint hops (wpd gap is larger between transitions)
- [ ] T3: `cargo build` clean; jump edges in map produce `J` flag in recorder (or at least no crash on jump edges)
- [ ] T4: `cargo build` + `cargo clippy` clean; `R` flag appears in log when bot would have been stuck before
- [ ] T5: `# SUMMARY reached=1` in at least 2 of 3 consecutive `spawn-to-spawn` runs on q2dm1
