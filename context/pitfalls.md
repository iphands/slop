# Programming Pitfalls and Lessons Learned

This document captures bugs, gotchas, and anti-patterns discovered while building software in this repository, along with guidance on how to avoid them.

## Streaming Response Delta Calculation in Proxies

### The Problem
When building a streaming HTTP proxy that fixes malformed JSON responses chunk-by-chunk, naively sending the "fixed" complete JSON to the client will corrupt their accumulated result, creating duplicate or malformed fields.

### Description (200 words)
In streaming APIs (SSE/Server-Sent Events), clients accumulate delta strings from each chunk to build the complete response. When a proxy intercepts the stream to fix malformed content (e.g., duplicate fields, broken JSON syntax), it must send a **completion delta** that completes what the client has already accumulated - NOT the full fixed JSON.

The bug manifests when the proxy detects malformed content spanning multiple chunks. After fixing the accumulated JSON, the proxy needs to calculate "what has the client already received?" to determine "what delta should I send to complete it validly?"

Using simple string matching like `accumulated.ends_with(current_chunk)` fails in real-world scenarios due to:
- JSON escaping differences (e.g., `\n` vs `\\n` after parse/serialize round-trips)
- UTF-8 multi-byte character boundaries
- Whitespace normalization
- Partial string overlaps

When the string matching fails, a naive fallback of `already_sent = ""` causes the proxy to assume "nothing was sent yet" and send the FULL fixed JSON. The client then appends this to what it already has, creating duplicate fields:

```
Client has: {"content":"...","filePath":"/path1",
Proxy sends: {"content":"...","filePath":"/path1"}  [FULL JSON - BUG!]
Client gets: {"content":"...","filePath":"/path1",{"content":"...","filePath":"/path1"}  [INVALID!]
```

### How to Avoid (200 words)
**Principle**: When modifying streaming responses, always work with deltas, never full content.

**Solution Pattern**:
1. **Track state carefully**: Maintain accurate bookkeeping of what was sent to the client
   - Use byte/character position tracking instead of string matching when possible
   - Store cumulative state in a dedicated accumulator structure

2. **Robust delta calculation**:
   ```rust
   // GOOD: Try multiple matching strategies
   let already_sent_len = if accumulated.ends_with(current_chunk) {
       accumulated.len() - current_chunk.len()
   } else if let Some(pos) = accumulated.rfind(current_chunk) {
       pos  // Fallback to last occurrence
   } else {
       // CRITICAL: Don't assume nothing was sent!
       // Send a safe completion instead
       tracing::warn!("Delta calc failed - using safe fallback");
       accumulated.len()  // Treat as "send minimal completion"
   };
   ```

3. **Safe fallback behavior**:
   - If delta calculation is uncertain, send a **minimal safe completion** (e.g., `}` to close JSON)
   - Never send full fixed content when you're unsure what the client has
   - Log warnings when fallback is used for debugging

4. **Test with realistic scenarios**:
   - Test with escaped characters (`\n`, `\"`, Unicode)
   - Test with JSON round-tripping (parse → serialize changes formatting)
   - Test with multi-byte UTF-8 characters
   - Verify client-side accumulation produces valid result

5. **Defensive completions**: When you must send a completion but state is uncertain, prefer sending a dummy field that consumes trailing punctuation (e.g., `"_":null}`) over risking invalid JSON.

### Sources
- llama-proxy: `src/fixes/toolcall_bad_filepath_fix.rs` (function: `apply_stream_with_accumulation_default`)
- llama-proxy: `src/proxy/streaming.rs` (SSE stream processing and fix application)
- Git commit: 2be7f3a "Still fighting the filePath fix" (historical context of the bug)

## Trait Method Overriding with Multiple Signatures

### The Problem
When a Rust trait provides multiple method variants with default implementations (e.g., one with extra parameters, one without), you must override ALL variants that are actually called in production, not just the one you think is "most specific".

### Description (200 words)
In llama-proxy, the `ResponseFix` trait defines two methods for streaming with accumulation:
- `apply_stream_with_accumulation(chunk, request, accumulator)` - with request context
- `apply_stream_with_accumulation_default(chunk, accumulator)` - without request context

Both have default implementations that delegate to other methods but **ignore the accumulator**. The `ToolcallBadFilepathFix` initially only overrode the `_default` variant, assuming it would handle both cases.

**The bug**: In production, when `request_json` is successfully parsed (which it always is for valid requests), the registry calls the WITH-request variant. Since that wasn't overridden, it used the trait default which called `apply_stream_with_context()`, completely bypassing the accumulation logic. The fix never ran, and duplicate filePath bugs went undetected.

This is subtle because:
- Unit tests called the override directly, so they passed
- Logging showed "Fix did not apply" for every chunk, but no clear error
- The override method existed and was syntactically correct
- Dynamic dispatch worked fine - it correctly called the trait default!

The fix appeared to work in tests but failed in production because tests explicitly called the overridden method, while production code path used the non-overridden variant.

### How to Avoid (200 words)
**Principle**: When overriding trait methods, audit ALL call sites to understand which variant is actually invoked in production.

**Solution Pattern**:
1. **Identify all call sites**: Use `grep` or IDE "find usages" to see which trait methods are called
   ```bash
   grep -r "apply_stream_with_accumulation" src/
   ```

2. **Override all production variants**: If a trait has multiple method signatures, override the ones that are actually called:
   ```rust
   impl MyTrait for MyType {
       // Override the WITH-parameter variant (called in production)
       fn method_with_param(&self, chunk: Value, param: &Param) -> Result {
           self.method_without_param(chunk)  // Delegate if param unused
       }

       // Override the WITHOUT-parameter variant (called in tests)
       fn method_without_param(&self, chunk: Value) -> Result {
           // Core logic here
       }
   }
   ```

3. **Add diagnostic logging**: When debugging "why isn't my override called?", add entry logging:
   ```rust
   fn my_override(&self, ...) {
       tracing::debug!("OVERRIDE CALLED - MyType.my_override");
       // ... rest of logic
   }
   ```
   If you never see this log in production, the override isn't being called.

4. **Test the actual code path**: Unit tests that directly call methods may pass while integration tests fail. Test through the same entry point as production (e.g., HTTP request → handler → registry → fix).

5. **Check trait defaults**: When a trait has default implementations that delegate, verify your override actually interrupts the delegation chain at the right point.

### Sources
- llama-proxy: `src/fixes/mod.rs` (lines 181-198: trait defaults that bypass accumulator)
- llama-proxy: `src/fixes/toolcall_bad_filepath_fix.rs` (fix that initially only overrode one variant)
- llama-proxy: `src/proxy/streaming.rs` (line 145: calls the WITH-request variant in production)

## Discovering API Types Reactively Instead of Proactively

### The Problem
When building format converters or API proxies, discovering content/message types **reactively** (only when hitting errors) leads to incomplete type coverage, production parsing failures, and multiple fix cycles for the same root cause. This pattern creates technical debt and user-facing errors that could be prevented with upfront research.

### Description
In llama-proxy, we initially implemented Anthropic content block types incrementally as we encountered them:

