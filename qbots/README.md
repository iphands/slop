# qbots

A multi-threaded **Rust** program that connects to a real Quake 2 server over UDP and
impersonates genuine clients — external bots that log in like real players and fight in a
deathmatch using only what the server sends over the wire.

Unlike every classic Q2 bot (which run *inside* the server as `gamex86.dll` gamecode and
get `gi.trace()` / full entity access for free), qbots is a separate program on the far end
of a socket. It sees only the protocol traffic, so it rebuilds the world itself: the wire
codec, the connection handshake, frame decoding, and — the genuinely novel part — a `.bsp`
collision model + navigation graph parsed locally.

> Status: **connect → perceive the world → walk**, all verified live against a Yamagi Q2
> server. World model + brain + fleet are in progress. See `context/plans/SERIES.md`.

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
qbots config              # print server + paths, count loose maps, check q2dm1.bsp
qbots bsp-info q2dm1      # load a BSP and print geometry counts
qbots connect-one --name test   # connect a bot; uses config's server (no --addr needed)
```

(With `cargo`: prefix each with `cargo run -p qbots -- `, e.g.
`cargo run -p qbots -- bsp-info q2dm1`.)

---

## Testing what's implemented

### Plan 05 / T1 — the BSP loader (`bsp-info`)

This verifies the world crate reads real maps — loose or from `.pak` archives — and parses
the collision lumps correctly.

```bash
qbots bsp-info q2dm1
# q2dm1: v38 | 2408 planes, 2246 nodes, 2250 leafs, 960 brushes, 6802 brushsides, 3745 leafbrushes, 3 models
```

Try a few sources to confirm the pak logic:

| Map | Where it lives | Command |
|-----|----------------|---------|
| `q2dm1` | `pak1.pak` (stock DM) | `qbots bsp-info q2dm1` |
| `base1` | `pak0.pak` (stock SP) | `qbots bsp-info base1` |
| any custom map | `maps/<name>.bsp` (loose) | `qbots bsp-info <name>` |

Expected: a `v38` line with non-zero counts and no error. A map not found → `map 'x' not
found loose or in any pak under …`. `qbots config` reports whether `q2dm1.bsp` resolves
(it'll say `MISSING` as a loose file because it's inside `pak1.pak` — that's expected; the
loader finds it in the pak).

### Plan 03 — connection (`connect-one`)

```bash
qbots connect-one --name test_001
```

Watch the server logs: `test_001 connected` → `test_001 entered the game`. The bot appears
in `rcon status`. Ctrl-C to stop.

### Plan 04 — perception + movement

```bash
qbots connect-one --name test_002
```

A heartbeat prints every ~1 s once active:

```
qbots: Active frame=724532 ents=25 origin=(1512.0,-24.0,546.0)
```

- `frame` ticks up (~10 Hz) → server frames are decoding.
- `ents` → how many world entities are in the bot's PVS.
- `origin` → the bot's world position. Once it starts **changing**, the `clc_move`
  movement + checksum are accepted and the bot is walking.

---

## Project layout

```text
qbots/
├── AGENTS.md                 # architecture, protocol notes, constraints (read me)
├── README.md                 # this file
├── config.example.yaml       # copy to config.yaml
├── justfile                  # build gate (fmt/clippy/test/build)
├── context/
│   ├── plans/                # numbered plans + trackers; SERIES.md = dependency chain
│   └── distilled.md          # confirmed protocol/format facts
├── vendor/                   # READ-ONLY reference (yquake2 source, bot archive)
└── crates/
    ├── q2proto/              # wire codec: MSG_*, usercmd delta, InfoString, OOB, frames
    ├── client/               # connection FSM + netchan + frame loop + movement
    ├── world/                # .bsp / .pak loader → (T2 trace, T3 PVS, T4 nav graph)
    ├── brain/                # AI (planned)
    ├── qbots/                # the binary (CLI)
    └── tools/                # reusable utilities (planned)
```

## How it's built

- Each non-trivial change follows a plan in `context/plans/` (format in `RULES.md`),
  committed task-by-task as `task(TN): …`.
- The wire format is ported **verbatim** from `vendor/yquake2/src/` — source lines are
  cited in the code and in `context/distilled.md`.
- The full gate (`just all`) must stay green; clippy treats warnings as errors.

## Further reading

- `AGENTS.md` — the big-picture architecture and the key constraint (external ≠ gamecode).
- `context/plans/SERIES.md` — the plan dependency chain + milestones.
- `context/distilled.md` — confirmed protocol/format facts (handshake, netchan, frames,
  `clc_move` checksum, BSP/pak layout).
