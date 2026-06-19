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
