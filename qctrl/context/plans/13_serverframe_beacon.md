# Plan 13 — Serverframe Beacon: an exact map clock

> **Status**: in-progress
> **Created**: 2026-07-13
> **Depends on**: Plan 12 (the `sv_maplist` watchdog stays as the beacon-less fallback); qbots Plan 66 (the producer)
> **Goal**: Consume qbots' optional serverframe beacon so the map clock is **measured** instead of inferred — exact on a cold start, and correct across a same-map restart.

> **Before writing any code, re-read `context/plans/RULES.md` in full.**

---

## TL;DR

**What**: Read qbots' optional unix-socket beacon and use `serverframe` to anchor the map clock exactly.

**Deliverables**:
1. `crates/api/src/frames.rs` — beacon parse + server-identity guard + reconnecting reader.
2. `clock.rs` gains `ClockSource::ServerFrame`, an exact anchor, and anti-jitter/staleness rules.
3. Optional `frames:` config section. **Absent ⇒ the feature does not exist at runtime.**

**Estimated effort**: Medium (1 day)

---

## Context

`clock.rs` opens by declaring the map clock unknowable:

> *"A Quake 2 server does not publish elapsed or remaining map time… There is nothing to read."*

That is true **of the two channels qctrl speaks** — RCON and the connectionless OOB `status`
query — and the module is admirably honest about the price it pays:

- Start qctrl mid-map and it never saw the start edge ⇒ `ClockAnchor::Unknown`, **forever**.
- Restart the server onto the **same map** ⇒ no name edge ⇒ the clock keeps counting and is
  **silently wrong**. This is the hole `sv_uptime` was supposed to plug.
- The rotator has to fall back to a 20 s `RESCUE_GRACE_SECONDS` timer whenever the anchor is
  `Unknown` (`rotator.rs:130-139`).

### `sv_uptime` is a ghost

yquake2 **has no `sv_uptime` cvar** — the only `uptime` in the entire tree is a local variable in
the *client's* input code (`cl_input.c:130`). When qctrl sends `set sv_uptime 1`, `Cvar_Set`
cheerfully **creates** the cvar, and nothing ever reads it. That is why the server reports
`"sv_uptime" is "1"` while no OOB reply ever carries an `uptime` key. The cvar management in
`main.rs:601-631` has been talking to itself.

### Key Facts: the clock is on the wire, just not on *our* wire

| Fact | Source |
|------|--------|
| `SV_SpawnServer` does `memset(&sv, 0, sizeof(sv))` ⇒ **`sv.framenum` is zeroed every map spawn** | `vendor/yquake2/src/server/sv_init.c:267` |
| `sv.framenum++; sv.time = sv.framenum * 100;` ⇒ **exactly 10 Hz** | `server/sv_main.c:343-344` |
| `svc_frame` carries `sv.framenum` to **every connected client, every frame** | `server/sv_entities.c:425-426` |

⇒ **`serverframe / 10` is the exact age of the running map.** qctrl cannot see it, because it
never connects a client. **qbots has up to 32 of them**, already decoding this exact field
(`qbots/crates/q2proto/src/frame.rs:107`) and throwing it away.

qbots Plan 66 publishes it on an optional unix socket. This plan consumes it.

### Pre-Identified Bug (caught in planning, before any code)

An earlier draft of this design intended to classify a level change by comparing `servercount`
numerically — *increasing ⇒ map change, decreasing ⇒ process restart*. **That is wrong.**

`SV_InitGame` does **`svs.spawncount = randk();`** (`sv_init.c:495`) — once per server *process*,
**randomly seeded**. A restarted server therefore returns a servercount that may be **higher or
lower** than the old one. The comparison would have been right almost always and catastrophically
wrong at random, which is the worst possible failure mode.

**The rule is simply: `servercount != previous` ⇒ a new level instance exists.** We never classify
which kind, and we never need to — `serverframe` tells us exactly how old the new level is. **Never
compare servercounts with `<` or `>`.**

### Why this integration is small

`ClockState` is already built on a monotonic `Instant` anchor. A serverframe does not need a new
time model — it is just a **better way to compute the anchor we already have**:

```text
map_start = beacon_received_at − (serverframe × 100 ms) − beacon.age_ms
```

Everything downstream — `elapsed_seconds`, `ClockQuality`, `Overdue`, `rotator::decide` — is
untouched. The `age_ms` term is why qbots' 1 Hz publish rate costs **zero** accuracy: the beacon
carries how stale it already was when it left.

