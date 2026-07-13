# Plan 12 — sv_maplist Resilience + Empty-Map Guards

> **Status**: in-progress (T1–T3 done, T4 live verification pending)
> **Created**: 2026-07-12
> **Depends on**: Plan 11 (deployed API), rotation feature (post-Plan-11, unplanned)
> **Goal**: The Q2 server can never again crash on `maps/.bsp` — `sv_maplist` is continuously re-synced, and every path that could emit a `map`/`gamemap` command with an empty or bogus argument is guarded at the API and frontend layers.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Add a background `sv_maplist` re-sync loop to the API, reject empty/blank `map`/`gamemap` commands in `rcon_execute`, and guard the frontend's implicit map-restart paths against an empty/unknown current map.

**Deliverables**:
1. A 60 s background task in `crates/api/src/main.rs` that detects a drifted/empty `sv_maplist` on the server and re-pushes the rotation queue (check-then-push, so the server console isn't spammed when nothing changed).
2. A command guard in `rcon_execute` that returns HTTP 400 for `map`/`gamemap` with an empty argument (defense in depth — nothing reachable through qctrl can ever ask the server to load `maps/.bsp`).
3. Map-name validation on the rotation endpoints (`[A-Za-z0-9_\-]+` only) so a queue entry can never corrupt the `set sv_maplist "…"` command string.
4. Frontend guards: `buildApplyCommands` skips the implicit `map` restart when the current map is unknown; `RestartMap` disables itself on an empty/unknown map; `ServerStatusSync` never overwrites a known-good map with an empty one.
5. Unit tests for every guard (Rust + vitest), enumerated below, plus a live regression protocol.

**Estimated effort**: Small–Medium (half day)

## Context

### Pre-Identified Bug/Issue — the `maps/.bsp` server crash (observed live 2026-07-12)

The Q2 server (cosmo, Yamagi 8.70, custom baseq2 game built 2026-07-11) died with:

```text
------- server initialization ------
********************
ERROR: Couldn't load maps/.bsp
********************
==== ShutdownGame ====
```

Root-cause chain, established during the qbots Plan 64 investigation (see
`../qbots/context/plans/64_map_change_survival*.md`):

1. When a deathmatch match ends (fraglimit/timelimit), the game's `EndDMLevel`
   consults **`sv_maplist`** for the next map. On this server's game build, an
   empty `sv_maplist` resolves the next map to the **empty string**, producing
   `gamemap ""` → `maps/.bsp` → fatal error + game shutdown. qctrl already knew
   this: the doc comment on `spawn_sv_maplist_sync` (`crates/api/src/main.rs:404`)
   documents this exact death.
2. `spawn_sv_maplist_sync` is the existing protection — it pushes the rotation
   queue as `set sv_maplist "…"`. **But it only fires at qctrl-API startup and on
   rotation CRUD endpoints** (`main.rs:103,305,337,363`). Any Q2 **server** restart
   after the last push wipes the cvar, and the protection is silently gone until
   someone restarts qctrl or edits the rotation.
3. This stayed latent for months because an **all-bot server never ends
   intermission** (exiting requires a client to press a button ≥5 s in —
   `yquake2 game/player/client.c:2122`). qbots Plan 64 taught bots to press
   ATTACK during intermission, so the rotation now actually runs — and it ran
   straight into the empty `sv_maplist`.
4. Verified live during the incident: `rcon sv_maplist` returned `""` while
   `rotation.yaml` held 8 maps. The crash console shows **no `Rcon from …: map`
   line** before the error — the fatal command came from the game's internal
   rotation, not from any rcon client.

### Key Facts

- **The sync command**: `set sv_maplist "q2dm1 q2dm2 …"` (space-joined queue,
  `main.rs:422`). The game tokenizes on ` ,\n\r` (`EndDMLevel`, `g_main.c:227`).
- **The cvar echo format** (what a bare `sv_maplist` rcon query returns):
  `"sv_maplist" is "q2dm1 q2dm2"` — parse target for the drift check in T1.
- **rcon rate limits**: the server throttles rcon floods (replies `Bad
  rcon_password` when throttled — see the comment in `main.rs:234`). The re-sync
  loop must therefore be **check-then-push** (1 query/minute, and a second
  command only on drift), not a blind push every tick.
- **`RconClient::execute` serializes** all calls behind a mutex and sleeps 100 ms
  after each (`crates/rcon/src/lib.rs:41-72`) — the loop can share the existing
  `Arc<RconClient>` without extra locking.
- **Frontend polling**: the UI already polls `/api/status` every 2 s
  (`ServerStatusSync.tsx`, `refetchInterval: 2000`). Do NOT piggyback the sync on
  that path — status polls happen even with multiple tabs open and would
  multiply rcon traffic. A single API-side interval task is the right place.
- **Rotation "Random" mode is not implemented server-side** — `sv_maplist` is
  inherently sequential and `RotationQueue::next_map()` is dead code that returns
  `maps.first()`. Out of scope here; do not attempt to fix in this plan.
- The stray `[` characters seen on some rcon echo lines in the server console
  (`timelimit[`, `status[`) were audited during the incident: the qctrl send path
  (`crates/rcon/src/lib.rs:79-84`) cannot produce them; they are console-side
  rendering artifacts. **Non-issue — do not chase.**

### Why check-then-push (T1 design)

A blind `set sv_maplist …` every 60 s would (a) spam the server console with an
`Rcon from …` line pair every minute forever, and (b) count against rcon flood
protection, occasionally poisoning real commands with `Bad rcon_password`
throttle replies. Querying first costs one cheap command per minute and pushes
only on real drift (server restart, manual cvar clear). Drift is rare, so steady
state is one quiet query per minute.

### Why validate map names at the rotation endpoints (T2b)

`spawn_sv_maplist_sync` builds `set sv_maplist "{}"` by joining queue entries. A
queue entry containing `"` (or `;`, `$`) would escape the quoting and inject
into the server console command buffer. Entries come from the HTTP API
(`POST /api/rotation`, `PUT /api/rotation`), which today accepts any string.
Q2 map names on disk are `[A-Za-z0-9_-]+` — enforce exactly that.

## Step-by-Step Tasks

### T1: Background `sv_maplist` re-sync loop (API)

**File**: `crates/api/src/main.rs` (+ small refactor of `spawn_sv_maplist_sync`)

**What to do**:

1. Refactor the body of `spawn_sv_maplist_sync` into a reusable async fn so the
   startup push, the CRUD-endpoint pushes, and the new loop share one
   implementation:

```rust
/// Push `maps` to the server as `sv_maplist`. Returns Ok(()) on success.
/// An empty list is a no-op (never clear a good sv_maplist — that would
/// reintroduce the maps/.bsp crash this module exists to prevent).
async fn push_sv_maplist(rcon: &RconClient, maps: &[String]) -> Result<(), String> {
    if maps.is_empty() {
        return Ok(());
    }
    let command = format!("set sv_maplist \"{}\"", maps.join(" "));
    rcon.execute(&command)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}
```

   `spawn_sv_maplist_sync` keeps its signature (used at `main.rs:103,305,337,363`)
   and becomes a thin `tokio::spawn` wrapper around `push_sv_maplist` with the
   existing info/warn logging.

2. Add a pure helper that decides whether a re-push is needed, so the decision
   logic is unit-testable without a socket:

```rust
/// Extract the value from a cvar echo line: `"sv_maplist" is "q2dm1 q2dm2"`.
/// Returns None if the reply doesn't match the echo shape (e.g. server was
/// throttling and replied "Bad rcon_password", or returned an error string).
fn parse_cvar_echo<'a>(reply: &'a str, cvar: &str) -> Option<&'a str> {
    let line = reply.lines().find(|l| l.contains(&format!("\"{cvar}\" is ")))?;
    // value is the LAST quoted span on the line
    let mut parts = line.rsplitn(3, '"');
    let _tail = parts.next()?;      // after the closing quote (usually empty)
    let value = parts.next()?;      // the value itself
    Some(value)
}

/// True when the server's live sv_maplist does not match the queue we want.
/// `None` (unparseable reply) → treat as NOT drifted: never push on garbage,
/// or a throttled server would get hammered exactly when it's least happy.
fn maplist_drifted(live: Option<&str>, wanted: &[String]) -> bool {
    match live {
        None => false,
        Some(v) => v.trim() != wanted.join(" "),
    }
}
```

3. In `main()`, after the existing startup sync block (`main.rs:101-104`), spawn
   the loop:

```rust
// Plan 12: the startup/CRUD pushes are lost whenever the Q2 SERVER restarts
// (cvars reset), which re-arms the maps/.bsp crash at the next match end.
// Re-check every 60 s and re-push only on drift (check-then-push keeps the
// server console quiet and stays under rcon flood protection).
{
    let state = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let wanted = state.rotation_queue.lock().await.get_maps();
            if wanted.is_empty() {
                continue; // nothing to protect with; never push/clear empties
            }
            match state.rcon_client.execute("sv_maplist").await {
                Ok(reply) => {
                    let live = parse_cvar_echo(&reply, "sv_maplist");
                    if maplist_drifted(live, &wanted) {
                        tracing::warn!(
                            live = live.unwrap_or("<unparseable>"),
                            "sv_maplist drifted (server restarted?) — re-pushing"
                        );
                        if let Err(e) = push_sv_maplist(&state.rcon_client, &wanted).await {
                            tracing::warn!("sv_maplist re-push failed: {e}");
                        }
                    }
                }
                // Server down/unreachable: stay quiet at debug level — this
                // loop must not turn a server outage into a log flood.
                Err(e) => tracing::debug!("sv_maplist check skipped: {e}"),
            }
        }
    });
}
```

**Design constraints (do not deviate without noting in the tracker)**:
- Interval **60 s**; first tick fires immediately (tokio `interval` semantics) —
  acceptable: it doubles as a startup retry if the boot-time push raced a
  server that was still coming up.
- Never push when the queue is empty (parity with the existing
  `spawn_sv_maplist_sync` guard and its comment, `main.rs:413-415`).
- Never push when the query reply is unparseable (throttle safety, see helper).
- Do **not** gate on `rotation_enabled` — the existing pushes aren't gated on it
  either, and `sv_maplist` is the crash-prevention floor, not a UI preference.
  (If the operator truly wants no rotation, they empty the queue.)

**Unit tests** (same file, `#[cfg(test)]`, no socket needed):

| # | Case | Input | Expect |
|---|------|-------|--------|
| 1 | echo parses | `"sv_maplist" is "q2dm1 q2dm2"` | `parse_cvar_echo` → `Some("q2dm1 q2dm2")` |
| 2 | empty value parses | `"sv_maplist" is ""` | `Some("")` |
| 3 | throttle reply | `Bad rcon_password.` | `None` |
| 4 | multi-line reply (banner + echo) | `foo\n"sv_maplist" is "a b"\n` | `Some("a b")` |
| 5 | drift: empty live vs 2 wanted | `Some("")`, `["q2dm1","q2dm2"]` | `maplist_drifted` → `true` |
| 6 | no drift: exact match | `Some("q2dm1 q2dm2")`, same | `false` |
| 7 | no drift on unparseable | `None`, `["q2dm1"]` | `false` |
| 8 | drift: different order | `Some("q2dm2 q2dm1")`, `["q2dm1","q2dm2"]` | `true` |
| 9 | whitespace-tolerant match | `Some(" q2dm1 q2dm2 ")`, `["q2dm1","q2dm2"]` | `false` |

**Commit**: `task(T1): background sv_maplist drift re-sync loop`

### T2: Reject empty `map`/`gamemap` in `rcon_execute` + validate rotation map names

**Files**: `crates/api/src/main.rs` (`rcon_execute`, `add_to_rotation`, `update_rotation`); consider a new `crates/api/src/guard.rs` if `main.rs` gets crowded.

**What to do**:

1. Pure command validator:

```rust
/// Reject rcon commands that would make the server load an empty map name
/// ("map" / "gamemap" with a blank argument → fatal "Couldn't load maps/.bsp").
/// Everything else passes through untouched — this is a tripwire, not a filter.
fn validate_rcon_command(command: &str) -> Result<(), String> {
    let trimmed = command.trim();
    let mut it = trimmed.split_whitespace();
    let head = it.next().unwrap_or("").to_ascii_lowercase();
    if head == "map" || head == "gamemap" {
        // strip optional surrounding quotes from the arg before judging it
        let arg = it.next().unwrap_or("").trim_matches('"').trim();
        if arg.is_empty() {
            return Err(format!(
                "refusing '{head}' with an empty map name (this crashes the server on maps/.bsp)"
            ));
        }
    }
    Ok(())
}
```

2. Call it at the top of `rcon_execute` (`main.rs:430`), before
   `state.rcon_client.execute(...)`. On `Err(msg)`: log at `warn!`, broadcast an
   `ERROR` line on `state.log_stream` (so the UI console shows why nothing
   happened), and return `StatusCode::BAD_REQUEST`. Check `ExecuteResponse`'s
   shape first — if the frontend expects a JSON body on failure, return
   `(StatusCode::BAD_REQUEST, Json(ExecuteResponse { success: false, output: msg }))`
   in whatever form the existing type supports; match the existing error style
   of the handler.

3. Map-name validator for the rotation endpoints:

```rust
/// Q2 map names as they appear on disk: letters, digits, underscore, hyphen.
/// Anything else could escape the quoting in `set sv_maplist "…"`.
fn valid_map_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
```

   Apply in `add_to_rotation` (reject the single `payload.map_name`) and
   `update_rotation` (reject the whole request if ANY entry fails; name the bad
   entry in the message). Return `BAD_REQUEST` with the existing response
   types' failure shape.

**Unit tests**:

| # | Case | Input | Expect |
|---|------|-------|--------|
| 1 | bare map | `map` | Err |
| 2 | map + spaces | `map   ` | Err |
| 3 | map + empty quotes | `map ""` | Err |
| 4 | gamemap empty | `gamemap` | Err |
| 5 | case-insensitive | `MAP` | Err |
| 6 | valid map | `map q2dm1` | Ok |
| 7 | valid gamemap | `gamemap q2dm3` | Ok |
| 8 | unrelated cmds untouched | `status`, `fraglimit 5`, `set sv_maplist "a b"`, `kick 3` | Ok |
| 9 | map inside another word is NOT matched | `mapcycle foo` | Ok |
| 10 | valid names | `q2dm1`, `the_edge`, `ztn2dm3-b` | `valid_map_name` → true |
| 11 | invalid names | ``, `q2dm1"; quit`, `maps/q2dm1`, `q2 dm1`, 65+ chars | false |

**Commit**: `task(T2): guard rcon_execute + rotation endpoints against empty/bogus map names`

### T3: Frontend guards (apply flow, restart button, status sync)

**Files**: `frontend/src/lib/applyLogic.ts`, `frontend/src/components/RestartMap.tsx`, `frontend/src/components/ServerStatusSync.tsx`, `frontend/src/components/ChangesQueueUI.tsx` (call-site), new `frontend/src/lib/__tests__/applyLogic.test.ts`.

**What to do**:

1. **`applyLogic.ts`** — `buildApplyCommands` (line 22) currently ALWAYS appends
   `map ${currentMap}` when no map change is queued (line 49). Guard it:

```ts
const UNKNOWN_MAPS = new Set(['', 'unknown', 'Unknown']);

export function buildApplyCommands(changes: Change[], currentMap: string): string[] {
  // ... existing cvar command building unchanged ...

  const mapChange = changes.find((c) => c.type === 'map');
  if (mapChange && String(mapChange.pendingValue).trim() !== '') {
    commands.push(`map ${mapChange.pendingValue}`);
  } else if (!UNKNOWN_MAPS.has(currentMap.trim())) {
    // Implicit restart to apply cvar changes — only when we actually know
    // the current map. `map <empty/unknown>` would drop or crash the server.
    commands.push(`map ${currentMap}`);
  }
  // else: send cvar changes without a restart; they take effect next map load.
  return commands;
}
```

   Keep the return type `string[]` (no signature break for `ChangesQueueUI`/e2e).
   If UX feedback is wanted when the restart is skipped, `ChangesQueueUI` may
   toast "settings apply at next map change" — optional, P2.

2. **`RestartMap.tsx`** — the prop default only covers `undefined`; an empty
   string sails through (`Deathmatch.tsx:54` passes `status?.map ?? undefined`,
   and `''` is not nullish). Disable on unknown:

```tsx
export function RestartMap({ currentMap = 'q2dm1' }: RestartMapProps) {
  const known = currentMap.trim() !== '' && currentMap !== 'unknown';
  // ...
  <Button disabled={isPending || !known} onClick={() => known && execute(`map ${currentMap}`)}>
    {known ? `Restart Current Map (${currentMap})` : 'Restart unavailable (map unknown)'}
  </Button>
```

3. **`ServerStatusSync.tsx`** — `currentMap: status.map ?? 'unknown'` (line 31)
   forgets the last good map whenever one poll comes back without one (server
   mid-change/down). Retain the previous known value instead:

```tsx
currentMap: (status.map && status.map.trim() !== '')
  ? status.map
  : lastGoodMapRef.current ?? 'unknown',
```

   with a `lastGoodMapRef = useRef<string | null>(null)` updated whenever a
   non-empty `status.map` arrives. This keeps Apply/Restart working through a
   momentary status blip instead of degrading to 'unknown'.

**Vitest tests** (`frontend/src/lib/__tests__/applyLogic.test.ts`):

| # | Case | changes / currentMap | Expect |
|---|------|----------------------|--------|
| 1 | explicit map change | `[{type:'map', pendingValue:'q2dm3'}]`, `'q2dm1'` | last command `map q2dm3` |
| 2 | implicit restart, known map | `[{type:'fraglimit', pendingValue:10}]`, `'q2dm1'` | `['fraglimit 10', 'map q2dm1']` |
| 3 | implicit restart, EMPTY map | same changes, `''` | `['fraglimit 10']` — **no** `map` command |
| 4 | implicit restart, 'unknown' | same changes, `'unknown'` | no `map` command |
| 5 | empty queued map value | `[{type:'map', pendingValue:''}]`, `'q2dm1'` | falls back to `map q2dm1` (known) |
| 6 | empty queued map + unknown current | `[{type:'map', pendingValue:''}]`, `''` | no `map` command at all |
| 7 | no changes, known map | `[]`, `'q2dm2'` | `['map q2dm2']` (existing behavior preserved) |

   Run: `cd frontend && npm run test` (vitest; e2e excluded). Also run
   `npm run lint` and `npm run build` (Rule A equivalent for TS).

**Commit**: `task(T3): frontend guards for empty/unknown current map`

### T4: Live regression verification

**No code.** Requires the live server + a qbots fleet (coordinate with the
operator; the qbots side already handles both map-change flows — see qbots
Plan 64).

Protocol — record every result in the tracker:

1. **Deploy/restart the qctrl API** (operator does the restart). Confirm log
   line `Synced sv_maplist (8 maps) to server`, then `rcon sv_maplist` shows
   all 8 maps.
2. **Drift detection**: `rcon set sv_maplist ""` by hand. Within ~60 s the API
   must log the `sv_maplist drifted … re-pushing` warning and
   `rcon sv_maplist` must show the queue again. This is THE regression test
   for the incident.
3. **Guard checks** (expect HTTP 400 + no server console `Rcon from` line for
   the map command):
   - `curl -X POST …/api/rcon/execute -d '{"command":"map"}'`
   - `-d '{"command":"map   "}'`, `-d '{"command":"gamemap \"\""}'`
   - `curl -X POST …/api/rotation -d '{"map_name":"bad name"}'` → 400
   - Control: `-d '{"command":"map q2dm2"}'` still works (200, map changes).
4. **The original crash scenario, end-to-end**: fraglimit 5, start a qbots
   fleet (`qbots competition …`), let a match end. Expected: intermission →
   bots press ATTACK → server rotates via `sv_maplist` to the next map —
   **no `Couldn't load maps/.bsp`**, no game shutdown; bots rejoin (soft
   `changing`/`reconnect` path) and keep playing.
5. **UI spot-check**: with the server briefly down (or between maps), the
   Restart button reads "Restart unavailable (map unknown)" and Apply without
   a queued map sends only cvar commands.

**Commit**: `task(T4): live regression protocol executed` (tracker/notes only)

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/api/src/main.rs` | re-sync loop, `push_sv_maplist`, `parse_cvar_echo`, `maplist_drifted`, `validate_rcon_command` wiring, rotation-name validation | P0 |
| `frontend/src/lib/applyLogic.ts` | skip implicit `map` restart on empty/unknown | P0 |
| `frontend/src/components/RestartMap.tsx` | disable on empty/unknown map | P1 |
| `frontend/src/components/ServerStatusSync.tsx` | retain last good map | P1 |
| `frontend/src/lib/__tests__/applyLogic.test.ts` | new vitest suite | P0 |
| `context/plans/SERIES.md` | add Plan 12 row (the "All Plans Complete" banner is stale) | P1 |

## Open Questions / Risks

1. **`ExecuteResponse` failure shape** — check how the frontend consumes
   `/api/rcon/execute` errors before choosing between plain `400` and
   `400 + JSON body`; keep the UI's error toast working. Mitigation: grep
   `frontend/src/lib/api.ts` for the execute call's error handling first.
2. **rcon flood protection** — the extra 1 query/min is negligible, but if the
   loop ever logs `Bad rcon_password` style replies, that is throttling, not a
   wrong password; the unparseable-reply → no-push rule already covers it.
   Do not "fix" the password on that symptom.
3. **Server console noise** — every rcon command echoes on the server console.
   Steady-state addition is one `sv_maplist` query per minute. If the operator
   objects, raise the interval to 300 s (do not go below 30 s).
4. **`interval` first-tick-immediate** overlaps the startup push (two pushes in
   the first minute if the server was empty). Harmless — `set` is idempotent.
5. **Frontend `'unknown'` literal** appears in `ServerStatusSync` (existing) and
   the new guards — keep the `UNKNOWN_MAPS` set in ONE exported place
   (`applyLogic.ts`) and import it, so the sentinel can't drift.
6. **Do not** touch rotation "Random" mode or `next_map()` dead code in this
   plan (noted in Key Facts; separate cleanup if ever).

## Verification Checklist

- [ ] T1: `cargo build` + `cargo clippy` zero warnings; new unit tests 1–9 pass (`cargo test -p qctrl-api` or workspace equivalent). **Committed.**
- [ ] T1: live: `set sv_maplist ""` by hand → auto-restored within ~60 s (logged).
- [ ] T2: unit tests 1–11 pass; `curl` with `{"command":"map"}` returns 400 and the server console shows NO corresponding `Rcon from` map line. **Committed.**
- [ ] T3: `npm run test` green including the 7 new applyLogic cases; `npm run lint` + `npm run build` clean. **Committed.**
- [ ] T4: fraglimit match-end rotation completes on the live server with a qbots fleet connected — no `maps/.bsp`, no game shutdown; results recorded in tracker. **Committed.**
- [ ] SERIES.md updated with Plan 12; plan + tracker moved to `completed/` when done.

---

## Follow-up (superseded assumption)

This plan treats `sv_maplist` sync as the thing that makes "the server's own rotation" a
correct fallback. That framing was too strong, and a later change corrected it.

`sv_maplist` decides **which** map the changelevel targets. It does **not** make the
intermission exit fire — as §Root Cause 3 above already notes, that needs a client to press
a button ≥5 s in (`yquake2 game/player/client.c:2122`). So on an idle server there is no
"server's own rotation" to fall back *to*; the match simply never ends. Rotation was living
in a React hook, which meant an unattended server stopped rotating as soon as the last
browser tab closed, and would sit in intermission until someone opened the frontend.

Rotation now has a headless owner in the backend: `crates/api/src/rotator.rs` (policy) and
`spawn_rotator` in `main.rs` (the task). Everything this plan built is unchanged and still
needed — `sv_maplist` remains the right destination whenever a real player *does* press fire
and the server exits intermission on its own. Item 6 above ("do not touch Random mode or
`next_map()` dead code") is now resolved: `next_map()` is gone and `Random` is implemented in
`rotator::select_next`, where it finally works without a browser driving it.
