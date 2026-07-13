# Plan 64 — Map-Change Survival + Intermission Handling

> **Status**: done
> **Created**: 2026-07-12
> **Depends on**: Plan 57 (ack-on-frame send loop), Plan 53 (connect deadline)
> **Goal**: Bots survive server map changes (rcon `map X` and fraglimit/timelimit rotation) — they re-handshake on the live netchan, reload per-map nav state, and during intermission press ATTACK so an all-bot server actually advances to the next level.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Teach `Conn` the Yamagi map-change wire flow (`stufftext "changing"` → `stufftext "reconnect"` → `clc_stringcmd "new"`), reset per-map state in `bot_task` when the servercount changes, and handle the `PM_FREEZE` intermission by voting the level onward with BUTTON_ATTACK.

**Deliverables**:
1. `Conn` survives `map q2dm2` issued over rcon: drops to `Connected`, sends `"new"` on the same netchan, re-runs configstrings/precache/begin. Unit-tested.
2. `bot_task` detects the new servercount, tears down nav/heatmap/frame trackers, reloads the nav graph from the new `CS_MODELS+1` configstring, and resets the connect deadline so the re-handshake isn't killed as a timeout.
3. Intermission (fraglimit reached): bots idle frozen, then press ATTACK after ~6 s so the baseq2 game code (`p_client.c ClientThink`, exit requires a button press ≥5 s in) exits to the next map.
4. Live-verified: fleet keeps playing across `curl` rcon map change AND across a fraglimit-5 rotation.

**Estimated effort**: Small–Medium (half day)

## Context

### Key Facts (vendor ground truth)

- **Map change flow** (`sv_init.c:614,654`): `SV_Map` broadcasts `stufftext "changing\n"`,
  spawns the new server, then broadcasts `stufftext "reconnect\n"`. The client slot and
  netchan **persist** — no new challenge/connect handshake.
- **Client side** (`cl_network.c:436 CL_Changing_f`, `:468 CL_Reconnect_f`): on `changing`
  drop to `ca_connected` (stop expecting frames, keep netchan); on `reconnect` (while
  connected) send reliable `clc_stringcmd "new"` on the SAME netchan. Server re-runs
  `SV_New_f`: fresh `svc_serverdata` (new servercount + level), configstrings, baselines,
  `precache` → client sends `begin <servercount>`.
- **Intermission** (`game/player/client.c:2117`): every client gets
  `pm_type = PM_FREEZE` (=4, `shared.h:640`). The level exits only when a client sends
  `buttons & BUTTON_ANY` (=128) after `intermissiontime + 5.0` — an all-bot server
  **hangs forever on the scoreboard** unless a bot presses a button.
- Fraglimit rotation goes through the exact same `changing`/`reconnect` stufftext flow
  (`ExitLevel` → `gamemap` → `SV_Map`).

### Pre-Identified Bugs (current code)

1. `Conn::on_payload` ignores `stufftext "changing"`/`"reconnect"` → bot keeps sending
   `clc_move`, never sends `"new"`, server eventually times the zombie client out.
2. `begin_queued` is never reset → even if `"new"` were sent, the follow-up `precache`
   would not queue `begin` and the bot would never spawn.
3. Stale `ConfigStrings`/`FrameRing` across the reload → old map path at `CS_MODELS+1`
   (index 33) would rebuild the OLD nav graph; stale ring entries poison delta decode.
4. `bot_task`: `map_loaded` never resets → nav graph/collision/heatmap stay bound to the
   old map even if the connection survived.
5. `bot_task` connect deadline (Plan 53) fires the moment state leaves `Active` after the
   deadline has passed → a mid-game map change would return `TimedOut`, which the
   supervisor treats as a **non-retryable join failure** (strict mode: kills the fleet).
6. Nothing handles `PM_FREEZE` → brains keep "playing" against a frozen playerstate, and
   nobody presses the button that ends intermission.

### Why re-handshake on the live netchan (not full reconnect)

That is what real clients do (`CL_Reconnect_f` when `ca_connected`), the server keeps our
slot (name/frags table position), and it avoids re-rolling challenge/qport. The existing
`svc_reconnect` opcode path (full restart) stays as-is for the server-initiated hard case,
but gets the same state-reset hardening.

## Step-by-Step Tasks

### T1: Conn handles the map-change stufftext flow

**File**: `crates/client/src/conn.rs`

**What to do**: In the `SvcEvent::StuffText` arm add two cases:
- `"changing"`: `begin_queued = false`, `frame = None`, `ring = FrameRing::new()`,
  `state = Connected` (netchan kept).
