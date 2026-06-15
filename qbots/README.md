# qbots

A multi-threaded **Rust** program that connects to a real Quake 2 server over UDP and
impersonates genuine clients — external bots that log in like real players and fight in a
deathmatch using only what the server sends over the wire.

Unlike every classic Q2 bot (which run *inside* the server as `gamex86.dll` gamecode and
get `gi.trace()` / full entity access for free), qbots is a separate program on the far end
of a socket. It sees only the protocol traffic, so it rebuilds the world itself: the wire
codec, the connection handshake, frame decoding, and — the genuinely novel part — a `.bsp`
collision model + navigation graph parsed locally.

> Status: **connect → perceive → walk** verified live against Yamagi Q2. World model (`.bsp`
> parse + trace + PVS + nav graph) complete. Brain (combat AI) and fleet (N bots) in progress.
> See `context/plans/SERIES.md` for the full roadmap.

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
     host: noir.lan        # server hostname or IP
     port: 27910
   paths:
     server_cfg: /mnt/noir/scratch/games/q2/baseq2/server.cfg
     baseq2:     /mnt/noir/scratch/games/q2/baseq2
   ```

   `config.yaml` is gitignored (it holds machine-specific paths).

2. Sanity-check the build gate:

   ```bash
   just all      # fmt-check + clippy (-D warnings) + test + build, all must pass
   ```

## Running

The binary takes a `--config` (default `config.yaml`) and a subcommand:

```bash
qbots config              # print server + paths, validate config
qbots bsp-info <map>      # load a BSP (loose or from pak) and print geometry counts
qbots connect-one --name <botname>  # connect a single bot to the server
```

(With `cargo`: prefix each with `cargo run -p qbots -- `, e.g.
`cargo run -p qbots -- bsp-info q2dm1`.)

---

## Testing what's implemented

### BSP loader (`bsp-info`) — World Model

Verifies the `world` crate reads maps from loose `.bsp` files or `.pak` archives and parses
the collision lumps correctly.

```bash
qbots bsp-info q2dm1
# q2dm1: v38 | 2408 planes, 2246 nodes, 2250 leafs, 960 brushes, 6802 brushsides, 3745 leafbrushes, 3 models
```

Try different sources to confirm pak loading:

| Map | Location | Command |
|-----|----------|---------|
| `q2dm1` | `pak1.pak` (stock DM) | `qbots bsp-info q2dm1` |
| `base1` | `pak0.pak` (stock SP) | `qbots bsp-info base1` |
| custom map | `maps/<name>.bsp` (loose) | `qbots bsp-info <name>` |

Expected: a `v38` line with non-zero geometry counts. Missing map → `map 'x' not found
loose or in any pak under ...`. The loader searches `pak0.pak` through `pak9.pak` in order.

### Connection (`connect-one`)

```bash
qbots connect-one --name test_001
```

Watch the server logs: `test_001 connected` → `test_001 entered the game`. The bot appears
in `rcon status`. Ctrl-C to disconnect.

### Frame loop & movement

Once connected, the bot receives server frames and can send movement commands:

```
qbots: Active frame=724532 ents=25 origin=(1512.0,-24.0,546.0)
```

- `frame` ticks up (~10 Hz) → server frames decoding successfully.
- `ents` → world entities visible in the bot's PVS.
- `origin` → bot's world position. When this changes, the bot is walking.

---

## Project layout

```text
qbots/
├── AGENTS.md                 # architecture, protocol, constraints (read first)
├── README.md                 # this file
├── config.example.yaml       # template → copy to config.yaml
├── justfile                  # build gate (fmt/clippy/test/build)
├── context/
│   ├── plans/                # numbered plans + trackers; SERIES.md = dependencies
│   ├── distilled.md          # protocol/format facts (read before new work)
│   └── pitfalls.md           # bugs/gotchas (read before new work)
├── vendor/                   # READ-ONLY reference (yquake2 source, bot archive)
└── crates/
    ├── q2proto/              # wire codec: MSG_*, usercmd delta, InfoString, OOB, frames
    ├── world/                # .bsp/.pak loader → collision trace + PVS + nav graph
    ├── client/               # connection FSM + netchan + frame parsing + movement
    ├── brain/                # combat AI (aim/lead/weapon-select + navigation)
    ├── qbots/                # binary: CLI, config, spawn N bots
    └── tools/                # reusable utilities (pcap-decode, bsp-dump, etc.)
```

## How it's built

- Every non-trivial change follows a plan in `context/plans/` (format in `RULES.md`),
  committed task-by-task as `task(TN): …`.
- Wire format ported **verbatim** from `vendor/yquake2/src/` — source lines cited in code
  and `context/distilled.md`.
- Full gate (`just all`) must stay green; clippy treats warnings as errors.
- Small, frequent commits. Move completed plans to `context/plans/completed/`.

## Further reading

- `AGENTS.md` — big-picture architecture and the key constraint (external ≠ gamecode).
- `context/plans/SERIES.md` — plan dependency chain + milestones.
- `context/distilled.md` — protocol/format facts (handshake, netchan, frames, `clc_move`
  checksum, BSP/pak layout, collision/PVS).
- `context/pitfalls.md` — bugs and gotchas, especially multi-attempt fixes.
