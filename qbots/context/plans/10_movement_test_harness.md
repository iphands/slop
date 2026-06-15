# Plan 10 — Movement Test Harness (`spawn-to-spawn` / `spawn-to-weapon`)

> **Status**: pending
> **Created**: 2026-06-15
> **Depends on**: Plan 09 (fleet/CLI/supervisor)
> **Goal**: Ship two CLI scenarios that drive one bot along a known route and dump a structured per-frame log, so movement quality (pathing accuracy + elapsed time) becomes *measurable* — the lens for Plans 11–14.

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Add BSP **entity-lump** parsing (exposes DM spawn points + `weapon_*`/`item_*`
origins keyed by classname), two new `qbots` subcommands — `spawn-to-spawn` and
`spawn-to-weapon --weapon-name <name>` — and a `MovementRecorder` that samples every
server frame and dumps a structured log to `./logs/<scenario>/<unix_ts>.<bot>.log`.

**Deliverables**:
1. `world::Bsp` parses `LUMP_ENTITIES` (index 0) → `Vec<BspEntity>`; helpers
   `spawn_points()` + `find_class(classname)`.
2. `qbots spawn-to-spawn [--map] [--addr] [--name]` — connect one bot, pick the
   **farthest DM spawn** from its current origin, navigate there, log, stop.
3. `qbots spawn-to-weapon --weapon-name rocketlauncher [--map] [--addr] [--name]` —
   navigate to that weapon's BSP-spawn origin, log, stop.
4. `MovementRecorder` producing the per-frame record below; wall-bump / wrong-turn /
   hindered detection via the existing `CollisionModel::trace`.
5. End-of-run summary line: elapsed time, distance traveled, mean speed,
   bumps/wrong-turns/hindered counts, reached-goal bool.

**Estimated effort**: Medium (1 day) — entity parse is small; the recorder + scenario
wiring is the bulk; reuses `connect-one`'s connect/loop scaffolding.

---

## Context

### Why this first

