# Implementation Summary: Anthropic Messages API Support

## Problem
Claude Code was configured to use the **Anthropic Messages API** (`/v1/messages`) but the proxy was only parsing responses as OpenAI Chat Completions format, causing synthesis failures with the error:
```
WARN: Cannot parse as ChatCompletionResponse for synthesis error=missing field `prompt_tokens`
```

## Root Cause
- Client sends: `POST /v1/messages` (Anthropic format)
- Backend (llama.cpp) correctly returns: Anthropic Messages API format with `input_tokens`/`output_tokens`
- Proxy synthesis path tried to parse: As OpenAI format with `prompt_tokens`/`completion_tokens`
- Result: Parsing failed, synthesis skipped, client got non-streaming response

## Solution Implemented

### 1. Added Anthropic Messages API Types (`src/api/openai.rs`)
Added complete type definitions for Anthropic Messages API:
- `AnthropicMessage` - Main response structure
- `AnthropicContentBlock` - Text and thinking content blocks
- `AnthropicUsage` - Token usage with `input_tokens`/`output_tokens`

### 2. Implemented Format Conversion
Added `From<AnthropicMessage> for ChatCompletionResponse` implementation that:
- Converts `input_tokens` → `prompt_tokens`
- Converts `output_tokens` → `completion_tokens`
- Maps `stop_reason` → `finish_reason` ("end_turn" → "stop", etc.)
- Concatenates multiple content blocks (text + thinking)
- Preserves all critical fields for synthesis

### 3. Updated Synthesis Logic (`src/proxy/handler.rs`)
Modified `handle_non_streaming_response()` to:
- Accept `is_anthropic_api` flag (already detected at line 41)
- Try parsing as `AnthropicMessage` when `is_anthropic_api == true`
- Try parsing as `ChatCompletionResponse` when `is_anthropic_api == false`
- Convert Anthropic → OpenAI format before synthesis
- Fall back gracefully with diagnostic logging if parsing fails

## Files Modified
1. **src/api/openai.rs**
   - Added Anthropic types (lines 268-341)
   - Added conversion implementation (lines 343-376)
   - Added comprehensive tests (lines 643-737)

2. **src/proxy/handler.rs**
   - Updated imports to include `AnthropicMessage` (line 13)
   - Added `is_anthropic_api` parameter to `handle_non_streaming_response()` (line 177)
   - Updated synthesis logic to handle both formats (lines 293-340)

## Tests Added
- `test_parse_anthropic_message` - Verify Anthropic JSON parsing
- `test_parse_anthropic_message_with_thinking` - Handle thinking blocks
- `test_anthropic_to_openai_conversion` - Basic conversion test
- `test_anthropic_to_openai_with_thinking` - Multi-block conversion
- `test_anthropic_stop_reason_mapping` - Verify reason mapping
- `test_real_anthropic_response_from_llama_server` - Integration test with real format

## Test Results
```bash
cargo test api::openai::tests
# Result: 30 tests passed (6 new Anthropic tests)

cargo test
# Result: 249 tests passed (all existing tests still pass)
```

## Expected Behavior After Fix

### Before (Broken)
```
Client → /v1/messages?beta=true
Backend → Anthropic format (input_tokens: 158, output_tokens: 265)
Proxy → ❌ Cannot parse as ChatCompletionResponse
      → ⚠️  WARN: missing field `prompt_tokens`
      → Returns complete JSON (not streaming)
```

### After (Fixed)
```
Client → /v1/messages?beta=true
Backend → Anthropic format (input_tokens: 158, output_tokens: 265)
Proxy → ✅ Parsed as AnthropicMessage
      → ✅ Converted to ChatCompletionResponse
      → ✅ Synthesizing streaming response
      → Returns streaming SSE response
```

## Compatibility
- ✅ **Existing OpenAI endpoint** (`/v1/chat/completions`) - unchanged behavior
- ✅ **New Anthropic endpoint** (`/v1/messages`) - now works correctly
- ✅ **All existing fixes** - still apply correctly
- ✅ **Metrics collection** - works with both formats (uses converted OpenAI format)
- ✅ **Backward compatibility** - zero breaking changes

## Manual Verification Steps

### Test 1: Anthropic Endpoint (The Fix)
```bash
curl -X POST http://localhost:8066/v1/messages \
  -H "Content-Type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -d '{
    "model": "qwen3",
    "messages": [{"role": "user", "content": "Hello"}],
    "stream": true,
    "max_tokens": 100
  }'
```

**Expected logs:**
```
DEBUG: Detected API format: is_anthropic_api=true
DEBUG: Parsed Anthropic Message, converting to ChatCompletionResponse
DEBUG: Synthesizing streaming response from complete JSON
INFO: model=ERNIE-4.5 tokens=158/265 tps=278.56s/116.80s
```
(No warnings about missing fields!)

### Test 2: OpenAI Endpoint (Regression Test)
```bash
curl -X POST http://localhost:8066/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "qwen3",
    "messages": [{"role": "user", "content": "Hello"}],
    "stream": true
  }'
```

**Expected:** Works as before, no changes to behavior.

## Implementation Details

### Field Mapping
| Anthropic Field | OpenAI Field |
|----------------|--------------|
| `input_tokens` | `prompt_tokens` |
| `output_tokens` | `completion_tokens` |
| `stop_reason` | `finish_reason` |
| `content[].text` | `message.content` |
| `content[].thinking` | `message.content` (concatenated) |

### Stop Reason Mapping
| Anthropic | OpenAI |
|-----------|--------|
| `end_turn` | `stop` |
| `max_tokens` | `length` |
| `stop_sequence` | `stop` |
| (other) | (pass-through) |

### Content Block Handling
- Text blocks → concatenated with `\n`
- Thinking blocks → concatenated with `\n`
- Multiple blocks → joined in order

## Design Decisions

### Why Convert Instead of Separate Synthesis?
- **Reuse existing synthesis logic** - no duplication
- **Metrics collection works** - uses standard OpenAI format internally
- **Fixes still apply** - conversion happens before fix application
- **Minimal changes** - only added parsing, not new synthesis path

### Why Not Skip Synthesis for Anthropic?
- **Client expects streaming** - when `stream: true` in request
- **Consistency** - both endpoints behave the same way
- **Fix application** - synthesis allows fixes to work in streaming mode

### Why Store Types in `openai.rs`?
- **Single API module** - all API types in one place
- **Clear conversion** - `From` trait shows relationship
- **Easy to find** - developers look in `api/` first

## Future Enhancements (Out of Scope)
- Native Anthropic SSE streaming (without conversion)
- Support for Anthropic-specific fields (citations, etc.)
- Separate metrics for Anthropic vs OpenAI endpoints

## Related Documentation
- Plan: `/home/iphands/prog/slop/plans/proxy_proper_anth_synth.md`
- Vendor code: `../vendor/llama.cpp/tools/server/server-task.cpp:1008-1071`
- Client notes: `../context/opencode_claude_llama_notes.md`
