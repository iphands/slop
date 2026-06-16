# qbots — External Quake 2 Bot Clients

A multi-threaded **Rust** program that connects to a real Quake 2 server
(Yamagi Q2 / q2pro) **over the network** and impersonates genuine clients.
Each bot logs in like a real player, runs the connection loop itself, and
fights in a deathmatch using only what the server sends over the wire.

> **Sister project:** [`../qctrl`](../qctrl/AGENTS.md) drives the *same* Q2 server via RCON.
> qctrl is the operator (out-of-band control); qbots is the players (in-game clients).
> Share protocol notes — RCON and the client handshake are both connectionless UDP.

---

## The One Thing That Makes This Hard

**Every classic Q2 bot in `vendor/` is a *gamecode plugin*** (`gamex86.dll`). It runs
*inside* the server, calls `gi.trace()`, reads `g_edicts[]` directly, and links itself
into the world. It has **perfect, free, omniscient knowledge** of the map and every entity.

**qbots is none of that.** qbots is an *external* program on the far end of a UDP socket.
It sees **only what the server chooses to send** — entity deltas within the client's
Potentially-Visible-Set (PVS), sounds, prints, and configstrings. There is no `trace()`,
no `g_edicts`, no free map.

So the bot-AI *ideas* (aiming, route selection, weapon choice) translate, but the
*mechanisms* (trace, pointcontents, line-of-sight, full entity table, the BSP) **do not**.
Two whole subsystems have no precedent in the archive and must be built from scratch:

1. **The network protocol layer** — speak the Q2 client wire format byte-for-byte.
2. **World reconstruction** — rebuild a usable world model (geometry, LOS, nav graph)
   from a `.bsp` file we parse ourselves and/or from observed entity traffic.

This file treats those two as first-class. The bot-brain logic sits on top.

---

## Project Goal

Spawn N independent bot clients that connect to a running deathmatch server and behave
like competent human players: navigate the map, collect items, fight, respawn — at high
throughput and low CPU.

### Core Features
- **Protocol-accurate client**: full connect handshake, netchan, per-frame `usercmd`.
- **Per-bot isolation**: each bot owns its socket, state machine, and brain; shared
  read-only world data (nav graph, configstring tables).
- **Self-contained world model**: `.bsp` parser + navigation graph + line-of-sight,
  rebuilt without the game DLL.
- **Pluggable brain**: combat AI (aim/lead/weapon-select) + navigation (pathfind +
  item/roam goals) inspired by 3ZB2 / Eraser / ACE algorithms.
- **Observability**: per-bot logs, optional packet capture for debugging the wire format.

---

## Critical Facts About Quake 2 Maps

**ALL spawn points on ALL q2dm* maps are accessible from each other.** This is a fundamental design requirement of Quake 2 deathmatch maps - players must be able to reach all spawn points.

If our bots cannot reach spawn points, it is **NOT** because:
- The map has "inaccessible areas"
- The nav graph is "naturally fragmented"
- Some spawns require stairs, ramps, elevators, or long travel paths

