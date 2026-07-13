# Plan 66 — Serverframe Beacon (optional qctrl coupling)

> **Status**: in-progress
> **Created**: 2026-07-13
> **Depends on**: Plan 57 (ack-on-frame — supplies the hook point), Plan 64 (servercount level detection)
> **Goal**: Publish the fleet's observed `serverframe` on an optional unix socket so qctrl can know the exact age of the running map without connecting a Q2 client of its own.
> **Agent**: implementation agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: qbots optionally publishes one NDJSON line per server tick on a unix socket carrying `serverframe`, `servercount` and `map`. qctrl consumes it (its Plan 13) to get an exact map clock.

**Deliverables**:
1. `crates/qbots/src/beacon.rs` — pure fold/encode/rate-limit core + a `watch`-based fleet handle + a unix-socket server.
2. Optional `beacon:` config section, **disabled by default**.
3. One hook in `bot_task`'s existing new-frame edge; zero behavioural change when disabled.

**Estimated effort**: Medium (1 day)

---

## Context

qctrl runs the map rotation and shows a map countdown. It speaks RCON and the connectionless OOB `status` query — and **neither carries a map clock**. Its `crates/api/src/clock.rs` says so in its module doc, and it is right: it infers map elapsed time by polling the map *name* and watching for a change. That inference has two holes it cannot close from where it stands:

- Start qctrl while a map is already running and it never saw the start edge ⇒ elapsed is **permanently unknowable**.
- Restart the server onto the **same map** and there is no name edge ⇒ the clock keeps counting and is **silently wrong**.

### Key Facts (verified in `vendor/yquake2`)

The map clock *is* on the wire. It is simply not visible to a non-client:

| Fact | Source |
|------|--------|
| `SV_SpawnServer` does `memset(&sv, 0, sizeof(sv))` ⇒ **`sv.framenum` is zeroed on every map spawn** | `src/server/sv_init.c:267` |
| `sv.framenum++; sv.time = sv.framenum * 100;` ⇒ **exactly 10 Hz** | `src/server/sv_main.c:343-344` |
| `MSG_WriteByte(msg, svc_frame); MSG_WriteLong(msg, sv.framenum);` ⇒ **sent to every connected client, every frame** | `src/server/sv_entities.c:425-426` |

Therefore **`serverframe / 10` == seconds since `SV_SpawnServer`**. We already decode it (`crates/q2proto/src/frame.rs:107` → `Conn::frame`, `crates/client/src/conn.rs:59`) and today we throw it away for anything but delta resolution and ack timing.

### Pre-Identified Bug (in the design, caught before coding)

An earlier draft of the qctrl-side design intended to classify a level change as *map change vs process restart* by comparing `servercount` numerically (increasing ⇒ map change, decreasing ⇒ restart). **That is wrong and would have shipped a rare, near-unreproducible bug.**

`SV_InitGame` does **`svs.spawncount = randk();`** (`src/server/sv_init.c:495`) — once per server *process*, **randomly seeded** — and only then `svs.spawncount++` per `SV_SpawnServer` (`sv_init.c:264`) and per `SV_Nextserver` (`sv_user.c:522`). A restarted server therefore comes back with a **random** servercount, which may be higher *or* lower.

**Rule, and it must not be "optimised" later: `servercount != previous` ⇒ a new level instance exists. Full stop. Never compare servercounts with `<` or `>`.** We never need to know *which* kind of change it was, because `serverframe` tells us exactly how old the new level is. This is why the beacon *measures* level age instead of *inferring* restarts.

### Why a beacon rather than qctrl connecting its own client

A Q2 client costs a player slot, shows in the scoreboard, and needs the whole handshake/nav/brain stack. We already have up to 32 clients connected and decoding this number. Relaying it costs one non-awaiting call on the frame path.

### Why this is telemetry, not shared world state

AGENTS.md §Concurrency forbids shared mutable *world* state across bots. `stats.rs` (`FleetStats`) already carves out the exception this follows: *"shared mutable **telemetry**, not shared mutable **world** state — a counter never lets one bot perceive another."* The beacon is strictly weaker than a kill tally: bots only ever **write** to it, never read, so no bot can perceive another through it.

### The hard requirement: 32 bots, one message per tick

All 32 bots see frame N. The socket must carry **one** message for frame N, not 32. This must be **structural**, not a property of a ticker someone could later refactor away.

**Mechanism.** The beacon value is a pure function of `(servercount, serverframe)`. Every bot holds a clone of `Arc<watch::Sender<BeaconState>>` and calls `send_if_modified(|cur| fold(cur, obs, ...))`. `fold` returns `true` **iff the beacon actually advanced** — so the first bot to report frame N returns `true` and the other 31 return `false` and wake nobody. A **single** fanout task owns the only `watch::Receiver` and is the only writer to the socket; `watch` additionally coalesces any sends between two `changed()` wakeups.