---

## Step-by-Step Tasks

### T1: `frames.rs` — parse + identity guard

**File**: `crates/api/src/frames.rs` (new)

Tests first. Pure functions over `&str`, per this repo's "network at the edges" discipline
(`parse_oob_status`, `parse_status_output`, `parse_cvar_echo` are all `&str -> Result`).

```rust
pub const SCHEMA_VERSION: u32 = 1;
pub struct Beacon { v, server, server_name, map, servercount, serverframe, age_ms, bots, seq }

/// PURE. One NDJSON line -> a beacon.
pub fn parse_beacon_line(line: &str) -> Result<Beacon, BeaconError>;

/// PURE. Is this beacon about the server WE manage?
pub fn beacon_matches(b: &Beacon, want_addrs: &[SocketAddr], want_name: &str) -> bool;
```

**The identity guard is not optional.** A qbots fleet pointed at a *different* server would
otherwise silently drive qctrl's countdown with a foreign map's age. Match on the resolved socket
address, with the configured hostname as a fallback (the two sides routinely spell the same host
differently).

### T2: `clock.rs` — the exact anchor

**File**: `crates/api/src/clock.rs`

New `ClockSource::ServerFrame`; new `FrameObservation { map, servercount, serverframe, age }`;
new `observe_frame(f, now)`.

**Re-anchor rule (anti-jitter).** Re-anchoring on *every* beacon would make the countdown wobble by
each beacon's network latency. Re-anchor only when:
- the anchor is not `Exact` — **the cold-start fix**, and the whole point of the plan; **or**
- `source != ServerFrame` — **precedence**: the first beacon after an inferred anchor always takes
  over and corrects it; **or**
- `servercount` changed (a new level instance); **or**
- the map name changed; **or**
- the derived start drifts more than `REANCHOR_TOLERANCE` (2 s) from the anchor we hold.

Otherwise **do nothing at all** to `map_start`. In steady state that is every beacon.

**Precedence, justified.** `ObservedEdge` anchors at *poll detection* time — up to a poll interval
late. `OwnMapCommand` anchors at *rcon send* time — before the server has even loaded the map. Only
`ServerFrame` is **measured**. So it outranks both, and corrects them.

**Staleness.** A beacon older than `FRAME_TRUST_MAX_AGE` (3 s) stops being trusted — but **must not
invalidate the anchor**. The anchor is a fixed monotonic `Instant`; it keeps ticking correctly on
its own. Stopping the fleet must not blank the countdown. We simply stop re-anchoring and let
map-edge detection resume being the authority.