Every other plan in this series (11–14) changes *how* a bot moves. We currently have no
way to tell whether a change helped. Per-bot `tracing` logs are coarse (deduped, no
structured per-frame record, no bump/wrong-turn accounting). **Elapsed time to a known
goal is the headline ability metric** the user asked for ("bots should move faster than
they do today"), and it is meaningless without a repeatable route + a recorder.

This plan is *measurement only* — it does **not** fix any movement bug. It deliberately
runs against the **current** steering code so the first logs establish a baseline. Plans
11–14 then re-run the same scenarios and compare.

### No cheating

The recorder must catch physics violations, not cause them. The bot drives the same
`MovementController`/`clc_move` path as `run`; the harness only *observes*. Expected
speed is bounded by `pm_maxspeed=300 u/s` (distilled.md physics oracle). If a log shows
sustained >~320 u/s grounded horizontal speed, that's a bug to flag, not a feature.

### Key facts

- **BSP entity lump is currently unparsed.** `crates/world/src/bsp.rs` reads lumps
  `{PLANES, VISIBILITY, NODES, LEAFS, LEAFBRUSHES, MODELS, BRUSHES, BRUSHSIDES}` only.
  `LUMP_ENTITIES = 0` (`files.h:273`) is a single NUL-terminated text block of
  `{ "classname" "..." "origin" "x y z" "angle" "..." ... }` entries. Trivial to parse;
  no binary structs. See `vendor/yquake2/src/common/filesystem.c` (load) and
  `vendor/yquake2/src/game/g_spawn.c` (`G_ParseEntity` / `spawn_fx` key parsing) for the
  text format.
- DM spawn classname is `info_player_deathmatch`; the rocket launcher pickup is
  `weapon_rocketlauncher`; mega-health `item_health_mega`; etc. (canonical Q2 classnames —
  confirm against the target map's entity dump in T1.)
- The `Cmd::Nav` subcommand (`main.rs:1000-1093`) already shows the pattern: load BSP,
  `CollisionModel::from_bsp`, `NavGraph::generate(&cm, bounds, 64.0)`, find farthest node.
  We mirror it — but the "farthest" target is a **spawn point** (Plan 10) not a graph node,
  and we actually *drive* a bot there instead of just printing a path.
- `connect-one` (`supervisor::run_single`) owns the connect + tick loop; the per-frame
  body is `bot_task` (`main.rs:374-770`). The recorder hooks in there.
- The `time` crate (already a dep) gives Unix timestamps for the log filename.

---

## Step-by-Step Tasks

### T1: Parse the BSP entities lump

**File**: `crates/world/src/bsp.rs`

**What to do**: Add `LUMP_ENTITIES = 0`, a `BspEntity { classname: String, fields: HashMap<String,String> }`
type, parse the lump's NUL-terminated text into `Vec<BspEntity>`, store as `Bsp::entities`,
and expose:
- `BspEntity::origin() -> Option<[f32;3]>` (parse the `"origin" "x y z"` triplet).
- `BspEntity::angle() -> Option<f32>`.
- `Bsp::spawn_points() -> Vec<SpawnPoint>` filtering `classname == "info_player_deathmatch"`
  (also keep `info_player_start` as a fallback for maps short on DM spawns).
- `Bsp::find_class(classname) -> Vec<&BspEntity>`.

The text parser: split on `}`'s, within each entity tokenize the quoted-string pairs.
Keys seen in DM maps: `classname`, `origin`, `angle`, `target`/`targetname`, `spawnflags`.
Don't over-engineer — store all key/value pairs in the map; helpers read what they need.

**Tests** (`crates/world/src/bsp.rs` `#[cfg(test)]`):
- Round-trip a synthetic entities block → `spawn_points()` returns 2 with parsed origins.
- `find_class("weapon_rocketlauncher")` returns the RL entity with a correct origin.
- A real-map smoke test is covered by the new CLI in T4 (no need to ship map bytes).

**Verify**: `cargo test -p world`; `cargo clippy -p world -- -D warnings`.

### T2: `MovementRecorder` — the per-frame record + detectors

**File**: `crates/brain/src/recorder.rs` (new), export from `brain/src/lib.rs`.

**What to do**: A struct that owns an `Arc<CollisionModel>` (for the forward-bump trace)
and the planned node path (for wrong-turn detection), plus an accumulating log. Sample
once per server frame:

```rust
pub struct FrameRecord {
    pub t_secs: f32,            // elapsed since scenario start
    pub frame: i32,             // serverframe
    pub origin: [f32; 3],
    pub velocity: [f32; 3],
    pub speed: f32,             // |horizontal velocity|
    pub view_yaw: f32,          // actual view yaw (from playerstate)
    pub view_pitch: f32,
    pub move_yaw: f32,          // yaw of the velocity heading (NaN if ~still)
    pub facing_move_delta_deg: f32, // |view_yaw - move_yaw| wrapped to ±180
    pub waypoint: Option<usize>,
    pub waypoint_dist: Option<f32>,
    pub goal_reached: bool,
    pub wall_bump: Option<WallBump>,    // None this frame, or a bump
    pub wrong_turn: bool,               // moved away from current waypoint
    pub hindered: bool,                 // speed < HINDER_THRESH while intending to move
    pub grounded: bool,
}
pub struct WallBump { pub endpos: [f32;3], pub normal: [f32;3], pub dist: f32 }
```

Detectors (all reuse existing `world::CollisionModel::trace`; **no new physics**):
- **wall_bump**: trace from origin along the view-forward vector (built from `view_yaw`,
  pitch flattened) for `BUMP_PROBE = 48.0` with the player hull (`HULL_MINS`/`HULL_MAXS`,
  `MASK_SOLID`); if `fraction < 1.0` and the hit normal is near-vertical
  (`normal.z.abs() < 0.3`) → record. Throttle to one bump per `BUMP_COOLDOWN = 0.4 s` so a
  sustained grind logs as a few events, not 100.
- **wrong_turn**: project `(origin - prev_origin)` onto the unit vector toward the current
  waypoint; if negative and `|movement| > 5.0` → the bot advanced away from its goal.
- **hindered**: `grounded && speed < HINDER_SPEED = 100.0` while the last intent had
  `forward.abs() > 0.5` (captured via a `set_intent_forward(f32)` hook the tick calls).
- **goal_reached**: 3D distance to the scenario goal `< GOAL_TOL = 48.0`.

Provide `Recorder::summary() -> RunSummary { elapsed_secs, distance, mean_speed,
bumps, wrong_turns, hindered_frames, reached, max_speed }` and `Recorder::dump(&path)`
writing the log (T3 format). `distance` = cumulative `|Δorigin|` over sampled frames.

**Tests** (`crates/brain/tests/recorder.rs`):
- A synthetic frame stream moving straight to a goal at 300 u/s → `reached=true`,
  `bumps=0`, `wrong_turns=0`, `mean_speed≈300`, `elapsed≈dist/300`.
- A stream that walks into a wall (feed a fake trace result via a trait, or construct a
  1-brush `CollisionModel`) → `bumps>=1`, `hindered` true on the stalled frames.
- `summary()` distance monotonic; `dump()` writes a parseable file.

**Verify**: `cargo test -p brain`; `cargo clippy -p brain -- -D warnings`.

### T3: Log format + dump

**File**: `crates/brain/src/recorder.rs`

**What to do**: Write `./logs/<scenario>/<unix_ts>.<bot>.log` (create dirs; never crash on
IO failure — log a warning). Format (human-readable, grep/awk-friendly; one line per frame):

```
# qbots movement log  scenario=spawn-to-weapon  bot=qb0  map=q2dm1  goal=(512,-128,24)
# goal_classname=weapon_rocketlauncher  started=2026-06-15T12:00:00Z
# t frame  x y z  vx vy vz  speed  yaw pitch  move_yaw  face_delta  wp wpd  flags
0.000 4021  512 -640 24  0 0 0  0  90 0  nan 0  3 96.0  .
0.100 4022  512 -610 24  0 300 0  300  90 0  90 0  3 66.0  .
...
# SUMMARY reached=1 elapsed=8.42 distance=2104 mean_speed=249 max_speed=311 bumps=2 wrong_turns=1 hindered_frames=14
```

`flags` is a small char set per line: `B`=wall_bump, `W`=wrong_turn, `H`=hindered,
`A`=airborne, `.`=none. Choose separators so `awk -F'  '` works; document the schema in a
header comment block at the top of `recorder.rs`.

**Verify**: eyeball a real dumped log in T5; assert in T2's dump test that the SUMMARY line
parses (regex `reached=(\d) elapsed=([\d.]+)`).

### T4: CLI subcommands + scenario runner

**File**: `crates/qbots/src/main.rs` (add `Cmd` variants + handlers); new module
`crates/qbots/src/scenario.rs` for the runner.

**What to do**:

1. Add to the `Cmd` enum (`main.rs:37-73`):
   ```rust
   /// Drive one bot from spawn to the farthest DM spawn point; log movement; stop.
   SpawnToSpawn { map: Option<String>, addr: Option<String>, name: Option<String> },
   /// Drive one bot from spawn to a named weapon's BSP origin; log; stop.
   SpawnToWeapon {
       weapon_name: String,   // e.g. "rocketlauncher"
       map: Option<String>,
       addr: Option<String>,
       name: Option<String>,
   },
   ```
2. `scenario.rs::run_scenario(cfg, addr, name, goal: ScenarioGoal, max_secs) -> ExitCode`:
   - Resolve the map (default to the server's current map via `status`, else
     `cfg`/first DM map) and load BSP → `spawn_points()` / `find_class("weapon_<name>")`.
   - Build nav graph (reuse the `Nav` path: `CollisionModel::from_bsp`, `NavGraph::generate`).
   - Connect one bot (reuse `connect-one`'s connect handshake; factor it out of
     `supervisor::run_single` if it isn't already a callable).
   - On first frame, record the spawn origin; **compute the goal**: for `SpawnToSpawn`,
     the DM spawn point farthest (3D) from the bot's origin; for `SpawnToWeapon`, the
     weapon entity's origin (error out if absent on this map).
   - Each tick: drive the bot toward the goal via the **existing** nav + steering code
     (`NavigationDriver::set_goal(NavGoal::Position(goal))`, then the current
     `main.rs:647-675` intent build). Feed the chosen `forward` into `recorder.set_intent_forward`.
     Sample the recorder. **Disable combat** for the scenario (no firing, ignore enemies)
     so the run measures pure navigation — set a `scenario_mode` flag the tick checks.
   - Stop conditions: `goal_reached` for `GOAL_SETTLE = 0.5 s`, or `max_secs=30` elapses,
     or the bot dies (re-spawn resets origin — keep going but note it).
   - On stop: `recorder.dump(...)`, print the SUMMARY line to stdout, return `ExitCode`.
3. The bot must still send `clc_move` at the normal cadence and honor `rate` — it's a real
   client, only its *brain* is pinned to the scenario goal. Do **not** teleport or set
   velocity directly (that would cheat).

**Verify**: `cargo build -p qbots`; `cargo clippy -p qbots -- -D warnings`.

### T5: Baseline run + documentation

**File**: `qbots/CLAUDE.md` (a short "Movement testing" subsection) + a sample log checked
into a new `examples/` or just referenced.

**What to do**:
1. Against a live server (or a local Yamagi/q2pro DM), run:
   `cargo run --bin qbots -- spawn-to-spawn` and
   `cargo run --bin qbots -- spawn-to-weapon --weapon-name rocketlauncher`.
2. Confirm a log lands in `./logs/...` with a sane SUMMARY (reached=1 ideally; if the
   baseline bot fails to reach — likely, given the diagnosed bugs — that is the *expected*
   baseline and the headline motivation for 11–14).
3. Note the baseline elapsed time / bumps / wrong-turns in the tracker's "Baseline"
   section. These numbers are what 11–14 must beat.
4. Add `.gitignore` entry for `/logs/` (generated output — Constraints #5). **Do not**
   commit log files themselves.

**Verify**: two green runs; `/logs/` gitignored; SUMMARY printed.

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/world/src/bsp.rs` | Parse `LUMP_ENTITIES`; `BspEntity`, `spawn_points()`, `find_class()` | P0 |
| `crates/world/src/lib.rs` | Re-export `BspEntity`, `SpawnPoint` | P0 |
| `crates/brain/src/recorder.rs` | NEW — `MovementRecorder`, `FrameRecord`, detectors, dump | P0 |
| `crates/brain/src/lib.rs` | Export `recorder` | P0 |
| `crates/qbots/src/scenario.rs` | NEW — `run_scenario`, goal resolution, scenario tick wiring | P0 |
| `crates/qbots/src/main.rs` | `SpawnToSpawn`/`SpawnToWeapon` `Cmd` variants + dispatch; `scenario_mode` flag in `bot_task` | P0 |
| `crates/qbots/src/supervisor.rs` | Factor the connect handshake out of `run_single` for reuse | P1 |
| `.gitignore` | `/logs/` | P0 |

---

## Open Questions / Risks

1. **Map resolution**: the `status` OOB reply on this server omits `map` (Plan 09 note).
   Mitigation: accept `--map`; default to a known DM map (e.g. `q2dm1`) from config; error
   clearly if the BSP can't be found under `cfg.paths.baseq2`.
2. **Weapon classname spelling**: is it `weapon_rocketlauncher` (Q2 standard) on the target
   map? Mitigation: T1 dumps all entities for the map; T4 does a fuzzy match
   (`weapon_name` lowercased, prefix `weapon_`, also accept `rocket_launcher`). Surface a
   "available weapons: …" hint on miss.
3. **Goal on a non-walkable origin**: a weapon floating or on a ledge the grid nav graph
   didn't sample. Mitigation: `NavGoal::Position` already falls back to `nearest_reachable_from`
   (`nav.rs:117`); the recorder's `goal_reached` uses the *entity* origin, so if the bot can
   get within `GOAL_TOL` of the entity it still counts. Document if a map's RL is unreachable.
4. **Recorder perf**: tracing + a trace-per-frame per bot is cheap at 1 bot/10 Hz. Keep the
   recorder off the fleet `run` path (gate behind `scenario_mode`) so 8-bot runs don't pay it.
5. **Death mid-run**: a real DM server has other players/bots shooting our test bot.
   Mitigation: run on an empty server, or accept deaths and let the bot re-path from the new
   spawn (the recorder keeps a continuous clock; note respawns in flags). Prefer empty server
   for baseline purity.
6. **`connect-one` refactor risk**: if the handshake is tangled into `run_single`, extracting
   it could touch reconnect logic. Mitigation: keep T4's connect call thin — duplicate the
   ~20-line handshake into `scenario.rs` if a clean factor-out is risky, and leave a TODO to
   unify. Prefer working baseline over a perfect refactor.

---

## Verification Checklist

- [ ] T1: `cargo test -p world` — entity parse round-trips; `spawn_points()` + `find_class()` correct.
- [ ] T1: `cargo clippy -p world -- -D warnings` clean.
- [ ] T2: `cargo test -p brain` — recorder straight-line, wall-bump, and summary/dump tests green.
- [ ] T2: `cargo clippy -p brain -- -D warnings` clean.
- [ ] T3: dumped log header + per-frame rows + SUMMARY line parse with the documented schema.
- [ ] T4: `cargo build -p qbots` exit 0, zero warnings; `cargo clippy -p qbots -- -D warnings` clean.
- [ ] T4: `qbots spawn-to-spawn` connects one bot, drives it, and exits with a SUMMARY line.
- [ ] T4: `qbots spawn-to-weapon --weapon-name rocketlauncher` resolves the RL origin and drives there.
- [ ] T5: `/logs/` is gitignored; no log bytes committed.
- [ ] T5: baseline SUMMARY numbers recorded in the tracker (reached/elapsed/bumps/wrong_turns).
- [ ] No cheating: grounded horizontal speed never sustainably > ~320 u/s in any baseline log.