It IS because we have **BUGS** in:
- BSP parsing (incorrect geometry, wrong lump sizes, endian issues)
- Collision model (incorrect trace, wrong contents flags, broken hull traces)
- Nav graph generation (nodes at wrong Z levels, edges through geometry, incorrect walkable detection)
- Bridge logic (connecting nodes that aren't actually walkable)

**The maps DO have spawn points on different levels that require stairs, ramps, or elevators to reach - but NONE are inaccessible.** Some paths may be long (potentially exceeding our 30s test timeout for slow bots), but all spawns are reachable. If our implementation says a spawn is unreachable, our implementation is wrong.

---

## Architecture

### Stack
- **Language:** Rust (edition current). No frontend, no server — pure client(s).
- **Async:** `tokio` (UDP I/O, timers, one task per bot). `tokio::net::UdpSocket`.
- **Byte codec:** hand-rolled little-endian readers/writers over `bytes::BytesMut`.
  The Q2 message API is tiny (`MSG_ReadByte/Short/Long/Float/String/Pos/Dir/Delta`).
- **Math:** `glam` (`Vec3`, quaternions) — matches Q2's float vec3 math.
- **Config:** `serde` + TOML/YAML (server addr, bot roster, skill params).
- **Build/verify:** `just` recipes (see `../qctrl/justfile` for the pattern),
  `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`.

### Workspace Layout
```text
qbots/
├── AGENTS.md                 # This file
├── context/
│   ├── plans/                # Active plans — read RULES.md & SERIES.md FIRST
│   │   ├── RULES.md          # Plan format + per-task build/commit rules (authoritative)
│   │   ├── SERIES.md         # Cross-plan dependency chain (NN → NN)
│   │   ├── completed/        # Done plans (historical examples)
│   │   └── NN_name.md        # Each non-trivial change gets a plan + tracker
│   ├── distilled.md          # Protocol/AI/algorithm learnings (read before new work)
│   ├── pitfalls.md           # Bugs & wire-format gotchas (read before new work)
│   └── high_level.md         # Crate/library pros-cons (tokio vs async-std, glam vs nalgebra…)
├── vendor/                   # READ-ONLY reference (see Vendor Map below)
└── crates/
    ├── q2proto/              # Wire codec: netchan, msg R/W, svc/clc, usercmd, configstrings
    ├── world/                # .bsp parse → geometry; nav graph; LOS; spatial index
    ├── client/               # Connection state machine + net loop + frame parsing
    ├── brain/                # Navigation, combat, behavior FSM, weapon/aim logic
    ├── qbots/                # Binary: load config, spawn N bot tasks, supervise
    └── tools/                # Reusable binaries — NO tmp/ scripts (packet cap, bsp dumper…)
```

### Concurrency Model
- **One tokio task per bot.** Each owns: a `UdpSocket` (or multiplexed socket + qport),
  its connection state machine, entity/frame history, and a brain tick.
- **Shared read-only:** the parsed nav graph + map metadata, loaded once into an `Arc`.
- **Mutability is local to a bot.** No shared mutable world state across bots — they
  cannot see each other except through server frames (just like real clients).
- **Pacing:** honor the server's `rate` and send `usercmd` at client-frame cadence
  (~fixed timestep). Never flood — the server will drop/kick.

---

## Domain Knowledge

### The Q2 Client–Server Protocol
**Ground truth lives in** `vendor/yquake2/src/`. qbots *is* a client, so read the
**client** code as the reference implementation of "what do I send?", and the **server**
code as "what will it accept/reject?". Confirmed specifics:

- **Transport:** UDP. Connectionless packets are prefixed with `0xff 0xff 0xff 0xff`.
  Ports: master `27900`, client `27901`, server `27910`. `PROTOCOL_VERSION == 34`.
  See `common/header/common.h`.

- **Connection handshake** (connectionless / out-of-band text):
  1. C→S: `getchallenge\n`                         (`client/cl_network.c:185`)
  2. S→C: `challenge %i p=34`                      (`server/sv_conless.c` → `SV_GetChallenge`)
  3. C→S: `connect %i %i %i "%s"\n`
     = `connect <proto=34> <qport> <challenge> <userinfo>`  (`client/cl_network.c:136`)
  4. S→C: `client_connect`                         (`server/sv_conless.c:SVC_DirectConnect`)

- **qport** (`common/netchan.c:72`): a 16-bit client-chosen id that survives NAT port
  remapping. Each bot picks one; the server keys the slot on base-addr + qport.

- **userinfo:** an InfoString (`key\value\key\value`) — `name`, `skin`, `rate`, `msg`,
  `hand`, `fov`, … . Server injects `ip`. At least set `name` + `rate`.

- **Opcodes** (`common/header/common.h:199` / `:231`):
  - Client→Server (`clc_*`): `clc_nop`, `clc_move`, `clc_userinfo`, `clc_stringcmd`.
  - Server→Client (`svc_*`): `svc_serverdata`, `svc_configstring`, `svc_spawnbaseline`,
    `svc_frame` (+`svc_playerinfo`, `svc_packetentities`, `svc_deltapacketentities`),
    `svc_print`, `svc_sound`, `svc_stufftext`, `svc_disconnect`, `svc_reconnect`, …

- **The heartbeat — `clc_move` carries a `usercmd_t`** (`common/header/shared.h:676`):
  ```c
  typedef struct usercmd_s {
      byte  msec;                       // duration this cmd covers
      byte  buttons;                    // BUTTON_ATTACK/JUMP/CROUCH/ANY (common/header/shared.h:660)
      short angles[3];                  // view pitch/yaw/roll, 16-bit
      short forwardmove, sidemove, upmove;  // signed, scaled
      byte  impulse;                    // weapon select, etc
      byte  lightlevel;
  } usercmd_t;
  ```
  Delta-compressed against the previous cmd via the `CM_ANGLE1..3 | CM_FORWARD | CM_SIDE |
  CM_UP | CM_BUTTONS | CM_IMPULSE` bitmask (`common/header/common.h:265`); `msec` +
  `lightlevel` always sent. **Encoding logic:** `common/movemsg.c` + `common/header/shared.h`
  (`MSG_WriteDeltaUsercmd`). **This is the loop the bot runs every frame.**

- **Netchan** (`common/netchan.c`, `common/header/common.h:587`): the reliable +
  unreliable sequence-numbered channel over UDP. `clc_*` go in the message body;
  reliable messages are ack'd and retransmitted. `client/cl_network.c` shows the loop.

### What the Server Sends Us — and What It Doesn't
- **`svc_serverdata`** → protocol, spawncount, gamedir, **our client number**, level string.
- **`svc_configstring`** → indexed string table (models, sounds, statusbar layout, …).
- **`svc_spawnbaseline`** → entity baselines for delta decoding.
- **`svc_frame`** → a snapshot: `svc_playerinfo` (our/players' state) +
  `svc_packetentities` (entity deltas). Delta-decoded against `UPDATE_BACKUP=16` history.
- **`svc_sound` / `svc_print` / `svc_temp_entity`** → events.
- ⚠️ **Entities arrive only within our PVS.** Out-of-sight players/items are *not*
  transmitted. This is the single biggest constraint vs. gamecode bots — design the
  brain around partial, PVS-limited, possibly-stale observations.

### World Reconstruction (the novel hard part)
We must build geometry + navigation **without** the game DLL. Options, in order of fidelity:
- **Parse the `.bsp` directly** (the map file): extract brushes/leafs/visibility/portals
  to get collision, LOS, and a walkable surface. Heaviest but most accurate. Reference:
  the BSP lump layout is described in `vendor/yquake2/doc/` and the loader in
  `common/filesystem.c` + client refresh code.
- **Nav-graph route files** (3ZB2 `.chn`, Eraser `.rt2`): waypoint graphs authored/learned
  per map. Reuse the *format* and *pathing approach*, not the C. See AI section below.
- **Observation learning** (Eraser "dynamic learning", ACE "dynamic pathing"): record a
  human/bot run and mine the trail into nodes. Slowest to bootstrap but needs no BSP.

### Bot-AI Inspiration (algorithms only — they call game-DLL APIs we can't use)
Priority reads from `vendor/Quake2BotArchive/blob/main/research/bots/`:
- **`3zb2.md`** (3rd-Zigock II, "Rago"/ponpoko) — route-linking, weapon-aware route
  selection, CTF AI. Live source: `vendor/3zb2-zigflag/src/bot/{bot,za,func,fire}.c`.
- **`eraser.md`** (Eraser/"Ridah") — dynamic map learning from human trails, danger
  avoidance (rockets/grenades), squadron/team AI, configurable per-bot skill/personality
  (`bots.cfg`), route files (`.rt2`).
- **`ace.md`** (ACE/"Meat") — dynamic pathing + learning, minimal waypoints.

> Treat their C as pseudocode for *behavior*. The `gi.*`/`SV_*`/`g_edicts` calls are
> unavailable to us — replace each with our reconstructed world (`world/` crate).

---

## Vendor Map (READ-ONLY)

| Path | What it is | Use it for |
|------|-----------|------------|
| `vendor/yquake2/src/common/header/common.h` | `PROTOCOL_VERSION`, svc/clc enums, `CM_*`, `PS_*`, netchan struct | The wire-format bible |
| `vendor/yquake2/src/common/header/shared.h` | `usercmd_t`, `BUTTON_*`, vec3/math, delta types | usercmd + math parity |
| `vendor/yquake2/src/common/movemsg.c` | `MSG_WriteDeltaUsercmd`, `bytedirs[]` | usercmd encoding (port to Rust) |
| `vendor/yquake2/src/common/netchan.c` | reliable/unreliable channel, qport rationale | port the channel logic |
| `vendor/yquake2/src/client/cl_network.c` | client net loop, `getchallenge`/`connect` send, frame ack | the reference *client* |
| `vendor/yquake2/src/client/cl_main.c` | connection setup, userinfo | connection lifecycle |
| `vendor/yquake2/src/client/cl_parse.c` | parsing `svc_*` messages | frame/entity decoding |
| `vendor/yquake2/src/server/sv_conless.c` | `getchallenge`/`connect` handling, reject reasons | what the server accepts |
| `vendor/yquake2/src/server/sv_user.c` | how the server consumes `usercmd` | validate our usercmd shape |
| `vendor/yquake2/doc/` | protocol/BSP docs | BSP lump layout for `world/` |
| `vendor/3zb2-zigflag/src/bot/*.c` | live 3ZB2 bot AI source | aim/nav/weapon algorithms |
| `vendor/Quake2BotArchive/research/bots/*.md` | bot histories + feature notes | AI inspiration (extract zips under `bin/` as needed) |

> `vendor/` is READ-ONLY except: you **may** extract archives under
> `vendor/Quake2BotArchive/bin/` (e.g. `3zb2src97.zip`, `Eraser*`) to read older bot
> source. Do not commit extracted trees.

---

## Development Workflow

### 1. Planning — MANDATORY before any non-trivial code
This repo already has a rigorous plan system. **Use it:**
1. Read `context/plans/RULES.md` **in full** before writing code (format, metadata block,
   required sections, Rule A/B).
2. Read `context/plans/SERIES.md` for the cross-plan dependency chain.
3. Create `context/plans/NN_name.md` (+ paired `NN_name_tracker.md`) from the canonical
   template, numbered to continue SERIES. Include `TL;DR`, `Context`, `Tasks`, `Critical
   Files`, `Open Questions`, `Verification Checklist`.
4. Execute task-by-task; update the tracker as you go; `git mv` to `completed/` when done.

### 2. Knowledge Management
- **`context/distilled.md`** — after reading `vendor/` or solving a hard problem,
  compress the finding (packet layout, BSP lump, an aiming formula). Read before new work.
- **`context/pitfalls.md`** — every bug/gotcha, **especially** multi-attempt fixes.
  Template: `# Title → Problem → Fix → Source`. Read before new work.
  *(Also mirrored up at `../context/pitfalls.md` per the slop convention — keep the
  Q2-specific ones local; cross-cutting deps go up.)*
- **`context/high_level.md`** — short pros/cons for library choices (tokio vs async-std,
  glam vs nalgebra, bytes vs bytemuck). Mark which qbots uses.

### 3. Code Quality
- **Tests first** (Red→Green→Refactor). The wire codec is pure functions over bytes —
  unit-test the hell out of `MSG_Read*/Write*` and `usercmd` round-trips with captured
  packets.
- **No type suppression:** no `.unwrap()` without a justified `expect`/handling, no
  `as` truncation that isn't intentional, no `unsafe` without a SAFETY comment.
- **Small modules:** functions < ~50 lines, single responsibility.
- **Docs:** `///` on all public items in `q2proto/`, `world/` — they're the load-bearing
  libraries. Wire-format structs should cite the vendor source line they mirror.

### 4. Build Verification — never commit broken code
- **`cargo build`** exits 0 with **zero warnings**, **`cargo clippy`** clean, **`cargo test`**
  green, **`cargo fmt`** applied — *before every commit*. (qbots RULES.md Rule A is
  authoritative and stricter; defer to it.)
- If the build breaks, **fix it first.** Do not claim "done" on broken code.
### 5. Commits

**CRITICAL: COMMIT AT EVERY TASK COMPLETION. DO NOT WAIT.**

- Small, frequent, one task per commit. Format: `task(TN): <description>`
  (e.g. `task(T3): port MSG_WriteDeltaUsercmd to q2proto`).
- **YOU MUST COMMIT BEFORE MARKING ANY TASK COMPLETE.** If you haven't committed, you haven't finished.
- Intermediate commits within a task are encouraged for logical checkpoints.
- **NEVER batch multiple tasks into one commit** unless they are truly inseparable.
- **Fix all warnings and ensure tests pass BEFORE every commit.**
- Never push — the human pushes after review. No co-author trailers unless asked.
  *(Global rule, `~/.claude/CLAUDE.md`.)*

### 5a. Move Completed Plans to `completed/`

**CRITICAL: WHEN A PLAN IS 100% COMPLETE, MOVE IT TO `completed/` IMMEDIATELY.**

- When a plan and its tracker reach 100% completion:
  ```bash
  git mv context/plans/NN_name.md context/plans/completed/NN_name.md
  git mv context/plans/NN_name_tracker.md context/plans/completed/NN_name_tracker.md
  ```
- Update `SERIES.md` to mark the plan **done**.
- **DO NOT LEAVE COMPLETED PLANS IN THE ACTIVE `context/plans/` DIRECTORY.**
- **Before starting a new plan, verify that the previous plan is either moved to `completed/` or marked as deferred/blocked in SERIES.md.**
- **FAILURE TO MOVE COMPLETED PLANS IS A VIOLATION OF THESE RULES.**

### 6. Tooling
- **No `tmp/` scripts.** Every helper is a binary in `crates/tools/` (e.g.
  `cargo run -p tools -- pcap-decode <file>`). Keep them reusable and documented.

### 7. Delegation
- Stuck on a wire-format detail? **Search `vendor/yquake2/src/` first** — the answer is in C.
- Then check `context/distilled.md` / `pitfalls.md`. Only then ask for help.

---

## Constraints & Rules

1. **Be a client, not a plugin.** No assumption of server-side access. Everything the
   bot knows comes through the socket — design around PVS-limited, lagged perception.
2. **Wire-format parity is non-negotiable.** A byte wrong = server drops us. Mirror
   `vendor/yquake2` exactly; cite the source line.
3. **Respect the server.** Honor `rate`, keep sane packet cadence, implement `disconnect`
   cleanly. qctrl (RCON) can also kick us — don't give it a reason.
4. **No type suppression. No broken commits. No `tmp/` scripts.** (Above.)
5. **Never commit build artifacts.** Generated/build output stays out of git — add it to
   the project `.gitignore` (qbots root) the moment it first appears. Mandatory entries:
   `/target/` (and `/target-*/`, e.g. `target-host/`, if we cross-compile like qctrl),
   `**/*.rs.bk`, `Cargo.lock` stays *for the binary* but ignore nothing else Cargo-owned.
   If a frontend/JS ever appears: `node_modules/`, `dist/`, build caches. **`vendor/` is
   vendored source — also gitignored** (it's cloned, not authored). When in doubt: if it
   can be regenerated by a build command, it does not belong in a commit.
6. **Honesty.** When you say you'll do something, do it, then say "done." Never claim
   something is recorded in `distilled.md`/`pitfalls.md`/`AGENTS.md` unless the bytes
   are actually on disk. Be direct.

---

## Movement Testing (Plan 10)

Two CLI scenarios measure how well a bot **moves** — the lens for Plans 11–14.
Each connects one bot like `connect-one`, **disables combat**, pins its nav goal to a
known target, samples every server frame into a `MovementRecorder`, and dumps a
structured log + a SUMMARY line. They never set velocity or teleport — a
grounded `max_speed` > ~320 in a log flags a physics bug, not a feature.

```bash
# Farthest DM spawn from where the bot spawns.
cargo run -p qbots -- spawn-to-spawn [--map q2dm1] [--addr host:27910] [--name qb0]
# A named weapon's BSP origin (resolved as weapon_<name>).
cargo run -p qbots -- spawn-to-weapon rocketlauncher [--map q2dm1] [--name qb1]
```

- **Map**: `--map` selects the BSP; if omitted it defaults to `q2dm1`. The map
  **must match the server's loaded map** (the nav graph + spawn/weapon origins come
  from the BSP, so a mismatch produces garbage). Discover the server's map with a
  brief `qbots connect-one` (it logs `loading nav graph map=…`).
- **Output**: `./logs/<scenario>/<unix_ts>.<bot>.log` — one frame per line, 16
  positional columns + a `flags` run (`B`=wall-bump, `W`=wrong-turn, `H`=hindered,
  `A`=airborne), ending in a `# SUMMARY reached=… elapsed=… …` line. The schema is
  documented in `crates/brain/src/recorder.rs`. `./logs/` is gitignored.
- **Exit code**: `0` = reached the goal (held within 48 u for 0.5 s); `2` = ran to
  the 30 s cap without reaching it; `FAILURE` = setup/IO error.

The **baseline** (current steering code) lives in
`context/plans/10_movement_test_harness_tracker.md`'s Baseline table — both runs
fail to reach, with low mean speed and many hindered/bump frames. That is the
contract Plans 11–14 must beat; re-run the same scenarios after each and compare.

---

## Getting Started (suggested plan series)

1. **Plan 01 — Workspace scaffold.** `crates/` skeleton, `.gitignore`
   (`/target*/`, `/vendor/`, any build output — see Constraints #5), `justfile`,
   fmt/clippy/test gates.
2. **Plan 02 — Wire codec (`q2proto`).** Port `MSG_*`, `usercmd_t` delta R/W, InfoString.
   Unit-test round-trips with hand-built bytes.
3. **Plan 03 — Connection (`client/`).** `getchallenge` → `connect` → `client_connect`,
   netchan, parse `svc_serverdata`/`configstring`/`spawnbaseline`. Prove: server lists us.
4. **Plan 04 — Frame loop.** Send `clc_move` at a fixed cadence; parse `svc_frame`. Prove:
   a bot stands on the map and looks around.
5. **Plan 05 — World (`world/`).** `.bsp` parse → nav graph + LOS.
6. **Plan 06 — Brain (`brain/`).** Navigate (pathfind) → roam/collect → combat (aim/lead/weap).
7. **Plan 07 — Fleet.** Spawn N bots, supervise, log.

Capture this in `context/plans/SERIES.md` once planning begins.