**Suppression + the map race.** While a *fresh* beacon owns the clock **for the map the OOB poll is
reporting**, `observe`'s anchor-mutating branches are skipped. Both halves matter: a fresh beacon
about the **old** map (bots haven't re-handshaked yet, ~1–2 s after a change) must **not** suppress
the poll's edge, or the countdown would keep showing the previous map's elapsed. Whoever sees the
new level first anchors; the other is a no-op; and the beacon always gets the last, precise word.

**Invariant.** `elapsed_seconds.is_some()` **iff** `anchor == Exact`. **Every existing test in
`clock.rs` must pass unmodified** — that is the regression gate for this task.

### T3: `status_cache.rs` — `apply_beacon`

`StatusCache::apply_beacon(&Beacon, Instant)` → `clock.observe_frame`, mirroring the existing
`apply_oob(status, now)` / `apply_rcon_identity(players, now)`.

It **must not** touch `last_ok` / `consecutive_failures`: the beacon proves the *game server* is
alive, not that *qctrl* can reach it. `ClockQuality::Degraded` must stay honest about qctrl's own
polling, because the rotator holds on it (`rotator.rs:114`).

`invalidate_clock()` is retargeted to `clock.invalidate_inferred(now)`, which is a **no-op while a
fresh beacon owns the anchor**: the `sv_maplist` watchdog only *guesses* at a restart within a
minute, while the beacon *measures* it within a second. Honoring the guess would throw away a
correct anchor.

### T4: `FramesConfig`

Follows the `PollConfig` pattern exactly (`config.rs:25-49`). `socket_path: ""` (the default)
means **off**. Note `Config` is `Serialize` and dumped wholesale by `GET /api/config`
(`main.rs:196`) — `FramesConfig` carries no secrets, but its fields will appear there.

### T5: `spawn_frame_listener`

Reconnecting reader with backoff, spawned **only when configured**. Tolerates qbots starting after
qctrl, restarting under it, and never existing at all. Bounded line reads. Logs a server mismatch
**once per connection**, loudly — that is the config footgun this guards.

Transport sits behind a seam so a localhost-TCP variant is a later `impl`, not a redesign.

### T6: frontend

`frontend/src/lib/api.ts` — add the new `MapClock` fields as optional and `'server_frame'` to the
`source` union. No component change needed: the countdown simply becomes correct on a cold start,
which is the entire user-visible payoff.

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/api/src/clock.rs` | `ServerFrame` source, `observe_frame`, staleness + suppression gates | P0 |
| `crates/api/src/frames.rs` | New: parse, identity guard, reconnecting reader | P0 |
| `crates/api/src/status_cache.rs` | `apply_beacon`; `invalidate_clock` → `invalidate_inferred` | P0 |
| `crates/api/src/config.rs` | `FramesConfig`, off by default | P0 |
| `crates/api/src/main.rs` | `mod frames`; guarded `spawn_frame_listener` | P0 |
| `frontend/src/lib/api.ts` | Optional `MapClock` fields | P1 |

---

## Open Questions / Risks

1. **`sv.framenum` is not the game DLL's `level.time`.** `level.time` is what `CheckDMRules`
   compares against `timelimit`. They advance in lockstep (one game frame per server frame); the
   residual offset is only the frames the server burns during precache/spawn, putting `sv.framenum`
   slightly **ahead** — a sub-second effect. Bounded and harmless: the rotator's
   `EARLY_FIRE_SECONDS = 5` swallows it, and erring *early* is the safe direction (we preempt
   intermission rather than miss it). **Do not "fix" this with a fudge constant.**
2. **Co-location.** A unix socket needs qbots on the same host. *Mitigation*: the reader is written
   against a line stream, not a socket, so a TCP variant is a new `impl`, not a clock change.
3. **Cross-repo wire contract, no shared crate** (a shared crate would make the coupling
   mandatory). *Mitigation*: a golden-line test in each repo asserting the same literal string —
   qctrl's `the_wire_format_matches_what_qbots_emits` ↔ qbots' `encode_pins_the_wire_format`. They
   must be edited together. Unknown schema versions are rejected loudly rather than mis-parsed.
4. **`systemd` `PrivateTmp=`** would make `/tmp` *not* shared between units — the socket then has to
   live under `/run/qbots/`. Note it in `DEPLOYMENT.md`.
5. **This adds a source, not a dependency.** The `sv_maplist` watchdog and `manage_sv_uptime` stay
   as the beacon-less fallback (retiring the `sv_uptime` ghost is a separate, later change). Every
   path must degrade to today's behaviour when the beacon is absent, stale, or rejected.

---

## Verification Checklist

- [ ] T1: parse tests green (valid; unknown version rejected; garbage rejected; **unknown extra
      fields accepted**, for forward-compat with a newer qbots); `beacon_matches` **rejects a
      foreign server**
- [ ] T2: **all 13 pre-existing `clock.rs` tests pass unmodified**
- [ ] T2: cold start mid-map + one beacon ⇒ `Exact` / `ServerFrame` with the right elapsed
- [ ] T2: 60 beacons with jittered arrival ⇒ elapsed advances exactly, no wobble
- [ ] T2: a stale beacon keeps the anchor ticking and does **not** blank the countdown
- [ ] T3/T4: `apply_beacon` does not fake `server_online`; a config with no `frames:` block loads
      with the feature off
- [ ] T5: `just be-test` and `just be-all` green (fmt + clippy `-D warnings` + release build)
- [ ] T6: `just fe-build` green (node 22 — see the justfile note)
- [ ] **Feature-off regression**: with no `frames:` block, `/api/status` is byte-for-byte what it is
      today and no new task is spawned
- [ ] Live: cold-start qctrl mid-map with the beacon on ⇒ `anchor: exact`, `source: server_frame`
      within a second (today: `unknown`, forever)
- [ ] Live: `rcon map q2dm7` while already on `q2dm7` ⇒ re-anchors to ~0 (today: silently wrong)
- [ ] Commit at every task boundary (`task(TN): …`), tests + clippy + fmt green before each