- `"reconnect"`: same resets PLUS `serverdata = None`,
  `configstrings = ConfigStrings::default()`, queue reliable `Stringcmd "new"`.

Harden the `SvcEvent::Reconnect` (opcode) arm with the same field resets before it
restarts the full handshake. Add unit tests: active conn → `changing` → `reconnect` →
keepalive carries `"new"` → new serverdata (servercount 4343) + `precache` → `begin 4343`
re-queued (proves the `begin_queued` reset).

**Commit**: `task(T1): survive map-change stufftext (changing/reconnect) in Conn`

### T2: bot_task per-map reset + connect-deadline reset

**File**: `crates/qbots/src/main.rs`

**What to do**:
- Track `map_servercount: Option<i32>` (set where `map_loaded = true`).
- In the ticker, when `map_loaded` and `conn.serverdata.servercount` differs: reset
  `map_loaded`, `nav_driver`, `collision`, `heatmap_obs`, `last_serverframe`,
  `last_health`, `last_frags`, `last_alive_pos`, `last_cmd`, `stall_mon`, `send_timing`;
  call `brain.on_death()` (clears enemy/goal/FSM state — same semantics as a respawn
  teleport). The existing `!map_loaded` block then reloads nav from the NEW map's
  configstring 33.
- Make `connect_deadline` mut; in the recv arm, on an `Active → non-Active` transition
  (map change re-handshake) push it forward by `connect_timeout_ms` so Plan 53's gate
  doesn't kill the bot (bug #5).

**Commit**: `task(T2): reload per-map state on servercount change in bot_task`

### T3: intermission — freeze the brain, vote next map

**File**: `crates/qbots/src/main.rs` (+ `pub const PM_FREEZE` in `crates/q2proto/src/playerstate.rs`)

**What to do**: Add `PM_FREEZE: u8 = 4` to q2proto (cite `shared.h:633-641`). In the
ticker's cmd build, when `frame.playerstate.pmove.pm_type == PM_FREEZE`: skip the brain
tick entirely; count intermission ticks; after 60 ticks (~6 s > the 5 s gate) send
`buttons = BUTTON_ATTACK | BUTTON_ANY` (constants from `brain::move_ctrl`). Reset the
counter when not frozen.

**Commit**: `task(T3): intermission freeze + ATTACK vote to advance the level`

### T4: live verification

**What to do**: `cargo run --release -- competition --brains q3,xon --count 2 --navmodes nm,sg --chars …` against cosmo.lan; then
`curl http://cosmo.lan:3000/api/rcon/execute … '{"command":"map q2dm2"}'` mid-run —
every bot must log the map-change reset, reload `q2dm2` nav, and keep fragging. Then let
fraglimit 5 hit — bots must freeze, press ATTACK, and survive the rotation. Record
findings in the tracker.

**Commit**: `task(T4): live-verify map-change + fraglimit survival` (tracker/notes only)

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/client/src/conn.rs` | `changing`/`reconnect` stufftext handling + state resets + tests | P0 |
| `crates/qbots/src/main.rs` | per-map reset on servercount change; deadline reset; intermission | P0 |
| `crates/q2proto/src/playerstate.rs` | `PM_FREEZE` const | P1 |

## Open Questions / Risks

1. **Missing `.qnav` for the new map is fatal** (`build_map_nav` aborts the process by
   design). q2dm1–8 caches exist in `data/mapcache/24/`; changing to an uncached map
   kills the fleet. Acceptable — same contract as startup; regenerate with
   `generate-map-cache` if the rotation grows.
2. **Brain residual state**: `set_map` swaps graph/items but xon's `ItemMemory` may hold
   old-map entity positions briefly. Mitigated by `brain.on_death()`; ratings self-correct
   from fresh PVS evidence.
3. **All bots press ATTACK at the same ~6 s tick** — harmless, any single press exits.
4. During the `changing → new-server` gap the bot receives nothing; the 100 ms ticker
   keeps flushing empty netchan transmits (keepalive), which is exactly what real clients
   do — no starvation risk, and the reset deadline bounds a hung changeover.

## Verification Checklist

- [x] T1: `cargo test -p client` green incl. new map-change flow test; clippy/fmt clean. **Committed.**
- [x] T2: fleet bot logs `map change` reset + `nav graph ready` for the new map after rcon `map q2dm2`; no `connect handshake timed out` on change. **Committed.**
- [x] T3: at fraglimit, logs show intermission detected, ATTACK pressed after ~6 s, server rotates. **Committed.**
- [x] T4: full live run: rcon change + fraglimit rotation, zero bot deaths (process-wise); findings in tracker. **Committed.**
- [x] Plan + tracker moved to `completed/`, SERIES.md updated.
