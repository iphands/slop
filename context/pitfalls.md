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
