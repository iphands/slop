# Plan 04 — Frame Loop & Movement

> **Status**: pending
> **Created**: 2026-06-14
> **Depends on**: Plan 03
> **Goal**: Decode full server frames (playerstate + packet entities) and drive a real
> movement loop so the bot **stands on the map and moves** — not just keeps a connection alive.
> **Agent**: implementation agent (ralph-loop) | sub-agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Add frame-level decoding to `q2proto` (player/entity state deltas) and a frame +
movement driver to `client`. Replace Plan 03's keep-alive zeros with real `Usercmd`s built
from a desired view angle + movement vector.

**Deliverables**:
1. `player_state_t` + `entity_state_t` delta decoders in `q2proto`.
2. `svc_frame` header parse + `UPDATE_BACKUP=16` frame ring in `client`.
3. `svc_playerinfo` (own state) + `svc_packetentities` (world entities) decode → a per-frame snapshot.
4. A movement controller: desired angles/velocity → `Usercmd` → `clc_move`.
5. Movement prediction (`pmove.c` port) so aim/movement is smooth and not rubber-banded.
6. Verification: bot walks a circuit on a flat map without being stuck/dropped.

**Estimated effort**: Medium–Large (2 days)

---

## Context

### Why this milestone

Plan 03 proved the bot can *exist* on the server. Plan 04 proves it can *perceive and act*
on frames — the foundation for every brain feature (Plan 06). "Bot stands on the map and
walks" is the visible proof the perception→action loop is closed.

### Key Facts (confirmed against `vendor/yquake2/src/`)

- **`CL_ParseFrame`** (`client/cl_parse.c:739`): header is `serverframe` (i32), `deltaframe`
  (i32), then `surpressCount` (byte, only in some builds). `servertime = serverframe * 100`
  (ms). `deltaframe <= 0` ⇒ snapshot baseline (no delta).
- **Frame ring**: `cl.frames[serverframe & UPDATE_MASK]`, `UPDATE_BACKUP == 16`
  (`common/header/common.h:195`). Delta decode uses the frame at `deltaframe & UPDATE_MASK`.
- **`CL_ParsePlayerstate`** (`cl_parse.c:547`): own `player_state_t` (`shared.h:1280`),
  delta against previous via the `PS_*` flags (`common.h:243`).
- **`CL_ParsePacketEntities`** (`cl_parse.c:363`): `entity_state_t` (`shared.h:1233`),
  delta-decoded per-entity against the old frame; entity bits flag which fields changed.
- **Move send**: `client/cl_input.c:787` writes `clc_move`; the body is `MSG_WriteDeltaUsercmd`
  (already in `q2proto` Plan 02 T3). `msec` = wall time since last sent cmd.
- **Prediction**: `common/pmove.c` is the authoritative player-movement physics (gravity,
  friction, acceleration, stair-stepping, water). Clients run it locally to predict where
  they'll be, reconciling against the server's authoritative `playerinfo`. We need a port
  of this for smooth aiming/movement and to detect our own stuck/dead states.

---

## Step-by-Step Tasks

> **RULES.md Rule A/B**: zero warnings, clippy clean, fmt applied, tests green — **commit**
> `task(TN): <desc>` at each boundary.

### T1: player_state_t + entity_state_t delta decoders (q2proto)

**Files**: `crates/q2proto/src/{playerstate,entitystate}.rs`, `crates/q2proto/src/lib.rs`

**What to do**: Port `player_state_t` (`shared.h:1280`) and `entity_state_t` (`shared.h:1233`)
as Rust structs. Implement delta decode using the `PS_*` (`common.h:243`) and entity-bit
flags: read the flag mask, then only the flagged fields — exactly as `CL_ParsePlayerstate`
/ `CL_ParsePacketEntities` do. Unit-test: a hand-encoded frame decodes to expected values.

**Commit**: `task(T1): port player_state/entity_state delta decoders`

### T2: svc_frame header + frame ring (client)

**Files**: `crates/client/src/frame.rs`

