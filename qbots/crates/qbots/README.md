# qbots — External Quake 2 Bot Clients

**The binary:** Connects N bot clients to a real Quake 2 server over UDP and watches
them frag each other in a deathmatch.

> **Status:** Full pipeline works live against Yamagi Q2. A bot connects, perceives,
> navigates, fights, and respawns. An N-bot fleet fills a server.

---

## Prerequisites

- **Rust** (stable; pinned via `rust-toolchain.toml`).
- **[`just`](https://github.com/casey/just)** — command runner for the build gate.
- **A Quake 2 server** (Yamagi Q2 / q2pro) running a deathmatch map.
- **The `baseq2` directory** mounted locally so qbots can read `.bsp` maps.

---

## Setup

1. **Edit the config** with your server IP and baseq2 path:

   ```bash
   cp config.example.yaml config.yaml
   nano config.yaml  # or vim, $EDITOR, etc.
   ```

   **Minimal config for qbots only:**
   ```yaml
   server:
     host: 192.168.1.100  # your server IP
     port: 27910
   paths:
     baseq2: /home/user/quake2/baseq2
   fleet:
     count: 8
   ```

   > **Note:** `server_cfg` is qctrl's job (RCON). qbots only needs `baseq2`.

2. **Generate nav cache** for your map (required before running bots):

   ```bash
   cargo run -p qbots -- generate-map-cache --map q2dm1 --spacing 24
   ```

3. **Sanity-check the build:**

   ```bash
   just all  # fmt + clippy + test + build
   ```

4. **Verify your setup:**

   ```bash
   cargo run -p qbots -- bsp-info q2dm1  # Does BSP loading work?
   cargo run -p qbots -- status          # Can you reach the server?
   ```

---

---

## Quick Start

Want to run bots **right now**? Here's the 5-second path:

1. Edit `config.yaml` with your server IP
2. Generate nav cache: `cargo run -p qbots -- generate-map-cache --map q2dm1`
3. Run bots: `cargo run -p qbots -- run --count 3`
4. Watch them frag: `cargo run -p qbots -- status`

---

## Running

The binary takes a `--config` (default `config.yaml`) and a subcommand. With `cargo`,
prefix each with `cargo run -p qbots -- ` (e.g. `cargo run -p qbots -- bsp-info q2dm1`).

> **⚠️ Required before running bots:** Generate nav cache first with
> `generate-map-cache --map <server-map>`. The bots will fail if the cache is missing.

### Connect a Single Bot

```bash
cargo run -p qbots -- connect-one --name mybot
```

The bot connects, appears in `rcon status`, and starts roaming/fragging.

### Run the Full Fleet

```bash
cargo run -p qbots -- run --count 12 --skin male/grunt
```

Spawns 12 bots with staggered connections.

### Competition Mode

```bash
cargo run -p qbots -- competition --count 8 --navmodes astar,navmesh --brains main,q3
```

Spawns bots for each (navmode, brain) combination and prints a frag scoreboard.

### Movement Testing

```bash
# Drive a bot to the farthest spawn point
cargo run -p qbots -- spawn-to-spawn --max-secs 60

# Drive a bot to a weapon
cargo run -p qbots -- spawn-to-weapon rocketlauncher
```

Logs movement metrics to `./logs/spawn-to-spawn/`.

---

## CLI Commands

| Command | Description |
|---------|-------------|
| `connect-one` | Connect one bot and keep it alive. |
| `run` | Launch the full fleet from config. |
| `competition` | N bots per (navmode, brain) group + scoreboard. |
| `config` | Print loaded config and exit. |
| `status` | Query server (map + player list). |
| `bsp-info <map>` | Load a BSP and print geometry counts. |
| `trace <map>` | Build collision model + fire test rays. |
| `pvs <map>` | Show PVS info (cluster visibility). |
| `nav <map>` | Generate nav graph + find a path. |
| `nav-debug <map>` | Diagnose disconnected nav components. |
| `spawn-to-spawn` | Movement test: spawn → farthest spawn. |
| `spawn-to-weapon <name>` | Movement test: spawn → weapon origin. |
| `generate-map-cache` | Pre-generate nav caches for one or more maps. |

---

## Navigation Backends (`--navmode`)

| Mode | Description |
|------|-------------|
| `astar` (default) | A* over grid-sampled waypoints. |
| `navmesh` | A* over walkable polygons + funnel. |
| `hybrid-fallback` | A* primary; navmesh takes over on hard-stuck. |
| `hybrid-race` | Plan both, run the cheaper-scoring one. |
| `hybrid-hier` | Navmesh picks corridor, A* executes sub-goals. |
| `hybrid-segment` | Navmesh routes open space, A* handles jumps. |

See `docs/BRAINS.md` for the full brain catalog.

---

## Movement Testing

The `spawn-to-spawn` and `spawn-to-weapon` scenarios measure navigation quality:

- **Output:** `./logs/<scenario>/<timestamp>.<bot>.log` — one frame per line.
- **Summary:** `# SUMMARY reached=... elapsed=... hindered=...`
- **Exit code:** `0` = reached goal, `2` = timed out, `FAILURE` = setup error.

Run with `--count N` to test N bots in parallel.

---

## Map Caching

Nav graphs are expensive to generate. Pre-generate them:

```bash
cargo run -p qbots -- generate-map-cache --map q2dm1 --spacing 24
cargo run -p qbots -- generate-map-cache --map 'q2dm*' --jobs 4
```

Caches live in `data/mapcache/<spacing>/` (gitignored).

---

## Observability

### Per-Bot Logs

Every bot emits structured logs:

```
0001.234 I bot connected
0002.456 I state=Active frame=123 ents=15 origin=(1512,-24,546) fsm=Roam
0003.567 I *** FRAG *** frags=1 gained=1
```

### Fleet Status

Query the server to see connected bots:

```bash
cargo run -p qbots -- status
```

Output includes player names, frags, and connection times.

---

## Configuration

### `config.yaml`

```yaml
server:
  host: noir.lan
  port: 27910
paths:
  baseq2: /home/user/quake2/baseq2
fleet:
  count: 8
  name_prefix: qb
  qport_base: 28000
  reconnect: true
  max_reconnects: 0  # 0 = unlimited
  max_bots: 0        # 0 = uncapped
```

### CLI Overrides

- `--addr <host:port>` — override server address.
- `--name <prefix>` — override bot name prefix.
- `--count <n>` — override fleet size.
- `--skin <model/skin>` — set bot skin.
- `--navmode <mode>` — select navigation backend.
- `--brain <kind>` — select decision plugin.

---

## Project Layout

```
qbots/
├── crates/
│   ├── q2proto/     # Wire codec (protocol 34)
│   ├── world/       # BSP parse → collision + nav graph
│   ├── client/      # Connection + frame loop
│   ├── brain/       # AI decisions (nav, combat, FSM)
│   └── qbots/       # Binary (CLI, fleet supervisor)
├── data/mapcache/   # Generated nav caches (gitignored)
├── logs/            # Movement test logs (gitignored)
├── context/         # Plans, distilled knowledge, pitfalls
└── vendor/          # yquake2 source, bot archive (read-only)
```

---

## Further Reading

- **`AGENTS.md`** — Architecture, protocol, constraints.
- **`context/plans/SERIES.md`** — Plan dependency chain (Plans 01–43).
- **`context/distilled.md`** — Protocol/format facts.
- **`context/pitfalls.md`** — Bugs and gotchas.
- **`docs/BRAINS.md`** — Brain plugin catalog.

---

## License

MIT / Apache-2.0.
