# RCON Password Quoting Regression

**Problem**: RCON commands were failing with "Bad rcon_password" even though the password was correct.

**Root Cause**: Commit `00284c4e9` accidentally removed quotes around the password in the RCON command format:
- **Working**: `rcon "ace123" status`
- **Broken**: `rcon ace123 status`

**Why it matters**: The Quake 2 server's `Cmd_TokenizeString` expects the password to be quoted so `Cmd_Argv(1)` parses correctly. Without quotes, the server rejects the password.

**Fix**: Always use `format!("rcon \"{}\" {}", password, command)` in both UDP and TCP implementations.

**Prevention**: Added regression tests `test_rcon_command_format` and `test_rcon_password_no_quotes_in_format` in `crates/rcon/src/lib.rs`.

## Sources
- `crates/rcon/src/lib.rs:75` (RCON command construction)
- Commit `006635d16` (correct implementation)
- Commit `00284c4e9` (regression that removed quotes)
- Commit `50dbfe768` (fix with tests)

---

# "Bad rcon_password" Can Mean Server-Side Flood Throttle (Not a Wrong Password)

**Problem**: Every rcon command (`status`, `dmflags`, ...) suddenly returned "Bad
rcon_password" even though the password was correct and unchanged — looks identical
to the quoting regression above, but the cause is different.

**Root Cause**: Quake 2 / q2pro servers throttle rapid rcon traffic. When polling
sends many commands in a short burst (here: 5 cvar round-trips per `/api/status`
poll, multiplied by repeated restarts + manual curls during debugging), the server
starts replying "Bad rcon_password" to *all* commands until the burst subsides. The
password and config are fine.

**How to tell them apart**: Send ONE raw rcon packet from the same host, bypassing
the app: `python3 -c '...sendto(b"\xff\xff\xff\xffrcon \"PW\" status", (HOST,PORT))'`.
If that single packet returns valid output, the password is correct and you are being
throttled — not a quoting/password bug. It also self-heals once traffic drops.

**Fix / Prevention**: Minimize rcon round-trips per poll. `get_status` now parses
settings from the single `status` serverinfo line and only issues a per-cvar query
for fields the serverinfo line omits (usually just `maxclients`), cutting the common
case from 5 commands to ~2. Don't hammer rcon in tight loops.

**Related bug found alongside**: the UDP read in `execute_udp` ignored the datagram
length — it stringified the whole 4096-byte zero-padded buffer (logs showed every
response as "4086 chars"). Fixed to slice `buf[4..n]` using the actual `recv` count.

## Sources
- `crates/api/src/main.rs` (`get_status` serverinfo-first, query-missing-only)
- `crates/rcon/src/lib.rs` (`execute_udp` length-aware read)

---

# Build Verification Before Committing

**Problem**: Claimed a feature was "done" without verifying the build, committing broken code.

**Rule**: NEVER claim "done" or commit unless:
1. `just fe-build` passes (TypeScript + Vite)
2. `just be-all` passes (Rust fmt, clippy, build)
3. Tests pass (`just be-test`)

**Prevention**: Always run the appropriate build command BEFORE committing.

---

# TypeScript Type Safety with Sets

**Problem**: `new Set(filteredMaps.map(...))` infers `Set<unknown>` instead of `Set<string>`.

**Fix**: Always use explicit type annotation: `new Set<string>(...)`

**Sources**:
- `frontend/src/components/AddMapDialog.tsx:194`

---

# JSX Structure Matching

**Problem**: Mismatched opening/closing tags in JSX cause cryptic TypeScript errors.

**Rule**: Always verify JSX structure:
- Each `<div>` has matching `</div>`
- Props are properly closed
- Nested structures are balanced

**Sources**:
- `frontend/src/components/AddMapDialog.tsx` (button group structure)

# `sv_uptime` on yquake2 is a Ghost Cvar — and the Map Clock Was Never Actually Unreadable

**Problem (as originally understood)**: To show a map countdown you need to know how long the current map has been running, and no channel qctrl speaks carries it. `level.time` lives in the game DLL with no cvar, no configstring, no serverinfo key; rcon `status` prints only the map name and the client table; the OOB status reply carries no clock. So elapsed time was *inferred* by watching the map name change — and if qctrl wasn't running when the map started, it was unrecoverable and had to be reported as unknown rather than guessed. (A guessed anchor is worse than none: it makes the rotation timer fire at an arbitrary moment. That part still stands.)

