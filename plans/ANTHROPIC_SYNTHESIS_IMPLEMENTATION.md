# Anthropic Streaming Synthesis Implementation

## Summary

Fixed the streaming synthesis format mismatch for Anthropic API endpoints. Previously, the proxy synthesized all streaming responses in OpenAI SSE format, causing Claude TUI to fall back to non-streaming mode.

## Changes Made

### 1. New Synthesis Function (`src/proxy/synthesis.rs`)

Added `synthesize_anthropic_streaming_response()` that outputs Anthropic SSE format:

**Event Sequence:**
```
event: message_start
data: {"type":"message_start","message":{...}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{...}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"..."}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{...}}

event: message_stop
data: {"type":"message_stop"}
```

**Key Features:**
- Text content chunked into 50-character pieces (configurable DEFAULT_CHUNK_SIZE)
- Supports both text and thinking blocks
- Proper event sequencing per Anthropic spec
- Complete usage and stop_reason information

### 2. Helper Functions

Added builder functions for each Anthropic event type:
- `build_message_start_event()` - Initial message metadata
- `build_content_block_start_event()` - Start of text block
- `build_thinking_block_start_event()` - Start of thinking block (with optional signature)
- `build_content_block_delta_event()` - Text delta chunks
- `build_thinking_block_delta_event()` - Thinking delta chunks
- `build_content_block_stop_event()` - End of content block
- `build_message_delta_event()` - Final metadata (stop_reason, usage)
- `build_message_stop_event()` - Stream terminator

### 3. Handler Updates (`src/proxy/handler.rs`)

Modified synthesis path to route based on API format:
- `/v1/messages` → `synthesize_anthropic_streaming_response()`
- `/v1/chat/completions` → `synthesize_streaming_response()`

**Before:**
```rust
if let Some(response) = chat_response {
    match synthesize_streaming_response(response).await {
        // Always OpenAI format
    }
}
```

**After:**
```rust
if is_anthropic_api {
    match synthesize_anthropic_streaming_response(anthropic_msg).await {
        // Anthropic SSE format
    }
} else {
    match synthesize_streaming_response(response).await {
        // OpenAI SSE format
    }
}
```

### 4. Module Exports (`src/proxy/mod.rs`)

Exported new function:
```rust
pub use synthesis::{synthesize_anthropic_streaming_response, synthesize_streaming_response};
```

## Test Coverage

Added comprehensive tests in `src/proxy/synthesis.rs`:

**Unit Tests:**
- `test_build_message_start_event()` - Event structure
- `test_build_content_block_start_event()` - Text block start
- `test_build_thinking_block_start_event()` - Thinking block start
- `test_build_content_block_delta_event()` - Text delta
- `test_build_thinking_block_delta_event()` - Thinking delta
- `test_build_content_block_stop_event()` - Block stop
- `test_build_message_delta_event()` - Message delta
- `test_build_message_stop_event()` - Message stop

**Integration Tests:**
- `test_synthesize_anthropic_chunks_text_block()` - Single text block
- `test_synthesize_anthropic_chunks_thinking_block()` - Single thinking block
- `test_synthesize_anthropic_chunks_multiple_blocks()` - Multiple content blocks
- `test_synthesize_anthropic_chunks_empty_content()` - Empty message
- `test_synthesize_anthropic_streaming_full_flow()` - Full async response
- `test_anthropic_event_sequence_order()` - Event ordering verification

**Test Results:**
```
test result: ok. 17 passed; 0 failed; 0 ignored
```

All 261 total tests pass.

## Expected Behavior

### Before Fix:
```
Client → /v1/messages stream=true
Proxy → OpenAI SSE format (data: {"object":"chat.completion.chunk",...})
Client → [ERROR] Stream completed without receiving message_start event
Client → Fallback to stream=false
Client → Re-request and display response
```

### After Fix:
```
Client → /v1/messages stream=true
Proxy → Anthropic SSE format (event: message_start, event: content_block_delta, ...)
Client → Receives and displays streaming response immediately
Client → No errors, no fallback, no re-request
```

## Manual Testing

### Test with curl:

```bash
# Start proxy
RUST_LOG=debug cargo run -- run --config config.yaml

# Test Anthropic streaming endpoint
curl -X POST http://localhost:8066/v1/messages \
  -H "Content-Type: application/json" \
  -d '{
    "model": "test",
    "messages": [{"role": "user", "content": "Hi"}],
    "stream": true,
    "max_tokens": 50
  }'

# Expected output:
# event: message_start
# data: {"type":"message_start","message":{...}}
#
# event: content_block_start
# data: {"type":"content_block_start",...}
#
# event: content_block_delta
# data: {"type":"content_block_delta",...}
# ...
```

### Test with Claude TUI:

1. Configure Claude TUI to use proxy: `http://localhost:8066`
2. Send message: "Hi"
3. Verify:
   - ✅ Response streams immediately
   - ✅ No error logs about "message_start event"
   - ✅ No fallback to non-streaming mode
   - ✅ Only ONE request in proxy logs (not two)

### Debug Logs to Check:

**Success indicators:**
```
[DEBUG] Synthesizing Anthropic streaming response from complete JSON
```

**Failure indicators (should NOT appear):**
```
[ERROR] Stream completed without receiving message_start event
[ERROR] Error streaming, falling back to non-streaming mode
```

## Files Modified

1. `src/proxy/synthesis.rs` - Added Anthropic synthesis function and helpers
2. `src/proxy/handler.rs` - Updated to route to correct synthesis function
3. `src/proxy/mod.rs` - Exported new function
4. `ANTHROPIC_SYNTHESIS_IMPLEMENTATION.md` - This documentation

## Compatibility

- ✅ OpenAI endpoints (`/v1/chat/completions`) unchanged - still use OpenAI SSE format
- ✅ Anthropic endpoints (`/v1/messages`) now use Anthropic SSE format
- ✅ Non-streaming requests unaffected
- ✅ All existing tests pass
- ✅ No breaking changes

## Next Steps

1. **Manual testing** with Claude TUI to verify real-world behavior
2. **Monitor debug logs** for synthesis path selection
3. **Verify no fallback errors** in Claude TUI
4. **Test with various message types** (text, thinking, tool calls if applicable)

## References

- Plan: `/home/iphands/prog/slop/plans/proxy_proper_anth_synth.md`
- Anthropic SSE spec: Inferred from `src/proxy/streaming.rs` event parsing
- OpenAI SSE spec: Existing `synthesize_streaming_response()` implementation
