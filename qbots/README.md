# qbots

A multi-threaded **Rust** program that connects to a real Quake 2 server over UDP and
impersonates genuine clients — external bots that log in like real players and fight in a
deathmatch using only what the server sends over the wire.

Unlike every classic Q2 bot (which run *inside* the server as `gamex86.dll` gamecode and
get `gi.trace()` / full entity access for free), qbots is a separate program on the far end
of a socket. It sees only the protocol traffic, so it rebuilds the world itself: the wire
codec, the connection handshake, frame decoding, and — the genuinely novel part — a `.bsp`
collision model + navigation graph parsed locally.

> **Status: full pipeline works live against Yamagi Q2.** A bot connects, perceives,
> navigates, fights, and respawns; an N-bot fleet fills a server. The world model (`.bsp`
> parse + trace + PVS + nav graph + navmesh), the combat/navigation brain, and the fleet
> supervisor are all complete and verified live. `spawn-to-spawn` reaches **24/24** on
> q2dm1 at the default grid spacing. The current frontier is fine-grid navigation
> reliability (which gates hard goals like the rocket-launcher platform) — see
> `context/nav_state_2026-06-18.md`. Full roadmap: `context/plans/SERIES.md`.

---

## Prerequisites

- Rust toolchain (stable; the repo pins it via `rust-toolchain.toml`).
- [`just`](https://github.com/casey/just) (command runner, for the build gate).
- A reachable Quake 2 server (e.g. Yamagi Q2 / q2pro) and its `baseq2` directory mounted
  locally so qbots can read the `.bsp` maps.

## Setup

1. Copy the config template and point it at your server + its `baseq2`:

   ```bash
   cp config.example.yaml config.yaml
   $EDITOR config.yaml
   ```

   ```yaml
   server:
     host: q2.example.com  # server hostname or IP (DNS-resolved at connect)
     port: 27910
   paths:
     server_cfg: /path/to/quake2/baseq2/server.cfg
     baseq2:     /path/to/quake2/baseq2
   # Fleet roster — `qbots run` spawns this many bots.
   fleet:
     count: 8
     name_prefix: qb
     qport_base: 28000
     connect_stagger_ms: 250
     reconnect: true
     max_reconnects: 0   # 0 = unlimited
     max_bots: 0         # 0 = uncapped; else hard cap (server maxclients headroom)
   ```

   `config.yaml` is gitignored (it holds machine-specific paths).

2. Sanity-check the build gate:

   ```bash
   just all      # fmt-check + clippy (-D warnings) + test + build, all must pass
   ```

## Running

The binary takes a `--config` (default `config.yaml`) and a subcommand. With `cargo`,
prefix each with `cargo run -p qbots -- ` (e.g. `cargo run -p qbots -- bsp-info q2dm1`).

### Playing

```bash
qbots connect-one --name <botname>   # connect a single bot and keep it alive
qbots run                            # launch the full fleet from config's [fleet] roster
qbots run --count 4 --skin male/grunt
qbots competition --count 8          # N bots per nav --mode at once + frag scoreboard
qbots status                         # query server (map + player list) — the fleet lens
```

`run` and `connect-one` honor `--addr`, `--qport`/`--qport-base`, and `--mode` (nav
backend, see below). `run` adds skin selection (`--skin model/skin`, `--skin-random-male`,
`--skin-random-female`) and `--name`/`--count` overrides.

### World-model inspection (no server needed)

```bash
qbots config             # print loaded config (server + paths + fleet) and exit
qbots bsp-info <map>     # load a BSP (loose or from pak) and print geometry counts
qbots trace <map>        # build the collision model and fire test rays from map center
qbots pvs <map>          # show PVS info (center cluster + how many clusters it sees)
qbots nav <map>          # generate the nav graph and find a corner-to-corner path
qbots nav-debug <map>    # diagnose disconnected nav-graph components (why spawns won't link)
```

### Nav-cache pregeneration

The nav graph is built once per map ahead of time, not per bot. Scenarios and the fleet
**load** the cache and fail with a clear message if it is missing or stale, so generate it
first:

```bash
qbots generate-map-cache --map q2dm1              # one map
qbots generate-map-cache --map 'q2dm*' --jobs 4   # glob, parallel (all 8 maps ~9.5s)
qbots generate-map-cache --map q2dm1 --spacing 12 # a finer grid (own cache dir)
```

Caches live under `data/mapcache/<spacing>/` (gitignored). Regenerate after any
graph-affecting change, or when changing `--spacing`.

---

## Navigation backends (`--mode`)

`connect-one`, `run`, `spawn-to-spawn`, and `spawn-to-weapon` take `--mode` to pick the
navigation backend. The steering loop is identical for all; only the path source differs:

| Mode | What it does |
|------|--------------|
| `astar` (default) | A* over the grid-sampled waypoint graph — the proven backend |
| `navmesh` | A* over walkable polygons + funnel/string-pull (Recast-style) |
| `hybrid-fallback` | A* primary; navmesh takes over a segment on a hard-stuck |
| `hybrid-race` | plan both per goal, run the cheaper-scoring one to completion |
| `hybrid-hier` | navmesh picks the corridor, A* executes a sliding local sub-goal |
| `hybrid-segment` | navmesh routes open space, A* owns jump-link segments only |

The `navmesh` and `hybrid-*` modes require the navmesh to be available (built lazily from
the map cache). `competition` spawns bots for every mode at once and prints a per-mode frag
scoreboard.

---

## Movement testing (scenarios)

Two scenarios measure how well a bot *moves*: each connects one bot like `connect-one`,
disables combat, pins the nav goal to a known target, samples every server frame, and dumps
a structured log + a `# SUMMARY` line.

```bash
# Farthest DM spawn from where the bot spawns.
qbots spawn-to-spawn [--count 24] [--max-secs 60] [--spacing 24] [--mode astar]
# A named weapon's BSP origin (resolved as weapon_<name>).
qbots spawn-to-weapon rocketlauncher [--count 24] [--max-secs 60]
```

- **Output**: `./logs/<scenario>/<unix_ts>.<bot>.log` — one frame per line (16 positional
  columns + a `flags` run: `B`=wall-bump, `W`=wrong-turn, `H`=hindered, `A`=airborne,
  `R`=recovery), ending in `# SUMMARY reached=… elapsed=… …`. Schema lives in
  `crates/brain/src/recorder.rs`. `./logs/` is gitignored.
- **Exit code**: `0` = reached the goal; `2` = ran to the cap without reaching it;
  `FAILURE` = setup/IO error. Multi-bot runs print an `N/M bots reached the goal` summary.
- **The map is autodetected from the server** (via the connectionless `status` query) — the
  nav graph and goal origins come from the BSP, so the loaded map must match the server's.
  Pass `--map <name>` only to override; a mismatch produces garbage navigation.

> Note: `--lift-penalty` (A* cost on elevator ride edges) is a **temporary hack** dodging a
> multi-bot `func_plat` deadlock until real wait/ride/step-off behaviour exists — see
> `context/elevator_todo.md`.

---

## Testing what's implemented

### BSP loader (`bsp-info`)

```bash
qbots bsp-info q2dm1
# q2dm1: v38 | 2408 planes, 2246 nodes, 2250 leafs, 960 brushes, ...
```

| Map | Location | Command |
|-----|----------|---------|
| `q2dm1` | `pak1.pak` (stock DM) | `qbots bsp-info q2dm1` |
| `base1` | `pak0.pak` (stock SP) | `qbots bsp-info base1` |
| custom map | `maps/<name>.bsp` (loose) | `qbots bsp-info <name>` |

A missing map → `map 'x' not found loose or in any pak under ...`. The loader searches
loose files then `pak0.pak` through `pak9.pak`.

### Connection & frame loop

```bash
qbots connect-one --name test_001
```

Watch the server logs: `test_001 connected` → `test_001 entered the game`. The bot appears
in `rcon status`. Per-tick log lines show frames decoding and the bot moving:

```
qbots: state=Active frame=724532 ents=25 origin=(1512.0,-24.0,546.0) fsm=Roam
```

- `frame` ticks up (~10 Hz) → server frames decoding successfully.
- `ents` → world entities visible in the bot's PVS.
- `origin` changing → the bot is walking; `fsm` → current behavior state.

---

## Project layout

```text
qbots/
├── AGENTS.md                 # architecture, protocol, constraints (read first; CLAUDE.md → this)
├── README.md                 # this file
├── config.example.yaml       # template → copy to config.yaml
├── justfile                  # build gate (fmt/clippy/test/build)
├── context/
│   ├── plans/                # numbered plans + trackers; SERIES.md = dependency chain
│   │   ├── completed/        #   done plans (historical record)
│   │   └── abandoned/        #   superseded plans (with a note on why)
│   ├── distilled.md          # protocol/format facts (read before new work)
│   ├── distilled/            #   per-bot AI research (3zb2, eraser, ace, …)
│   ├── pitfalls.md           # bugs/gotchas (read before new work)
│   └── nav_state_*.md        # latest navigation-quality handoff
├── data/mapcache/            # generated nav caches, by spacing (gitignored)
├── logs/                     # scenario movement logs (gitignored)
├── vendor/                   # READ-ONLY reference (yquake2 source, bot archive)
└── crates/
    ├── q2proto/             # wire codec: MSG_*, usercmd delta, InfoString, OOB, frames, CRC
    ├── world/               # .bsp/.pak loader → collision trace + PVS + nav graph + navmesh
    ├── client/              # connection FSM + netchan + frame parsing + movement
    ├── brain/               # combat (aim/lead/weapon) + nav + FSM + steering + recovery + heatmap
    ├── qbots/               # binary: CLI, config, fleet supervisor, scenarios
    └── tools/               # nav diagnostics: navinspect, gridscan, compgaps, bsp_verify
```

## How it's built

- Every non-trivial change follows a plan in `context/plans/` (format in `RULES.md`),
  committed task-by-task as `task(TN): …`. Completed plans move to `completed/`.
- Wire format ported **verbatim** from `vendor/yquake2/src/` — source lines cited in code
  and `context/distilled.md`.
- Full gate (`just all`) must stay green; clippy treats warnings as errors.
- Small, frequent commits. Never push — the human pushes after review.

## Further reading

- `AGENTS.md` — big-picture architecture and the key constraint (external ≠ gamecode).
- `context/plans/SERIES.md` — plan dependency chain + milestones (Plans 01–23).
- `context/nav_state_2026-06-18.md` — current navigation state, what works, and the
  next well-defined step (projection-native nav rewrite).
- `context/distilled.md` (+ `context/distilled/`) — protocol/format facts and per-bot AI
  research (handshake, netchan, frames, `clc_move` checksum, BSP/pak layout, collision/PVS).
- `context/pitfalls.md` — bugs and gotchas, especially multi-attempt fixes.
</content>
</invoke>