**The trap**: q2pro/q2repro have an `sv_uptime` cvar that adds `\uptime\MM.SS` to the status reply — a monotonic server clock, and seemingly the only way to notice a *server restart onto the same map* (no map-name change, so a naive edge detector keeps counting from a dead anchor). **yquake2 has no such cvar.** But `set sv_uptime 1` still *appears* to succeed there, because Q2 creates an inert user cvar for any unknown name and dutifully echoes `"sv_uptime" is "1"` back. qctrl believed restart detection was armed while it was doing precisely nothing — writing a junk cvar to someone's server every minute, and then detecting that it had had no effect. **All of it is now deleted. Do not reintroduce it.**

**The bigger lesson**: the premise was wrong. The server *does* publish the map clock — it just tells its **clients**, not its admins. `SV_SpawnServer` zeroes `sv.framenum` on every map spawn (`sv_init.c:267`), it increments at exactly 10 Hz (`sv_main.c:343`), and `svc_frame` carries it to every connected client every frame (`sv_entities.c:425`). So `serverframe / 10` **is** the exact age of the running map. "There is no way to read X" almost always means "there is no way to read X *on the channel I happen to be standing on*" — before concluding a value is unobtainable, enumerate every channel the software has, including the ones you aren't using. qctrl had no Q2 client; qbots had 32, already decoding this exact field and discarding it. See qctrl Plan 13 / qbots Plan 66.

**Avoidance**: Never treat a cvar echo as proof a feature works — test for the *effect*, not the acknowledgement. Q2 will happily accept, store, and echo back a cvar no code reads. Identify the engine before relying on its extensions (yquake2 reports `version\8.70` and `maxspectators`; q2pro-family builds differ). And when a value looks unobtainable, ask which *other* process in the system is already being told it.

## Sources
- qctrl: `crates/api/src/clock.rs` (`ClockAnchor::Unknown`; `observe_frame` — the anchor that measures instead of infers)
- qctrl: `crates/api/src/frames.rs` (the beacon reader that replaced the ghost)
- qbots: `crates/qbots/src/beacon.rs` (the producer)
- vendor/yquake2: `src/server/sv_init.c:267` (`memset(&sv,…)` zeroes framenum), `src/server/sv_main.c:343` (10 Hz), `src/server/sv_entities.c:425` (`svc_frame` → every client), `src/server/sv_main.c` (`SV_StatusString` — no uptime); vendor/q2repro `src/server/main.c:440` (the uptime block yquake2 lacks)

# The OOB `status` Query Is the Free Read Path — RCON Is for Mutations

**Problem**: `/api/status` used to do a live rcon round-trip per HTTP request. Six frontend components poll the `['status']` react-query key, which dedupes to the shortest interval (2s) — so an open browser meant an rcon `status` every 2 seconds against a server whose `sv_rcon_limit` defaults to **1/sec**. Past that limit a Q2 server answers *every* command with `Bad rcon_password`, which looks exactly like a misconfigured password (see the flood-throttle note above).

**The fix that was available all along**: the connectionless UDP status query — `\xff\xff\xff\xffstatus\n`, the one server browsers send — returns the entire serverinfo string (`mapname`, `timelimit`, `fraglimit`, `dmflags`, `maxclients`, `hostname`) plus a player line per client. It needs **no password** and is metered under `sv_status_limit` (default **15/sec**), a completely separate budget from rcon. Polling it at 1 Hz costs the rcon budget nothing.

**Avoidance**: Read with the OOB query; reserve rcon for mutations and for the two columns OOB does not carry — **client number and address** (`SV_StatusString` emits only frags/ping/name, so `clientkick` still needs an rcon `status` table). Serve HTTP from a cache so UI poll frequency is decoupled from server traffic. Measured: 30 `/api/status` reads now produce **1** rcon command, where the old path produced 30+. When merging the two player lists by name, refuse to guess on a duplicate or unseen name — emit `client_num: -1` and disable the action, because a wrong `clientkick` boots the wrong player.

## Sources
- qctrl: `crates/rcon/src/lib.rs` (`ServerQuery`)
- qctrl: `crates/api/src/oob.rs` (reply parser), `crates/api/src/status_cache.rs` (hybrid poller, player merge)
- vendor/q2repro: `src/server/main.c:425` (SV_StatusString), `src/server/main.c:2189` (sv_status_show/sv_status_limit defaults)