⇒ `messages/sec ≤ distinct serverframes/sec`, **independent of bot count**. Bot count cannot amplify it. `fold` is a free function, so the guarantee is unit-testable without tokio at all.

Alternatives rejected: `Arc<Mutex<_>>` + an independent ticker (the "one message" property becomes the ticker's, not the data's); `broadcast` from the bots (fans out per message — the exact amplification we are avoiding).

---

## Step-by-Step Tasks

### T1: `beacon.rs` — pure core

**File**: `crates/qbots/src/beacon.rs` (new)

**What to do**: Tests FIRST. No tokio, no sockets in this task.

```rust
pub const SCHEMA: u32 = 1;

/// One bot's report of a freshly decoded server frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BotFrame { pub servercount: i32, pub serverframe: i32 }

/// The single fleet-wide beacon value. Only the NEWEST frame survives.
#[derive(Clone, Debug, Default)]
pub struct BeaconState {
    pub servercount: i32,
    pub serverframe: i32,
    pub map: String,               // passenger: never triggers a message on its own
    pub observed_at: Option<Instant>,
    pub seq: u64,                  // +1 per accepted frame == the message counter
    retired: Option<(i32, Instant)>,  // just-left servercount + when; anti-flap
}

/// Fold one bot's frame into the fleet beacon.
///
/// Returns `true` iff the beacon changed — i.e. iff this bot is the FIRST to report
/// this frame. Passed straight to `watch::Sender::send_if_modified`, whose return
/// value gates the single downstream wakeup. THIS is the coalescing point.
pub fn fold(cur: &mut BeaconState, obs: BotFrame, map: &str, now: Instant) -> bool;
```

`fold` rules, in order:
1. `obs.serverframe < 0` → `false`.
2. beacon empty (`observed_at.is_none()`) → accept.
3. `obs.servercount == cur.servercount` → accept **iff `obs.serverframe > cur.serverframe`**. *(the only branch that runs in steady state)*
4. `obs.servercount != cur.servercount` → reject if it is the just-retired servercount within `LEVEL_FLAP_GRACE` (2 s); else accept, recording `retired = Some((cur.servercount, now))`. **No numeric comparison** — see Pre-Identified Bug.
5. On accept: update `servercount`/`serverframe`/`observed_at`, `cur.map = map.to_string()` (only on accept, so ≤10 allocs/sec fleet-wide), `seq += 1`.

Then the wire encoder and the rate limiter, both pure:

```rust
/// PURE. `None` when there is nothing worth publishing — no frame yet, or the map name
/// hasn't arrived. qctrl must NEVER see a frame attributed to the wrong map.
pub fn encode(st: &BeaconState, server: &str, server_name: &str, bots: u32, now: Instant) -> Option<String>;

/// PURE. Write now? Immediately on a level change; otherwise at most once per `interval`;
/// plus a heartbeat on the interval even when the frame counter is frozen, so a wedged
/// server shows up as a growing `age_ms` rather than as silence.
pub fn should_write(st: &BeaconState, last: Option<&Written>, now: Instant, interval: Duration) -> bool;
```

`age_ms = now - observed_at` is what makes a 1 Hz publish interval cost **zero** accuracy: qctrl reconstructs the anchor as `received_at - serverframe*100ms - age_ms`.

### T2: the fleet handle

**File**: `crates/qbots/src/beacon.rs`

`Beacon { tx: Arc<watch::Sender<BeaconState>>, bots: Arc<AtomicU32> }` — clone-cheap, handed to every bot task exactly like `FleetStats`. `on_frame()` is non-async and allocation-free on the reject path. `bot_active() -> ActiveBot` is an RAII guard (+1 now, −1 on drop, covering every `bot_task` exit path including unwind). Bot count is **deliberately not part of the watch value**, so a bot joining can never emit a message.

`Cargo.toml`: add `serde_json = "1"`; dev-dep `tempfile = "3"`.

### T3: config

**File**: `crates/qbots/src/config.rs`

`BeaconCfg { enabled: false, socket_path: /tmp/qbots-beacon.sock, publish_interval_ms: 1000, socket_mode: 0o666, max_clients: 4 }`, same `#[serde(default)]` + hand-written `Default` shape as `Fleet`. Add `#[serde(default)] pub beacon: BeaconCfg` to `Config`. **Off by default** — an existing `config.yaml` keeps behaving exactly as it does now.

### T4: `beacon::serve`

**File**: `crates/qbots/src/beacon.rs`

