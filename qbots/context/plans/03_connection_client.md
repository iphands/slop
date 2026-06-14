# Plan 03 — Connection (`client`)

> **Status**: pending
> **Created**: 2026-06-14
> **Depends on**: Plan 02
> **Goal**: A bot that completes the Q2 connect handshake over UDP, parses serverdata, and
> finishes spawning — appearing in the server's player list (`status`) and staying connected.
> No movement logic yet beyond a keep-alive `clc_move`.
> **Agent**: implementation agent (ralph-loop) | sub-agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Build `crates/client` on top of `q2proto` (Plan 02) — a tokio task per bot that
runs the connection state machine, netchan, and spawn handshake until the server marks the
client active. Wire a tiny `qbots connect-one` CLI to prove it end-to-end against a real server.

**Deliverables**:
1. `Netchan` sequence bookkeeping + reliable/unreliable framing over `tokio::net::UdpSocket`.
2. Connection FSM (`Disconnected → Connecting → Connected → Active`) driving the
   `getchallenge → connect → client_connect → "new" → serverdata → "begin"` exchange.
3. `userinfo` builder + `svc_serverdata` / `svc_configstring` / `svc_spawnbaseline` parsing.
4. Keep-alive `clc_move` heartbeat so the server doesn't time us out.
5. `qbots` binary `connect-one` mode + integration test: bot shows in `status` (via qctrl RCON).

**Estimated effort**: Medium–Large (2 days)

---

## Context

### Why this milestone

"Bot appears in the server's player list" is the first externally-verifiable proof that the
external-client approach works at all. Everything in Plans 04–07 (frames, world, brain)
presumes a live, stable connection — so this must be rock-solid first.

### The spawn sequence (confirmed against `vendor/yquake2/src/`)

1. C→S (OOB): `getchallenge\n` — `client/cl_network.c:185`
2. S→C (OOB): `challenge %i p=34` — `server/sv_conless.c` (`SV_GetChallenge`)
3. C→S (OOB): `connect <34> <qport> <challenge> "<userinfo>"` — `client/cl_network.c:136`
4. S→C (OOB): `client_connect` — `server/sv_conless.c` (`SVC_DirectConnect`)
5. netchan established; C→S reliable: `clc_stringcmd "new"` — `client/cl_network.c:483`
6. S→C: `svc_serverdata` (parse: protocol, servercount, attractloop byte, gamedir,
   **playernum** i16, levelname) — `client/cl_parse.c:887` (`CL_ParseServerData`)
7. S→C: stream of `svc_configstring` + `svc_spawnbaseline`, then `svc_stufftext "precache\n"`
8. C→S reliable: `clc_stringcmd "begin <servercount>"` — `client/cl_download.c:531`
9. state → `ca_active`; client begins sending `clc_move` each frame — `client/cl_parse.c:847`

### Key Facts

- **qport** (`common/netchan.c:72`): 16-bit, client-chosen, survives NAT remapping. Each
  bot picks a distinct qport; server keys the slot on base-addr + qport. If we run many
  bots from one IP, **distinct qports are mandatory**.
- **Skip precache.** qbots renders nothing, so it has no models/sounds to fetch. After
  parsing `svc_serverdata` we can send `begin <servercount>` almost immediately — the
  download loop in `cl_download.c` does exactly this when `allow_download` is off. We must
  still **consume** all configstrings/baselines the server sends before/around `begin`.
