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