1. **Initial implementation**: Only `text` and `thinking` content blocks
2. **Hit parsing error**: Backend sent `tool_use` block we didn't handle
3. **Debug session**: Discover `tool_use` exists, add to enum
4. **Fix and deploy**: Parsing works again
5. **Later**: Hit same error with `tool_result` blocks
6. **Repeat cycle**: Add `tool_result`, fix parsing
7. **Potential future**: Will hit errors if backend sends `image` blocks (llama.cpp supports this but proxy doesn't)

This reactive approach causes:
- **Production errors**: Legitimate backend responses fail to parse ("unknown variant `tool_use`")
- **User friction**: Features break when backends evolve or use uncommon types
- **Wasted effort**: Multiple debugging sessions for the same root issue
- **Incomplete coverage**: Missing types only discovered when users hit specific code paths
- **False confidence**: Tests pass, but only cover types we've encountered

The **root cause** is treating API integration as "implement what we see right now" rather than "implement the complete specification". We were parsing incrementally based on observed traffic instead of consulting authoritative sources (official SDKs, documentation, backend source code).

### How to Avoid
**Principle**: Research ALL possible types/variants BEFORE implementing parsers, converters, or API clients.

**Solution Pattern**:

1. **Consult authoritative sources first**:
   ```bash
   # Check official SDK for complete type list
   cd vendor/anthropic-sdk-python
   grep -r "class.*Block" src/

   # Check backend source for what it actually sends
   cd vendor/llama.cpp
   grep -r "content_block.*type" tools/server/

   # Read official API docs
   curl https://api.anthropic.com/docs/messages
   ```

2. **Document complete type catalogs**:
   - Create `context/<api_name>_api.md` with ALL types (even if not implementing yet)
   - List what backend supports vs. what official API supports
   - Note which types you're implementing vs. deferring

3. **Implement all types together**:
   ```rust
   // GOOD: Complete enum from research
   #[derive(Deserialize)]
   #[serde(tag = "type")]
   enum AnthropicContentBlock {
       #[serde(rename = "text")]
       Text { text: String },
       #[serde(rename = "thinking")]
       Thinking { thinking: String, signature: Option<String> },
       #[serde(rename = "tool_use")]
       ToolUse { id: String, name: String, input: Value },
       #[serde(rename = "tool_result")]
       ToolResult { tool_use_id: String, content: String },
       #[serde(rename = "image")]  // Even if not handling yet
       Image { source: ImageSource },
       // ... other types from spec
   }

   // BAD: Only implementing what we've seen
   enum AnthropicContentBlock {
       Text { text: String },
       // We'll add more when we hit errors...
   }
   ```

4. **Handle unimplemented types gracefully**:
   ```rust
   match content_block {
       ContentBlock::Text { .. } => handle_text(),
       ContentBlock::Thinking { .. } => handle_thinking(),
       ContentBlock::ToolUse { .. } => handle_tool_use(),
       ContentBlock::Image { .. } => {
           tracing::warn!("Image blocks not yet supported, skipping");
           Ok(())  // Don't crash
       }
       _ => {
           tracing::error!("Unknown content block type");
           Err(Error::UnsupportedContentBlock)
       }
   }
   ```

5. **Create comprehensive documentation**:
   - `context/<api>_complete_types.md` - All official types
   - `context/<backend>_supported_types.md` - What backend actually sends
   - `context/<formats>_mapping.md` - Conversion possibilities/limitations

6. **Test with all known types**:
   ```rust
   #[test]
   fn test_parse_all_content_block_types() {
       // Even if we don't handle them, ensure we can parse
       let blocks = vec![
           r#"{"type":"text","text":"hi"}"#,
           r#"{"type":"thinking","thinking":"..."}"#,
           r#"{"type":"tool_use","id":"x","name":"y","input":{}}"#,
           r#"{"type":"tool_result","tool_use_id":"x","content":"z"}"#,
           r#"{"type":"image","source":{"type":"url","url":"..."}}"#,
           // Test ALL types, even unimplemented ones
       ];

       for block_json in blocks {
           let result = serde_json::from_str::<ContentBlock>(block_json);
           // Should parse without error (even if handling is TODO)
           assert!(result.is_ok(), "Failed to parse: {}", block_json);
       }
   }
   ```

**Proactive Research Checklist**:
- [ ] Read official API documentation
- [ ] Clone and search official SDK source code
- [ ] Check backend/server source for what it actually sends
- [ ] Document complete type catalog before coding
- [ ] Implement all known types (even if handlers are stubs)
- [ ] Test parsing all documented types
- [ ] Log warnings for unimplemented types instead of crashing

### Sources
- llama-proxy: `src/api/openai.rs` - Initially missing `tool_use`, `tool_result`, still missing `image`
- llama-proxy: Multiple commits incrementally adding content block types after hitting errors
- Created comprehensive documentation: `context/anthropic_api.md`, `context/llama_cpp_supported_types.md`, `context/openai_anthropic_mapping.md`

## UTF-8 Char Boundary Panics When Byte-Slicing Strings

### The Problem
Rust panics with `byte index N is not a char boundary` when you slice a `&str` at a byte offset that falls inside a multi-byte UTF-8 character (e.g., emoji = 4 bytes, accented chars = 2 bytes).

### Description
In llama-proxy's `chunk_text()` function, text was split into fixed-size chunks using byte arithmetic: `end = start + max_size`. When the calculated `end` landed inside an emoji (e.g., 💡 is 4 bytes, offset 50 lands at byte 2 of the emoji at bytes 48..52), the slice `text[start..end]` panicked. The function was documented as splitting on "chars" but worked with byte offsets — a hidden mismatch. The bug only surfaced in production when LLM responses contained emojis, since unit tests used ASCII-only text.

Additionally, `rfind()` returns a byte offset; using `i + 1` to advance past the found whitespace char is wrong for multi-byte spaces (U+00A0 no-break space = 2 bytes, U+2009 thin space = 3 bytes).

### How to Avoid
1. **Never slice `&str` at raw byte offsets from arithmetic.** Always verify with `str::is_char_boundary()` first.
2. Use a `floor_char_boundary` helper to walk back to the nearest valid boundary:
   ```rust
   fn floor_char_boundary(s: &str, index: usize) -> usize {
       if index >= s.len() { return s.len(); }
       let mut i = index;
       while i > 0 && !s.is_char_boundary(i) { i -= 1; }
       i
   }
   ```
3. When finding whitespace with `rfind`, use `char_indices().rev()` and advance by `c.len_utf8()` instead of `+ 1`.
4. **Test with emoji-heavy strings** — they are the most common multi-byte content in LLM output.

### Sources
- llama-proxy: `src/proxy/synthesis.rs` (`chunk_text` function)

# q2repro server rejects yquake2 clients: "Unsupported protocol 2"
q2repro (Paril's Q2PRO/re-release fork) uses the `q2proto` library, which
abstracts multiple wire protocols behind an enum: `Q2P_PROTOCOL_VANILLA=2`
(wire protocol 34, what yquake2/original Q2 speak), `R1Q2=3`, `Q2PRO=4`,
`Q2REPRO=8`, `KEX=10`. The server has a COMPILE-TIME allow-list in
`src/server/main.c`:
`static const q2proto_protocol_t q2repro_accepted_protocols[] = {Q2P_PROTOCOL_Q2REPRO};`
It accepts ONLY its own Q2REPRO protocol. A yquake2 client connects with
vanilla (34), `q2proto_parse_connect()` returns `Q2P_ERR_PROTOCOL_NOT_SUPPORTED`,
and the server prints `Unsupported protocol %d.` where `%d` is the q2proto ENUM
value (2 = VANILLA), NOT the wire number 34 — which is why the message says "2".
There is no cvar for this; it's hardcoded.

### How to avoid / fix
- To serve vanilla clients (yquake2, original Q2, most source ports), either run
  a yquake2/vanilla server, OR patch the allow-list to add the protocols you
  want, e.g. `{Q2P_PROTOCOL_Q2REPRO, Q2P_PROTOCOL_Q2PRO, Q2P_PROTOCOL_R1Q2,
  Q2P_PROTOCOL_VANILLA}`. q2proto ships full SERVER-side impls for all of these
  (`q2proto_proto_vanilla.c`, `_r1q2.c`, `_q2pro.c`), so widening works — but the
  re-release game's extended content (big maps, extended indices) may not fully
  represent over vanilla. The same list also feeds the challenge advertisement
  (`q2proto_get_challenge_extras`, main.c ~619), so widening it is consistent.
- Remember q2proto's "protocol N" in errors is the enum ordinal, not the wire
  protocol version.

### Sources
- qcontainer: vendor/q2repro/src/server/main.c (`q2repro_accepted_protocols`, `SVC_DirectConnect`)
- qcontainer: vendor/q2repro/q2proto/inc/q2proto/q2proto_protocol.h (enum)
- qcontainer: vendor/yquake2/src/common/header/common.h (`PROTOCOL_VERSION 34`)

# yquake2 server lockup on "status" with long player names (unsigned underflow)
In yquake2 `SV_Status_f` (src/server/sv_cmd.c), the column-padding width `l` is
declared `size_t` (unsigned). For each connected client it computes
`l = 16 - strlen(cl->name);` then `for (j = 0; j < l; j++) Com_Printf(" ");`
(and `l = 22 - strlen(s);` for the address). If a player's name is longer than
16 chars, the subtraction underflows to ~2^64; `j` (int) is promoted to size_t
in the comparison, so the loop iterates astronomically -> the server's main
thread hangs and the WHOLE server locks up (not a crash). Reachable via console
or `rcon status`. It's a regression from vanilla id Quake II, where `l` was
`int` (result goes negative, loop is skipped). Symptom looked player-count
related ("~24 players") but the real trigger is ANY single name > 16 chars,
which just becomes likely as the lobby fills.

### How to avoid
- Never subtract `strlen()` (size_t) into an unsigned and then loop `int < that`.
  Keep padding widths signed: `int l = 16 - (int)strlen(name);` so a too-long
  string yields a negative width and the loop is skipped. Watch for size_t vs
  int mismatches in any `for (int j; j < unsigned; j++)` padding loop.
- Fixed in qcontainer via patches/yquake/0001-fix-status-name-padding-underflow.patch
  (applied at image build time), since the yquake flavor tracks upstream tags.

### Sources
- qcontainer: vendor/yquake2/src/server/sv_cmd.c (`SV_Status_f`)

# Quake 2 rcon replies are multi-datagram — a single recv() truncates "status" (~18 players)
A large `rcon status` response does NOT arrive in one UDP datagram. The server
redirects console output through `Com_BeginRedirect`/`SV_FlushRedirect` into a
fixed buffer `sv_outputbuf` sized `SV_OUTPUTBUF_LENGTH` (yquake2: `MAX_MSGLEN-16`
= 1384 bytes; q2repro: `MAX_PACKETLEN_DEFAULT-16`). Whenever the next line would
overflow that buffer, the current buffer is flushed as its OWN connectionless
packet — `\xff\xff\xff\xff` + `print\n<chunk>` — and reset (clientserver.c:
`if ((msgLen + strlen(rd_buffer)) > (rd_buffersize-1)) rd_flush(...)`). So the
full reply is several separate datagrams, each with its own 0xFFFFFFFF prefix and
leading `print\n`.

A client that calls `recv()` exactly once reads only the FIRST datagram and
silently drops the rest. The first ~1384 bytes = the header block + about the
first 18 player rows (~65 bytes/row), which presents as a hard "only 18 players"
ceiling even though the parser and UI are unbounded. The recv buffer size (e.g.
4096) is irrelevant — a single UDP recv returns exactly one datagram regardless.

### How to avoid
- Read rcon replies in a LOOP: full timeout on the first datagram, then a short
  idle timeout (~250ms) on subsequent ones; when the idle timeout elapses with no
  data, the reply is complete (normal end, not an error). Strip the 4-byte OOB
  prefix and a leading `print\n` from EACH packet, then concatenate. Cap the loop
  (packet count / total bytes) as a backstop. These OOB print packets carry no
  sequence numbers, so arrival order is all you get (fine on a LAN). TCP rcon is a
  stream and needs the same loop-until-EOF/idle treatment.

### Sources
- qctrl: crates/rcon/src/lib.rs (`execute_udp`, `execute_tcp`)
- qctrl: vendor/yquake2/src/common/clientserver.c (`Com_VPrintf` redirect flush)
- qctrl: vendor/yquake2/src/server/sv_send.c (`SV_FlushRedirect`)

# Empty sv_maplist kills a Quake 2 server on `maps/.bsp`

When a deathmatch match ends (fraglimit/timelimit), the game's `EndDMLevel` picks the
next map from the **`sv_maplist`** cvar. If `sv_maplist` is empty, some game builds
resolve the next map to the *empty string* and issue `gamemap ""` → `ERROR: Couldn't
load maps/.bsp` → `ShutdownGame`. The server process dies with no rcon `map` line in
the console, so it looks like a spontaneous crash rather than a command someone sent.
Two things hide this for months: `sv_maplist` is normally empty and *nothing ever ends
a match* on an all-bot server (leaving intermission needs a client to hold a button),
so the fatal path is only reached once bots learn to press ATTACK at intermission.

How to avoid: treat `sv_maplist` as **server state that resets on every server
restart**, not as config you push once. A controller that pushes it at its own startup
(or on rotation edits) silently loses the protection the moment the *game server*
restarts. Poll the cvar (`rcon sv_maplist` → `"sv_maplist" is "q2dm1 q2dm2"`) on an
interval and re-push on drift — check-then-push, never blind-push, because rcon flood
protection answers `Bad rcon_password` when throttled. Never push an empty list (that
re-arms the crash), and never push on an unparseable reply (that hammers a server
that's already unhappy). Independently, reject `map`/`gamemap` with a blank argument
at every layer that can emit rcon, and never build an implicit `map $current` restart
from a status field that may be empty/unknown.

## Sources
- qctrl: `crates/api/src/main.rs` (`spawn_sv_maplist_watchdog`, `validate_rcon_command`)
- qctrl: `frontend/src/lib/applyLogic.ts` (`buildApplyCommands`)
- qbots: Plan 64 (bots pressing ATTACK at intermission surfaced the latent crash)

# Quake 2 intermission never ends by itself — rotation cannot live in a client

When a Q2 deathmatch match ends, `CheckDMRules` → `EndDMLevel` → `BeginIntermission` parks
the server, and `CheckDMRules` then returns at the top forever after. The **only** writer of
`level.exitintermission` reachable in deathmatch is `ClientThink`
(`yquake2 game/player/client.c:2122`): it needs a *connected client* to send `BUTTON_ANY`
(attack/use) at least 5 s in. There is no timeout, no max intermission length, no
"empty server → advance" case. `G_RunFrame` calls `ExitLevel` only if that flag is set. So an
empty or idle server hits the timelimit and **sits in intermission indefinitely**.

The trap is believing `sv_maplist` is a fallback. It is not. It only decides *which* map the
changelevel points at (`g_main.c:236-279`); it does nothing to make the exit **fire**. qctrl
carried a comment claiming "losing the race is benign, sv_maplist is kept in sync, so the
server's own rotation lands on the right map" — the premise was false, and every code path
that "deferred to the server's rotation" was really deferring to a deadlock.

This hides for a long time because *something* usually presses a button: a human player, or a
bot taught to press ATTACK at intermission. It only surfaces on an unattended server.

How to avoid: an external controller must **own** map advancement, and own it somewhere that
runs headless. qctrl originally drove rotation from a React hook, so rotation silently became
a property of having a browser tab open — the map would not advance until someone loaded the
frontend, which looked like "the UI pokes the server awake." Put the timer in the daemon,
trigger a few seconds *before* the timelimit so intermission never starts, and keep a rescue
trigger for the case where you cannot know the elapsed time (e.g. the controller restarted
mid-map) — otherwise that state has no way out.

## Sources
- qctrl: `crates/api/src/rotator.rs` (`decide`, `select_next`), `crates/api/src/main.rs` (`spawn_rotator`)
- qctrl: vendor/yquake2 `src/game/g_main.c` (`CheckDMRules`, `EndDMLevel`, `ExitLevel`)
- qctrl: vendor/yquake2 `src/game/player/client.c` (`ClientThink`, the BUTTON_ANY gate)
- qbots: Plan 64 (bots pressing ATTACK at intermission — the other way to unstick it)

# Silent SIGSEGV toolchains: per-env node_modules, node 24 + vite, vitest/vite major skew

A frontend where `npm run test`, `npm run build` and even `npm ci` all exit 139 with
**zero output** looks like one catastrophic break; it was three unrelated ones (qctrl):

1. **node 24 + vite**: Gentoo's system node 24.14 segfaults inside vite. Nothing is
   printed because piped stdout is block-buffered and the buffer dies with the process.
   Node 22 runs the same tree clean. Suspect the node binary before the project when a
   crash produces no output at all — and re-run without a pipe to recover the message.
2. **Per-env `node_modules.<env>` trees** (a symlink swap so host and container never
   share native binaries) defeat every tool's built-in `node_modules` ignore, which
   matches the *name*, not the symlink target. vitest then collects dependency test
   files (zod ships 185 locale suites); eslint lints the dependency tree and dies on the
   first package with its own config. Fix: explicit `node_modules*` ignores in
   `vitest.config.ts` and `eslint.config.js`.
3. **vitest/vite major skew**: vitest 2 supports vite ≤5. Under vite 8 it starts, finds
   the files, and reports "No test suite found" for every one — a green-looking runner
   that tests nothing. Keep the vitest major peer-matched to vite.

Lesson: a test suite nobody can run rots. All three broke while the repo looked healthy
because CI for the frontend was never actually executing.

## Sources
- qctrl: `justfile` (`_nm`, `fe-node-check`), `frontend/vitest.config.ts`, `frontend/eslint.config.js`

# Quake 2 rcon strips quotes: a value with spaces can never be `set` remotely

`rcon set sv_maplist "q2dm1 q2dm2"` looks like it works and silently does nothing. The
server's `SVC_RemoteCommand` (`sv_conless.c`) tokenizes the incoming packet, then
rebuilds the command line by concatenating `Cmd_Argv(i)` for i>=2 separated by spaces.
The tokenizer has already consumed the quotes, so `Cmd_ExecuteString` receives
`set sv_maplist q2dm1 q2dm2` — N arguments instead of 2. `set` prints
`usage: set <variable> <value> [u / s]` into the rcon reply and the cvar is never
assigned. Re-quoting, escaping (`\"`), or single quotes do not help: quoting is lost at
tokenization, before any code you can influence. This is not a client bug — nothing sent
over rcon can carry a space inside one cvar value.

How to avoid: never send a multi-word value over rcon. Find a separator the *consumer*
accepts that is not a space. For `sv_maplist`, `EndDMLevel` tokenizes on `" ,\n\r"`
(`g_main.c`), so comma-joined and unquoted works: `set sv_maplist q2dm1,q2dm2,q2dm3`.
Always verify a `set` by reading the cvar back (`rcon sv_maplist` →
`"sv_maplist" is "…"`) rather than trusting an empty/OK-looking reply — this bug hid for
months behind a command that *appeared* to succeed, and it was the actual cause of the
empty-`sv_maplist` `maps/.bsp` server crash.

## Sources
- qctrl: `crates/api/src/main.rs` (`sv_maplist_value`, `push_sv_maplist`)
- yquake2: `src/server/sv_conless.c` (`SVC_RemoteCommand`), `src/game/g_main.c` (`EndDMLevel`)

# Never run a qbots fleet at a Q2 server with no qctrl running

A qbots fleet plus an empty `sv_maplist` kills the server. The bots are the trigger, but
the missing `sv_maplist` is the loaded gun, and **qctrl is the only thing that keeps it
unloaded** — it pushes `sv_maplist` on startup and re-pushes it every 60s (a server
restart wipes the cvar). Run the fleet with qctrl down and you re-arm the crash.

The chain: Q2 never leaves intermission on its own — `ClientThink` requires a *connected
client* to press `BUTTON_ANY` (`client.c:2122`), so an unattended server parks in
intermission forever once the timelimit hits. qbots bots, on joining, see that and
deliberately "press ATTACK to advance the level" — which is the *right* behaviour, and
exactly what the server was waiting for. The changelevel then fires, reads an empty
`sv_maplist`, resolves the next map to `""`, and `Com_Error`s:

    Server crashed: Couldn't load maps/.bsp

Note the failure mode: `q2ded` does **not** exit. `Com_Error(ERR_DROP)` drops it to a
no-map state, so the process is still alive and the container still "up" while the server
answers nothing on UDP. It looks like a network problem; it is a dead server. Revive with
a single rcon `map <name>` — no restart needed.

How to avoid: **start qctrl first, always** — before any fleet, in dev as in prod. Verify
`sv_maplist` is actually populated (read it back; see the rcon-spaces pitfall above — a
`set` that *appears* to succeed may have silently done nothing). If you must run bots at a
bare server, push a maplist by hand first. Corollary for anyone testing the map clock: a
frozen `serverframe` with a climbing `age_ms` on the beacon means the server is parked in
intermission, not that the beacon is broken.

Discovered twice: once in production (2026-07-12, qctrl Plan 12's founding incident) and
again during Plan 13/66 verification (2026-07-13) by doing precisely what Plan 12 forbids.

## Sources
- qctrl: `crates/api/src/main.rs` (`spawn_sv_maplist_sync`, `spawn_sv_maplist_watchdog`), `crates/api/src/rotator.rs` (module doc — why intermission needs an owner)
- qbots: `crates/brain` (intermission → press ATTACK), `context/plans/64_map_change_survival_tracker.md` (forensics)
- yquake2: `src/game/g_main.c` (`EndDMLevel`), `src/game/player/client.c:2122` (`ClientThink` / `exitintermission`), `src/server/sv_init.c` (`SV_SpawnServer` → `Com_Error` on a missing BSP)

# nginx package-cache: regex-location proxy_pass drops the URI rewrite, and temp dirs need one-level paths
When building an nginx reverse cache for OS package repos (slop/cache, `pkgcache`), two
non-obvious things bit during verification.

(1) **Nested regex `location` + `proxy_pass` path remap.** To give immutable `.rpm`/`.deb`
files a long TTL while metadata stays short, you nest a `location ~* \.rpm$ {}` inside a
prefix `location /fedora/ {}`. But a *regex* location's `proxy_pass` MUST NOT carry a URI
part — nginx forwards the (normalized) request URI verbatim. So the parent's tidy
`proxy_pass https://host/pub/fedora/;` remap is LOST in the nested block: `/fedora/x.rpm`
would hit the upstream as `/fedora/x.rpm` (404) instead of `/pub/fedora/x.rpm`. Fix: re-apply
the remap explicitly with `rewrite ^/fedora/(.*)$ /pub/fedora/$1 break;` then
`proxy_pass https://host;` (no URI). Prefix locations *can* use a URI in proxy_pass; regex
ones cannot. Always test the nested branch, not just the prefix.

(2) **`*_temp_path`/cache dirs are created with a single `mkdir()`.** nginx does not create
intermediate parents. On a fresh bind-mounted empty volume, `proxy_temp_path
/var/cache/nginx/tmp/proxy` fails with `mkdir() ... (2: No such file or directory)` because
`tmp/` doesn't exist. Keep every temp/cache path exactly one level under the volume root
(`/var/cache/nginx/proxy_temp`, matching the stock image layout) so the single mkdir works.

Bonus (env, not code): rootless podman here failed to even pull the base image — short-name
resolution off (`no unqualified-search registries`) and a subuid/graphdriver mismatch
(`potentially insufficient UIDs or GIDs`, vfs-vs-overlay). Fully-qualify FROM with
`docker.io/...`; the subuid issue is a host setup fix (`podman system migrate`, check
`/etc/subuid`). Verified the image with docker instead.

## Sources
- slop/cache: `conf.d/pkgcache.conf` (nested `.rpm`/`.deb` regex locations + `rewrite`), `nginx.conf` (`*_temp_path`), `Dockerfile` (`FROM docker.io/...`)

---

# `set -e` plus `[[ cond ]] && assign` silently truncates a script

A statement of the form `[[ cond ]] && var=value` returns non-zero when the condition is
false. Under `set -e` that non-zero status is the exit status of a top-level statement, so
**the shell exits right there** — no error, no message, exit code 0 from the trap or 1
from the shell. The idiom is safe inside an `if` condition or as a non-final element of a
`&&`/`||` chain, which is why it looks fine in review and in most of the places it is
copied from.

Concretely, in a GPU sampler:

```bash
set -euo pipefail
[[ -r "$hwmon/energy1_input" ]] && energy_file="$hwmon/energy1_input"   # file absent
emit "$header"        # never runs; script already exited
```

The observed symptom was a capture script that produced an **empty file and exited 0**.
For measurement tooling this is the worst possible failure: it is indistinguishable from
"the thing being measured was idle", and it silently truncates every run that takes an
optional-probe branch. The bug only fires on machines where the optional file is missing,
so it survives testing on the developer's box.

## Fix / How to avoid

Use an explicit `if` block for conditional assignment, or terminate the statement with
`|| true` when the guard is genuinely optional:

```bash
if [[ -r "$hwmon/energy1_input" ]]; then energy_file="$hwmon/energy1_input"; fi
```

Rule of thumb: under `set -e`, never let `[[ ... ]] &&` be the *last* command of a
statement. Grep for `^\s*\[\[.*\]\] &&` before shipping any script that writes data a
human will later trust.

## Sources
- slop/gpu: `scripts/gpu-survey.sh`, `scripts/record.sh`, `scripts/launch-game-debug.sh`

# bash `set -e` silently kills loops that use `((i++))` as a counter

Under `set -euo pipefail`, a bare arithmetic command `((i++))` used as a statement is a
*command* whose exit status is derived from the arithmetic *result*, not from success. Post-
increment evaluates to the OLD value, so the very first `((ok++))` when `ok=0` evaluates to
0 → exit status 1 → `set -e` terminates the script on the spot. The failure is invisible: no
error message, no non-zero exit at top level if it happens inside a function whose output is
already partially printed. Symptom in the affinity script: the per-user summary line and the
entire second user's section just never printed, while the header line did. It looks like a
logic/data bug (`the second user has no processes`), not a shell bug.

Avoid: never use `((i++))`/`((i--))` as a standalone statement in a `set -e` script. Use
`i=$((i + 1))`, or `((i++)) || true`, or pre-increment `((++i))` (which evaluates to the new
value — still lands on this trap whenever the new value is 0, e.g. counting up from -1).
Note the related-but-safe case: in an `a && b` list where `a` fails, `set -e` does NOT exit,
because only the command following the *final* `&&`/`||` is checked. So `((VERBOSE)) &&
printf ...` and `[[ -z $x ]] && x=foo` are fine as-is; the bare `((...))` is the hazard.
Same trap applies to `let i++` and to `((flag))` used as a standalone truth test.

## Sources
- slop/affinity: `bin/game-affinity` (`apply_to_user` ok/fail counters, `cmd_watch` pass counter)

# X server on a multi-seat box is root-owned, so uid matching never finds it
Pinning "everything belonging to user U" by scanning `/proc/*/status` for `Uid:` silently
misses the single most latency-critical process in an X session: the X server itself. On a
multi-seat box (LightDM/GDM driving two seats) X is spawned by the display manager as
**root** — `/usr/bin/X :1 -layout seat1 -seat seat1 -auth /var/run/lightdm/root/:1`. A uid
filter drops it, so the game tree and the audio stack get pinned to a CCD while the X server
that composites their frames stays free to float across every CPU on the machine. The bug is
invisible because the pin *appears* to succeed: process counts look plausible and every pid
reported is genuinely the right user's. Wayland hides the problem — the compositor (labwc,
sway) and Xwayland do run as the user, so the same code is correct for one seat and wrong
for the other.

Avoid: attribute display-server processes by **seat**, not uid. `loginctl show-user U -p
Sessions --value`, then `loginctl show-session N -p Seat/-p Display --value`, gives the
user's seat (`seat1`) and display (`:1`); match any `X`/`Xorg` process whose cmdline contains
`-seat <seat>`, `-layout <seat>`, or the display as a standalone argument. The same
seat→hardware path answers "which GPU is this user's": `/sys/class/drm/cardN` carries
`ID_SEAT` in udev properties (absent means seat0), and `cardN/device/irq` +
`device/msi_irqs` give the interrupt line to steer onto that user's CCD.

## Sources
- slop/affinity: `bin/game-affinity` (`desktop_for_user`, `seats_of_user`, `irqs_for_user`)

# scaling_cur_freq can be a cached constant, not a measurement
Diagnosing "the CPU is not maxed but frames are slow" on a Ryzen 5950X, every userspace
source of CPU clock agreed on 3368.5 MHz — `/sys/.../cpufreq/scaling_cur_freq`, `lscpu -e=MHZ`,
and the monitors that read them. The number was identical with the core idle and with the
core pinned at 100% by a busy loop, which is the tell: a real aperf/mperf-derived reading
moves. Under `amd_pstate` (and other drivers that do not implement `cpuinfo_cur_freq`),
`scaling_cur_freq` can report the policy's cached target rather than the achieved clock, so
it neither proves boost is working nor proves it is not. Two separate attempts to confirm a
boost cap from sysfs were inconclusive because of this.

The mirror-image trap on the same box: `/proc/cpuinfo` "cpu MHz" *is* aperf/mperf-derived,
but an **idle** core has no recent delta and falls back to reporting exactly the policy
maximum. So idle cores read 5086.181 MHz (precisely `cpuinfo_max_freq`) and busy cores read
the true 3368 MHz — which looks exactly like working boost that "drops under load", and is
the reverse of the truth. Two readings that disagree in opposite directions, both wrong.

Avoid: measure clock from the cycle counter, which cannot be cached.
`perf stat -e cycles -- taskset -c 3 bash -c 'end=$((SECONDS+3)); while [ $SECONDS -lt $end ]; do :; done'`
then divide the cycle count by the elapsed seconds perf prints — that gave 9.695e9 / 2.8817 s
= 3.364 GHz against a rated 5086 MHz boost, proving the cap. `cycles:u` counts without root
at the common `perf_event_paranoid=2`. `turbostat` is the other correct tool. Corollary: a
value that is suspiciously round, or identical across all cores, or unchanged between idle
and load, should be treated as a constant until a second independent method agrees.

## Sources
- slop/affinity: `perf_notes.md` (Finding 1, Finding 2)

# EINVAL from a cpufreq sysfs write does not mean the value was invalid
Writing `1` to `/sys/devices/system/cpu/cpufreq/boost` as root returned `Invalid argument`,
which reads like a rejected value and sent the investigation looking for a syntax or
permissions problem. It is neither. `store_boost()` in `drivers/cpufreq/cpufreq.c` validates
the value, then calls `cpufreq_boost_trigger_state()` and collapses **any** driver-side
failure into `-EINVAL` -- so a driver returning `-ENOTSUPP` surfaces to userspace as
"Invalid argument". On this box `amd_pstate` refused because `X86_FEATURE_CPB` (the `cpb`
flag in `/proc/cpuinfo`) was absent: firmware had masked Core Performance Boost, so no
sysfs, module-parameter or governor change could ever have worked.

Avoid: when a cpufreq (or similar) sysfs write returns EINVAL, read `dmesg` immediately --
the kernel logs the real reason next to the failure, here
`cpufreq: store_boost: Cannot enable BOOST!`. Then check whether the CPU even advertises the
capability (`grep -x cpb` over `/proc/cpuinfo` flags for AMD boost) before hunting for a
userspace culprit. Generally: a sysfs errno is whatever the last layer chose to return, not
a description of your input; treat `dmesg` as the actual error message. Boost gated this way
is fixable only in BIOS (Core Performance Boost = Auto), or at runtime by clearing
`MSR_K7_HWCR` (0xC0010015) bit 25 CPBDIS.

## Sources
- slop/affinity: `perf_notes.md` (Finding 1)

# Restarting one lightdm seat can destroy that seat permanently
LightDM drives every seat from one daemon, so `systemctl restart lightdm` restarts all of
them. Cycling a single seat means terminating its session or its X server and letting lightdm
rebuild it — but every such path funnels into `seat_switch_to_greeter`, which calls
`seat_stop` when it cannot bring a greeter up (seat.c:508-509, 855-856). `find_greeter_session`
skips stopping sessions (seat.c:532), so lightdm always builds a *new* display server; there is
no falling back to the greeter that was already on screen. If that new X fails to start, the
Seat object is destroyed. For any seat other than seat0 nothing recreates it at runtime:
`AddSeat` returns "AddSeat is deprecated" (display-manager-service.c:229) and the logind re-add
path only exists when `logind-check-graphical=true` (lightdm.c:511-512). Recovery is a full
daemon restart, which drops the very seat you were protecting. `loginctl terminate-seat` is the
worst offender — it kills the greeter session directly (seat.c:841-848). Note also that seat0
alone gets `exit-on-failure=true` hardcoded *after* config load (lightdm.c:418-419), so it
cannot be disabled from lightdm.conf, and a seat0 stop exits the whole daemon.

Avoid: before ever cycling a seat, put `type=local;local` in its lightdm.conf section — on seat
removal lightdm skips the first type and rebuilds from the next (lightdm.c:224-256), buying one
automatic recovery. It is read at `add_login1_seat` time, so it only arms after the next daemon
start. Prefer terminating a live user session over restarting a seat already at the greeter,
which risks everything for nothing. Never address a seat via `dm-tool` or a D-Bus path:
`/org/freedesktop/DisplayManager/SeatN` is a bare registration counter
(display-manager-service.c:481) that is unrelated to logind names and increments on every seat
restart — on cosmo, DBus `Seat0` was logind `seat1`. Use `loginctl` seat names only. Reading
the control flow shows which paths dodge a guard; it does not show whether the replacement
display server will actually start.

## Sources
- slop/scripts: `lightdm-restart-seat`
- slop/context: `lightdm.md`

# Upstream sources do not tell you what the installed binary does
Diagnosing lightdm on a Gentoo box, `desired-display-number` in `/etc/lightdm/lightdm.conf`
was checked against the upstream 1.32.0 tarball and against the distro's patch tarball, found
in neither, and written off as an inert leftover key — in a commit, in a context file, and in
a recommendation to delete it from the config. It was in fact a locally written feature that
pins a seat's X display number, very much live in the running daemon. It was invisible to that
search because `eapply_user` patches live in `/etc/portage/patches/<cat>/<pkg>/` and never
appear in the ebuild, in `SRC_URI`, in the distfiles, or in the `PATCHES` array recorded in
`/var/db/pkg/.../environment.bz2`. Searching every overlay for the string also finds nothing.
The same blind spot applies to any distro's user-patch mechanism, to `-r` revision bumps whose
ebuild has since been re-synced away, and to hand-built installs.

Avoid: when a config key, symbol, or flag looks unrecognised, ask the installed artifact rather
than the source you happen to have. `strings -a /usr/sbin/foo | grep <key>` settles it in one
command; `nm -D`, `--help`, and the package manager's own file list are the same move. Check
`/etc/portage/patches/` (Gentoo), `debian/patches/` and `dpkg -l` versions, or the RPM spec
before concluding a feature is absent. "Not in upstream" and "not in this binary" are different
claims, and only the second one licenses telling someone their config line does nothing.

## Sources
- slop/context: `lightdm.md` (desired-display-number)

# A state enum named after a protocol state that does not mean the same thing

qbots' `ConnState::Active` reads like Quake 2's `ca_active` ("spawned, receiving frames").
It is not: the crate enters `Active` on `svc_serverdata`, which is the START of the reliable
configstring/baseline pull, while the server still holds the client in `cs_connected` and
drives it forward with reliable `cmd configstrings N K` / `cmd baselines` / `precache`
stufftexts that each need a prompt reply. A netchan change gated on `state != Active`, meant
to match the reference client's "no cmd-less packets once active", silenced every one of
those replies for up to a tick. A single bot tolerated it (a 50 s live run looked fine); a
24-bot join did not — the server dropped 10 of 24 mid-handshake with a bare `svc_disconnect`
(the `SV_DropClient` shape for an unspawned client: no `ClientDisconnect` print). The commit
had a green suite, a vendor-cited rationale, and a correct A/B against the netchan source; it
was wrong about what the enum variant meant in *this* codebase.

Avoid: before gating behaviour on a state name, read where the variant is assigned, not what
it is called — one `grep "State::Active ="` would have shown `svc_serverdata`. When a
protocol has a well-known state machine, either mirror its names exactly or name the local
states after the local event (`ServerDataSeen`, `Spawned`). The fix was a predicate on the
actual observable (`state == Active && frame.is_some()`, the reference's first-parsed-frame
line) rather than the name. And: verify netchan/handshake changes with a many-client join,
not one client — starvation of a request/reply pull only shows under contention.

## Sources
- qbots: crates/client/src/conn.rs (`Conn::spawned`, `on_recv`; commits 8e4546290 → 8d8645f24)
- qbots: context/pitfalls.md ("`clc_move` sent the same usercmd 3×", corollary)

# CMake enable_language(CUDA) self-seeds CMAKE_CUDA_ARCHITECTURES before your default runs

`enable_language(CUDA)` initializes `CMAKE_CUDA_ARCHITECTURES` itself (historically to `75`)
the first time it runs, and a project default written as `if(NOT DEFINED ...)` AFTER the
`enable_language()` call sees an already-defined variable and never fires. The cache value then
seeds every `nvcc` invocation, kernels compile for `sm_75`, and a Blackwell (sm_120) / newer
driver rejects the artifact with "PTX compiled with an unsupported toolchain" — a toolchain-sounding
error that is really an architecture mismatch. Any `--arch=` passthrough or preset-style override
is silently dead for the same reason if the override check sits after the language call.

Avoidance: set `CMAKE_CUDA_ARCHITECTURES` (and any honor-the-user-cache `if(NOT CACHE ...)`
logic) BEFORE `enable_language(CUDA)`; treat user cache entries as authoritative over project
defaults. Diagnostic: CMake configure banner prints the arch line — verify it there, not in
build logs after a failure.
## Sources
- kin (vendor/azu): CMakeLists.txt — arch default must precede enable_language; fixed 89;120 for RTX 4060 + Blackwell PRO 6000

# Lazy `if(!ptr)` GPU scratch buffers survive a resize and go stale

Point-cloud/raycast scratch buffers allocated lazily on first use (`if (!d_pc_is_valid_) cudaMalloc(n)`)
are correct until the owning object changes size. If a TSDF volume grows (setParams / resolution
bump) the buffers keep the OLD element count while consumers assume the new one — silent
out-of-bounds reads or garbage frames, not a clean error. The lazy pattern has no place to put
the invalidation, because "first use" already happened.

Avoidance: any setter that changes capacity must call a guarded, idempotent `freeGPU()`-style
teardown BEFORE the next lazy reallocation; free every scratch pointer, not just the first
(`d_pc_is_valid_/offsets_/out_points_/out_colors_` in Azu), since partial frees leave the same
class of stale buffer. Mirror the teardown in the HIP twin. Test by growing the volume mid-session,
not just boot-size.
## Sources
- kin (vendor/azu): src/tsdf/TSDFVolume.cpp setParams, TSDFVolume_cuda.cu/_hip.hip freeGPU

# Kinect v1 free-hand room scan: default TSDF box smaller than the room ⇒ "Tracking lost" loop

A KinFusion-style tracker fails as a *loop*, not a crash: camera walks outside the fixed TSDF
volume (default 256³ @ 10 mm = 2.56 m box), live points stop finding model correspondences, and
the UI repeats "Tracking lost!" while frames keep integrating whenever the user swings back.
Signature in logs: valid_live huge, valid_model collapsed (observed 304190 vs 11744) — the camera
sees the room but not the *model*. Nothing is broken; the capture volume is a shoebox.

Avoidance: match volume to the room before scanning (Room preset, or resolution 256 with voxel
12-15 mm), raise ICP distance threshold toward 0.15, and scan in arcs/traverses — rotation about
the camera position moves the FOV out of a small box even without translation. GUI aids that make
this self-evident: overlap % gauge (valid_model/valid_live, note it tops out ~85-92 % at identity
pose) and a wireframe cage of the volume, amber when the pose leaves it (inset the exit test:
identity start pose sits exactly on a default origin face).
## Sources
- kin (vendor/azu): TSDFVolume.h defaults, ICPTracker CUDA/CPU counters, PipelineController lost-logging

# libfreenect timestamps are 60 MHz ticks, not µs ⇒ RGB/depth pairing gate silently starves pipeline

`freenect_*_cb(dev, data, uint32_t timestamp)` timestamps are raw Kinect v1 hardware counter
ticks at **60 MHz** (OpenNI2-FreenectDriver VideoStream.hpp divides by 60000 for ms; measured
~2,002,155 ticks per 33.37 ms depth frame). uint32 wraps every **71.58 s**. Azu divided by 1000
("µs → ms"), so a 50 ms pairing window was really **0.83 ms**. Depth (29.968 fps) and RGB
(29.996 fps) run on slightly different periods, so their phase slides ~1,868 ticks/frame: pairs
succeed in ~54-frame (1.8 s) bursts, then nothing for ~34 s. Symptom: "captures a few frames then
hangs" while UI still reports Running and capture fps shows its last stale value. Real 80 s trace:
162/2400 paired; under libfakenect: 0 in 12 s. Base code had the same wrong divisor — harmless
until a strict gate was added on top.

Avoidance: define one tick-unit constant (`kFreenectTicksPerMs = 60000`), compute deltas as wrap-safe
`int32_t(a - b)`, pair each depth frame with the *nearest* RGB within ~half a frame (17 ms), and
publish depth-only instead of stalling when none fits. Test with a recorded real timestamp trace
and a tick-sweep (phase offsets across a full slip cycle), plus a fakenect end-to-end smoke test
asserting paired fps > N. Show capture fps decaying to 0 when frames stop.
## Sources
- kin (vendor/azu): src/sensor/KinectSensor.cpp onDepth/onRgb, KinectSensor.h pairing gate (commit c311c69)

# Kinect v1 depth intrinsics: 525 px is the RGB camera; IR depth is ~576 px

Unregistered `FREENECT_DEPTH_11BIT` depth is in IR-camera geometry. The common
fx = fy = 525 value belongs to the RGB camera. The unit's own IR focal length comes
from its factory registration: `freenect_copy_registration(dev).zero_plane_info`,
f = reference_distance / (2 * reference_pixel_size) = 120 / (2 * 0.1042) = 575.8 px
(fakenect-record saves the same block as device.json). The 9.7% error cancels under
translation but not rotation: tracking sees ~9% less turn than happened, and a full
spin stops matching itself when it comes back around (synthetic spin: 327 deg tracked,
lost, 34 deg end error; with 575.8: 359.9 deg, 0.3 deg).

Avoidance: read intrinsics from the device registration at init, carry one intrinsics
value through back-projection, ICP projection, integration and raycast, and keep the
525 fallback only for data without calibration. libfakenect serves
freenect_copy_registration from device.json, so replays get it too.
## Sources
- kin (vendor/azu): include/sensor/CameraIntrinsics.h, KinectSensor.cpp init

# Grading ICP frames by inliers / ALL live points starves the model when turning

A tracking gate like "Good if inliers / valid_live >= 0.3" looks harmless, but when
valid_live counts every live point in front of the model camera, points that land
outside the old model image, outside the volume, or on unobserved space all count
against the frame. Turning toward new geometry lowers the ratio, the frame is graded
Poor and not integrated, the model never grows, the ratio keeps falling: Lost after
~15 degrees of slow rotation. KinectFusion integrates every tracked frame for exactly
this reason.

Avoidance: measure fit over correspondences (inliers / points that hit a valid model
pixel) plus RMS and an absolute inlier floor; never let "unseen geometry in view" veto
integration.
## Sources
- kin (vendor/azu): include/tracking/TrackingPolicy.h (policy v2)

# Point-to-plane ICP facing one wall: constant-velocity prediction carries null-space noise

Facing a single plane, sliding along it and rolling about its normal are unobservable:
the solve returns the initial guess plus noise there, Tikhonov damping keeps it near the
guess, and a constant-velocity motion model turns that noise into velocity and
re-applies it every frame. A synthetic in-place 360 degree spin drifted 0.35 m while
graded Good the whole time.

Avoidance: keep the final undamped J^T W J, eigen-decompose it, and hold motion along
eigen-directions below ~5e-3 of the largest eigenvalue at the previous pose
(degeneracy-aware update; 2e-2 started discarding real yaw). Drift fell to ~3 mm.
## Sources
- kin (vendor/azu): TrackingPolicy.h keepObservableMotion, ICPResult::information

# Kinect v1 depth is rolling shutter: fast handheld sweeps bend every frame

The Kinect v1 IR sensor reads its rows top to bottom over most of the ~33 ms frame, so
each depth row is seen from a slightly different pose. A 90 deg/s pitch sweep stretches
or squashes the frame vertically by ~1.5 deg top to bottom; a pan shears it. Rigid ICP
cannot absorb that: on a handheld photosphere-style room take the point-to-plane RMS
rose from ~4 mm (still) to 10-20 mm, correlated with pitch rate (r = 0.58), not depth.
Above the RMS gate the frames grade Poor, integration stops, the model stops growing,
and the next flat wall ends in Lost. Looks like "noise at range", is not.

Avoidance: unwarp each row by the constant-velocity step before ICP (pose at row time =
mid-row pose * exp(s * log(step)), s = (v/(H-1) - 0.5) * readout / period), resampling
by inverse mapping so there are no splat holes. Readout ~20-33 ms fits (mean RMS in the
worst segment 15.1 -> 8.7 mm); a negative readout makes it worse, which confirms the
direction. Also tell the operator to sweep slower (<= ~30 deg/s).
## Sources
- kin (vendor/azu): include/sensor/RollingShutter.h (AZU_RS_READOUT_MS), cap_001

# Nondeterministic GPU ICP reductions make single-run A/B comparisons meaningless near a failure

Float atomics in a GPU reduction sum in a different order every run. Far from a failure
the results agree to ~1e-6; near the edge (the frame where tracking is lost) the tiny
differences pick different basins, and after the first loss relocalization amplifies
them. Four identical replays of one recording gave 397-908 Good frames and 89-359 deg
of tracked yaw. An A/B "win" from one run each is noise.

Avoidance: compare only up to the first loss (it was stable: same frame every run), or
run each variant several times and compare distributions; make the reduction
deterministic (fixed-order block sums) before trusting end-to-end metrics.
## Sources
- kin (vendor/azu): tools/azu_replay.cpp runs on cap_001; ICPTracker_cuda.cu reduction

# Constant-velocity prediction replays a relocalization jump

A predictor of the form `predicted = current * (last^-1 * current)` treats whatever
happened between the last two frames as motion. After a relocalization, `current` is
the re-acquired pose and `last` is the stale pose from before the loss, so the next
frame is predicted one whole jump further (cap_001: 16.6 deg / 116 mm). ICP from that
guess fails or lands wrong, the retry from the previous pose may also miss, and three
failures later the track is lost again, right after a correct recovery. A separate
velocity model in the same code already reset on relocalization; the default path did
not.

Avoidance: on any pose that is not motion (re-acquisition, reset, manual re-anchor),
set the motion source to the new pose so the next prediction is zero motion. Test it
through the prediction seam, not end to end: end-to-end runs on real data were too
noisy to show it.
## Sources
- kin (vendor/azu): src/app/PipelineController.cpp (last_pose_ on re-acquisition), tests/pipeline_gravity_contract.cpp (D)

# A re-acquired pose can have the orientation right and the position wrong

Relocalization that scores hypotheses by ICP fit can lock onto a basin with the
right rotation (and a clean fit: 7.7 mm RMS, tilt 5 deg vs gravity) but a position
~26 cm off. Integrated at once, it writes a ghost into the model; the next frame's ICP
then wants to move 26 cm, trips the per-frame motion gate, and the track is lost
again. Gravity cannot see position or yaw.

Avoidance: probation. Integrate nothing until N (3) consecutive Good frames have
tracked from the re-acquired pose; any failure goes straight back to relocalizing from
the pre-re-acquisition pose. A self-consistent wrong basin can still pass (synthetic:
21.6 cm off at 15 deg from the last good pose), so the relocalizer's reach matters too.
## Sources
- kin (vendor/azu): PipelineController.cpp (reacquire_probation_), big-fix-two T4.5

# Labelling relocalization results against pre-loss frames only mislabels correct ones

To judge whether a re-acquisition was right, I compared each re-acquired frame's RGB
with the most similar-posed frame tracked before the FIRST loss. After a correct
re-acquisition, tracking maps new areas (a closet, shelves); a later re-acquisition of
those areas then has no pre-loss twin, looks "wrong", and I declared 7 of 8 wrong on a
run that was mostly right. Worse, the verdict drove a threshold change.

Avoidance: label against the most similar pose among ALL frames tracked Good before that
re-acquisition's loss, and state the pose distance next to the pair. Remember chains: a
frame tracked after a wrong re-acquisition "confirms" the wrong basin. Check the final
mesh too (duplicated/rotated structure is the tell), not only per-event labels.
## Sources
- kin (vendor/azu): cap_001 relocalization analysis; tracking/Relocalizer.h max_visited_*

# Depth-only relocalization in a plain room: fit and consistency do not tell right from wrong

On a handheld photosphere take (cap_001), wrong re-acquisitions (closet doors matched to
the wall under a monitor, bare ceiling corners to other corners) passed ICP at Good fit
and render-and-compare at 0.6-0.96 consistency, as high as right ones. The information
matrix constraint ratio, colour NCC against the model, and rotation/translation from the
last good pose all overlapped too. Accepting them wrote rooms in twice (a second window
and desk rotated 90 deg).

Avoidance: require the re-acquired pose to be near one the camera actually tracked
(within 0.2 m and 45 deg of a stored Good pose): right ones sat 4-15 cm from such a pose,
wrong ones 26-142 cm. Prefer waiting over guessing; verify with the mesh.
## Sources
- kin (vendor/azu): tracking/Relocalizer.h (max_visited_distance_m / max_visited_angle_deg)

# OpenMP `schedule(dynamic)` + per-thread float sums: the CPU path is nondeterministic too

The CPU ICP summed its 6x6 normal equations into per-thread accumulators under
`schedule(dynamic, 32)`, then combined them in thread order. Dynamic scheduling hands rows
out first-come, so which rows land in which partial sum changes run to run, and float
addition is not associative. On a marginal frame the difference flipped a Good/Poor
grade: a synthetic room-spin test passed alone and in most gate runs, and failed in
others (lost at 121-123 deg, 77/249 Good). It looked like a regression from unrelated
changes and cost several gate cycles to tell apart from one.

Avoidance: for float reductions use `schedule(static)` (same rows per thread every run)
and combine partials in a fixed order; the result is then bit-reproducible for a given
thread count (results still differ ACROSS thread counts, so pin `OMP_NUM_THREADS` in
tests). Before blaming a change for a flaky failure, run the test 3x at the same thread
count and diff the numbers: identical = deterministic, investigate; different = fix the
nondeterminism first. After the fix: 3/3 identical runs, 249/249 Good.
## Sources
- kin (vendor/azu): src/tracking/ICPTracker.cpp (track level accumulation), tests pipeline_spin_room_preset

# Parallel OpenMP test processes spin-starve each other (set OMP_WAIT_POLICY=PASSIVE)

A test gate ran 4 ctest jobs at once, each with OMP_NUM_THREADS=16, pinned to the same
16 CPUs. libgomp spin-waits at barriers and only throttles the spin when ONE process has
more threads than CPUs, so four processes with 16 spinning threads each burned the CPUs
the working threads needed. A relocalization test with thousands of short parallel
regions took 13 s alone and 195-197 s in the gate, and timed out (240 s) in Debug while
passing its checks. It looked like the new feature had made the test slow.

Avoidance: when several OpenMP processes share a CPU set (ctest -j, parallel benches),
export OMP_WAIT_POLICY=PASSIVE: 4 concurrent copies went 210 s -> 36 s, and the whole
Release lane 202 s -> 176 s. It is not free: lockstep pipeline tests with many tiny
regions got 10-40% slower (thread wake-up), and a single live process gained nothing,
so set it where processes share CPUs, not blindly in the app. Before blaming code for a
slow test, time it alone vs under the gate's concurrency.
## Sources
- kin (vendor/azu): scripts/gate.sh, tests relocalizer_contract / pipeline_*_contract

# `git log --since=<YYYY-MM-DD>` silently skips that day's commits

A premise check meant to ask "did anything change since this plan was written?" ran
`git log --since=2026-10-03 -- <files>` on the day the plan was written. It returned
nothing, even for commits made minutes earlier, so the check always answered "unchanged"
on day one. Git reads a bare date as that date *at the current time of day*, so with a
12:56 clock `--since=2026-10-03` means "after 12:56 today". Verified: 0 commits with the
bare date, 7 with `--since="2026-10-03 00:00"`. Even with midnight added, a date cannot
tell commits from before the plan apart from commits after it on the same day.

Avoidance: compare against a commit, not a date. For "what changed since file X was
added", take the commit that added it as the base:
`base=$(git log --diff-filter=A --format=%h -1 -- X); git log "$base"..HEAD -- <files>`.
If a date is unavoidable, always spell out the time (`"<date> 00:00"`) and treat
same-day results as ambiguous.
## Sources
- skills/hands-plan: verbs/resume.md (premise refresh), found by the end-to-end test run

# `git log --grep='[P1]'` is a regex character class, not the literal tag

A close-out audit looked for a plan's commits with `git log --grep='[P1]'` to check that
every task was committed under the `[P1][T…]` tag format. Unquoted brackets in a regex are
a character class: the pattern matches any subject containing `P` or `1`. It reported a
commit `[plans] scaffold … (hands-plan v1)` as belonging to Plan 1, through the `1` in
`v1`, so the audit could count commits that are not the plan's. Verified: 6 matches with
the regex, 5 with `-F`.

Avoidance: grep commit tags as fixed strings, `git log -F --grep='[P1]'`, or escape the
brackets (`'\[P1\]'`). In a monorepo, also scope by path (`-- <sub-project>`), because
plan numbers repeat across sub-projects with their own plan systems.
## Sources
- skills/hands-plan: verbs/close.md, verbs/status.md (commit audits), found by the end-to-end test run