- **Heartbeat / timeout.** The server drops idle clients (~`cl_main.c:860` checks
  `packetdelta > 100000` µs in `ca_connected`). Send a `clc_move` on a fixed cadence
  (target ~client frametime, e.g. 1/72 s, but cap packet rate by the server's `rate`).
- **`svc_stufftext`** = a console command the server pushes into us (e.g. `precache\n`,
  `reconnect\n`, `disconnect`). We must parse and at least `ack`/no-op the harmless ones.
- **`svc_disconnect` / `svc_reconnect`**: handle both — log + tear down, or restart FSM.

---

## Step-by-Step Tasks

> **RULES.md Rule A/B**: zero warnings, clippy clean, fmt applied, tests green — **commit**
> `task(TN): <desc>` at each boundary.

### T1: Netchan over UDP

**Files**: `crates/client/Cargo.toml` (add `q2proto` workspace dep + `tokio`, `tracing`),
`crates/client/src/netchan.rs`

**What to do**: Port `netchan_t` bookkeeping (`common/header/common.h:587`,
`common/netchan.c`): `incoming_sequence`, `outgoing_sequence`, `reliable_sequence`
(1-bit), `last_reliable_sequence`. Wrap a `tokio::net::UdpSocket`. Implement
`send_reliable` (set REL flag, queue into the reliable buffer, will re-send until acked),
`send_unreliable`, `recv` (decode header: `flags | sequence | reliable_seq | qport`), and
ack handling. Use `q2proto::oob` for the `0xff×4` connectionless path vs. the netchan
in-band path. **Add `client` → `q2proto` dep** in both `Cargo.toml`s.

**Commit**: `task(T1): implement netchan sequence bookkeeping over tokio UdpSocket`

### T2: Connection FSM + handshake

**Files**: `crates/client/src/conn.rs`, `crates/client/src/state.rs`

**What to do**: Enum `ConnState { Disconnected, Connecting, Connected, Active }`. A
`Connection` driving the 9-step sequence above. `start()` sends `getchallenge`; on
`challenge` reply, builds + sends `connect`. On `client_connect`, switch to in-band
netchan, send `clc_userinfo` + `clc_stringcmd "new"`. Drive timeouts with `tokio::time`
(re-send `getchallenge`/`connect` if no reply within ~1 s, a few retries).

**Commit**: `task(T2): drive connect handshake FSM`

### T3: userinfo builder

**Files**: `crates/client/src/userinfo.rs`

**What to do**: Build the `userinfo` InfoString with required/expected keys: `name`
(distinct per bot), `skin`, `rate` (e.g. `25000`), `msg` (`0`), `hand`, `fov`. Server
injects `ip` — do not set it. Use `q2proto::infostring`.

**Commit**: `task(T3): build userinfo InfoString`

### T4: Parse `svc_serverdata` + configstrings + baselines

**Files**: `crates/client/src/parse.rs`

**What to do**: On the first in-band message, dispatch by `SvcOp`. Implement
`parse_serverdata` (fields per `cl_parse.c:887`): assert `protocol == 34` (else log +
disconnect), store `servercount`, `gamedir`, `playernum` (our entity), `levelname`. Store
`svc_configstring` into a `Vec<String>` table (size `MAX_CONFIGSTRINGS`), and
`svc_spawnbaseline` into a baseline map (full decode deferred to Plan 04; here store raw
for completeness). Handle `svc_stufftext` (parse command name; on `precache\n`, proceed to
send `begin`; on unknown, log + no-op). Handle `svc_print` (log the line).

**Commit**: `task(T4): parse serverdata, configstrings, baselines, stufftext`

### T5: Complete spawn (`begin`) + reach Active

**Files**: `crates/client/src/spawn.rs`

**What to do**: After serverdata + first configstring/baseline flush, send reliable
`clc_stringcmd "begin <servercount>"`. On the next `svc_frame` (or the server's
spawn-confirmation), set `ConnState::Active`. **Verify** the server now treats us as a
spawned player. Keep parsing minimal — full frame decode is Plan 04.

**Commit**: `task(T5): send begin, transition to Active`

### T6: Keep-alive heartbeat

**Files**: `crates/client/src/loop.rs`

**What to do**: While `Active`, on a fixed cadence build a `Usercmd` (zeros / a neutral
stance) and send `clc_move` via `q2proto::usercmd` delta. Ack incoming frames so the
server's `incoming_acknowledged` advances. Cap send rate to the server's advertised
`rate`. This alone should hold the connection open indefinitely.

**Commit**: `task(T6): add clc_move keep-alive heartbeat`

### T7: `qbots connect-one` CLI

**Files**: `crates/qbots/Cargo.toml`, `crates/qbots/src/main.rs`

**What to do**: A `clap` CLI: `qbots connect-one --addr <ip:port> --name <bot>`. Reads
server addr, spawns one bot task, runs the connection, logs state transitions +
`print`s from the server. On Ctrl-C / signal, send `clc_stringcmd "disconnect"` (or just
drop) and exit cleanly. This is the integration harness for T8.

**Commit**: `task(T7): add qbots connect-one CLI harness`

### T8: Integration test against a real server

**What to do**: Start a local yquake2/q2pro deathmatch server (or point at the qctrl-managed
one). Run `qbots connect-one …`. Via qctrl RCON (`status`), confirm the bot is listed with
a client slot and ping. Let it sit ≥ 60 s — assert no timeout/drop. Capture a clean packet
exchange (e.g. via `tools` pcap or `tcpdump`) and **save it** as the Plan 02 gold vector.
Record any wire surprises in `context/pitfalls.md`.

**Commit**: `task(T8): verify bot appears in server status and holds connection`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/client/src/netchan.rs` | netchan over tokio UDP | P0 |
| `crates/client/src/conn.rs`, `state.rs` | connect FSM | P0 |
| `crates/client/src/userinfo.rs` | userinfo builder | P0 |
| `crates/client/src/parse.rs` | serverdata/configstring/baseline/stufftext | P0 |
| `crates/client/src/spawn.rs` | `begin` + Active | P0 |
| `crates/client/src/loop.rs` | `clc_move` heartbeat | P0 |
| `crates/qbots/src/main.rs` | `connect-one` harness | P1 |
| `crates/client/Cargo.toml` | add `q2proto` dep | P0 |

---

## Open Questions / Risks

1. **Does the server require real precache before `begin`?** Likely not for an external
   bot (no assets), but some server builds may gate `begin` on configstring acks.
   *Mitigation*: T8 will reveal it; if gated, add a "consume-all-configstrings-then-begin"
   gate rather than fetching files.
2. **NAT + multiple bots on one IP.** Distinct qports are required; the server still keys
   on base-addr+qport. *Mitigation*: T1 assigns qport per `Connection`; verify with 2 bots.
3. **`rate` / flood.** Sending `clc_move` too fast gets us throttled/kicked. *Mitigation*:
   T6 caps cadence to the server's `rate`; start conservative (≤ ~30 cmd/s).
4. **Encryption/q2flood protection.** Some q2pro builds add anti-flood; a well-formed,
   paced client should be unaffected. *Mitigation*: log any `svc_print`/`svc_disconnect`
   with a reason string during T8.
5. **No frame decode yet.** Plan 03 ignores `svc_packetentities` detail beyond what
   handshake needs. *Mitigation*: clearly bounded — full frame/world parsing is Plan 04/05.

---

## Verification Checklist

- [ ] T1: netchan round-trips a reliable + unreliable message; seq/ack counters match `common/netchan.c`.
- [ ] T2: FSM reaches `Connected` from a real server's `client_connect`.
- [ ] T3: `userinfo` parses identically to q2's `Cvar_Userinfo()` output for the same keys.
- [ ] T4: `svc_serverdata` yields `protocol==34`, correct `playernum`, `servercount`, `levelname`.
- [ ] T5: after `begin`, `status` shows the bot as spawned (not "connecting").
- [ ] T6: bot holds connection ≥ 60 s with no timeout; `rate` respected.
- [ ] T7: `qbots connect-one --addr … --name …` runs and logs state transitions.
- [ ] T8: qctrl RCON `status` lists the bot; a clean packet capture is saved for Plan 02 gold tests.
