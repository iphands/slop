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