**What to do**: Port `CL_ParseFrame` (`cl_parse.c:739`). Read `serverframe`, `deltaframe`,
`surpressCount`. Maintain a `[Frame; UPDATE_BACKUP]` ring indexed `serverframe &
UPDATE_MASK`. Resolve the delta source from `deltaframe`; detect dropped/unavailable delta
(`old.serverframe != deltaframe`) and fall back to baseline. Store the snapshot.

**Commit**: `task(T2): parse svc_frame header and maintain frame ring`

### T3: Decode own player state + packet entities

**Files**: `crates/client/src/frame.rs`, `crates/client/src/snapshot.rs`

**What to do**: On each frame, decode `svc_playerinfo` (own origin/velocity/angles/
viewheight/health/weapon) and `svc_packetentities` (other players + items + projectiles),
producing a `Snapshot { self, entities }`. Tag entities by configstring type (player model
vs item vs projectile) — the configstring table from Plan 03 T4 gives the type.

**Commit**: `task(T3): decode playerinfo + packetentities into a snapshot`

### T4: Movement controller → Usercmd

**Files**: `crates/client/src/movement.rs`

**What to do**: A controller that takes a desired yaw/pitch + desired forward/side/up
velocity and emits a `Usercmd` (clamped angles, scaled `forwardmove`/`sidemove`/`upmove`
per Q2 ranges, `buttons` for attack/jump/crouch, `impulse` for weapon switch). Wire it into
the heartbeat loop (Plan 03 T6) so `clc_move` now carries real intent instead of zeros.
Seed with a trivial "look forward, walk forward" driver to prove movement works.

**Commit**: `task(T4): movement controller producing real usercmds`

### T5: Prediction (pmove port)

**Files**: `crates/client/src/predict.rs`

**What to do**: Port `common/pmove.c` (Pmove + the PM_* physics) enough to locally simulate
the next `Usercmd` and predict our origin/velocity. Reconcile against the server's
`playerinfo` each frame; log rubber-band corrections. This smooths aim and lets the brain
know where it *will* be. Prediction needs collision — stub it with a flat-floor assumption
for now; wire the real `world` tracer in Plan 05.

**Commit**: `task(T5): port pmove prediction and reconcile with server frames`

### T6: Verify — bot walks on the map

**What to do**: Against a real server on a flat/open map (e.g. `q2dm1`), watch the bot's
origin advance across frames (via captured snapshots or qctrl `status` ping/position). Run
≥ 2 min — assert no drop, no "stuck at spawn", coordinates actually change. Record frame
decoding surprises in `context/pitfalls.md`.

**Commit**: `task(T6): verify bot stands and moves on the map`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/q2proto/src/{playerstate,entitystate}.rs` | state delta decoders | P0 |
| `crates/client/src/frame.rs` | svc_frame parse + ring | P0 |
| `crates/client/src/snapshot.rs` | decoded snapshot | P0 |
| `crates/client/src/movement.rs` | Usercmd controller | P0 |
| `crates/client/src/predict.rs` | pmove prediction | P1 |

---

## Open Questions / Risks

1. **`surpressCount`/q2pro frame extensions.** Some server builds add fields to the frame
   header. *Mitigation*: T2 logs unknown trailing bytes; if a q2pro server breaks parsing,
   revisit (record in `pitfalls.md`).
2. **Delta decode correctness.** Off-by-one in the entity delta loop corrupts the whole
   snapshot. *Mitigation*: T1/T3 unit-test against a captured frame (gold vector from
   Plan 03 T8); assert entity set matches expected.
3. **Prediction without collision.** T5 stubs collision (flat floor) until Plan 05 supplies
   the real tracer. *Mitigation*: clearly bounded; brain correctness isn't blocked.

---

## Verification Checklist

- [ ] T1: `player_state_t`/`entity_state_t` decoders round-trip against a gold frame.
- [ ] T2: frame ring advances; dropped deltas fall back to baseline without panic.
- [ ] T3: snapshot lists self + visible entities with correct types.
- [ ] T4: sending real `Usercmd`s moves the bot's origin across consecutive frames.
- [ ] T5: predicted origin tracks server origin within a small tolerance.
- [ ] T6: bot walks ≥ 2 min on a real map without drop or stuck-at-spawn.