- **Bind / stale socket:** if the path exists, `UnixStream::connect` it first. Connect **succeeds** ⇒ another qbots owns it ⇒ log an error and disable the beacon (do **not** steal the path). Connect **fails** ⇒ stale file ⇒ `remove_file`, then bind. Then apply `socket_mode`.
- **Fanout task:** the one `watch::Receiver` + a `broadcast::Sender<Arc<str>>`. `select!` on `rx.changed()` vs the heartbeat deadline → `should_write` → `encode` → broadcast.
- **Accept loop:** capped at `max_clients`; each peer gets a writer task. A lagging or erroring client is dropped (qctrl reconnects).
- **Shutdown:** the existing `Shutdown` AtomicBool + `spawn_signal_listener` (`supervisor.rs:172,203`); unlink the socket on exit.
- **A beacon failure must never take the fleet down** — log and disable.

### T5: wire it in

**Files**: `crates/qbots/src/supervisor.rs`, `crates/qbots/src/main.rs`

- `FleetShared` gains `beacon: Option<Beacon>`; `run_fleet` builds it when `cfg.beacon.enabled` and spawns `beacon::serve`.
- `bot_task` takes `Option<&Beacon>`, arms an `ActiveBot` guard on reaching `Active`.
- **The hook**, inside the existing `if Some(sf) != prev_sf` block at `main.rs:1053` (Plan 57's ack-on-frame edge — already exactly once per distinct frame per bot):
  ```rust
  if let (Some(b), Some(sc)) = (beacon, conn.serverdata.as_ref().map(|sd| sd.servercount)) {
      b.on_frame(sc, sf, &beacon_map, now);
  }
  ```
- `beacon_map` is set in the CS-33 block (`main.rs:1209`) and **cleared in the servercount-change reset block (`main.rs:1185`)**. That clear is load-bearing: without it, a bot that has seen the new level's frames but not yet re-parsed CS 33 would attribute them to the **old map name**, and qctrl would reject or misfile the beacon.

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/qbots/src/beacon.rs` | New: fold/encode/should_write, `Beacon` handle, `serve` | P0 |
| `crates/qbots/src/main.rs` | `mod beacon`; `bot_task` arg; hook at `:1053`; `beacon_map` at `:1185`/`:1209` | P0 |
| `crates/qbots/src/supervisor.rs` | `FleetShared.beacon`; spawn `serve` in `run_fleet` | P0 |
| `crates/qbots/src/config.rs` | `BeaconCfg`, off by default | P0 |
| `crates/qbots/Cargo.toml` | `serde_json`; dev-dep `tempfile` | P1 |
| `config.example.yaml` | Document the `beacon:` block | P1 |

---

## Open Questions / Risks

1. **Cross-repo wire contract with no shared crate.** qbots and qctrl are separate workspaces; a shared crate would be a hard build dependency between them and would defeat "optional coupling". *Mitigation*: the struct is duplicated and pinned by a **golden-line test in each repo asserting the same literal string** (qbots `encode_pins_the_wire_format` ↔ qctrl `the_wire_format_matches_what_qbots_emits`). Each test names the other. Bump `SCHEMA` on any breaking change; qctrl rejects unknown versions loudly rather than mis-parsing.
2. **Hot-path cost.** One `send_if_modified` per decoded frame per bot = 320/sec at 32 bots: an uncontended `RwLock` write plus two integer compares. Negligible — but it *is* on the ack path Plan 57 tuned for ping, so `on_frame` stays non-async and allocates only on accept (≤10/sec fleet-wide).
3. **Stale socket after SIGKILL.** Handled by the connect-probe on bind (T4). A *live* peer on the path means another qbots owns it, and we disable rather than steal.
4. **Co-location.** A unix socket requires qctrl on the same host. True today. qctrl's side puts the reader behind a transport seam so a TCP variant is a later add, not a redesign.
5. **`systemd` `PrivateTmp=`.** If the two ever run as different units with `PrivateTmp=`, `/tmp` is **not** shared — the socket must move to `/run/qbots/`. Note it in qctrl's `DEPLOYMENT.md`.

---

## Verification Checklist

- [ ] T1: `fold` tests green, including **`thirty_two_bots_reporting_one_frame_produce_one_message`** and `a_lower_servercount_is_still_a_level_change` (pins the `randk()` finding so nobody "fixes" it into a `>`)
- [ ] T2: `#[tokio::test]` — 32 concurrent tasks × 10 frames wake the publisher **≤ 10 times**, not 320
- [ ] T3: a config with no `beacon:` block loads and reports `enabled == false`
- [ ] T4: one line arrives over a real unix socket; the file is unlinked on shutdown; a stale file is reclaimed
- [ ] T5: `just all` green (fmt-check + `clippy -D warnings` + test + build), **zero warnings**
- [ ] T5: with `beacon.enabled: false` (the default), no socket is created and fleet behaviour is bit-identical
- [ ] T6: live — 32-bot fleet + `socat -u UNIX-CONNECT:/tmp/qbots-beacon.sock -` shows **~1 line/sec with `seq` incrementing by 1**, and the rate does **not** scale with bot count
- [ ] Commit at every task boundary (`task(TN): …`), clippy + fmt + tests green before each
