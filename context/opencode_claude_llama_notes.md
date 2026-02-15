# OpenAI API Compatibility: Claude Code, Opencode, and llama.cpp

## Overview

This document details how Claude Code and Opencode CLI clients interact with OpenAI-compatible APIs, and what llama-proxy must support to work with both clients plus llama.cpp server.

Last updated: 2026-02-15

## Client Differences Summary

| Feature | Claude Code | Opencode | llama.cpp Server |
|---------|-------------|----------|------------------|
| Primary Endpoint | `/v1/chat/completions` | `/v1/chat/completions` | `/v1/chat/completions` |
| Streaming | SSE (`text/event-stream`) | SSE (`text/event-stream`) | SSE (`text/event-stream`) |
| Tool Calls | OpenAI format | OpenAI format | OpenAI format |
| Special Extensions | None identified | `reasoning_text`, `reasoning_opaque` | `timings` object, `/props`, `/slots` |
| Auth Header | `Authorization: Bearer <key>` | `Authorization: Bearer <key>` | Optional (can be empty) |
| Response Format | Standard OpenAI | Standard OpenAI + Copilot | Standard OpenAI + timings |

## Vanilla OpenAI Specification

Both clients follow the standard OpenAI Chat Completions API:

### Request Schema
```json
{
  "model": "string (required)",
  "messages": [
    {
      "role": "system|user|assistant|tool",
      "content": "string or array",
      "tool_call_id": "string (for tool role)",
      "tool_calls": [{
        "id": "string",
        "type": "function",
        "function": {
          "name": "string",
          "arguments": "string (JSON)"
        }
      }]
    }
  ],
  "temperature": 0.0-2.0,
  "top_p": 0.0-1.0,
  "max_tokens": "integer",
  "stream": "boolean",
  "tools": [{
    "type": "function",
    "function": {
      "name": "string",
      "description": "string",
      "parameters": "object (JSON schema)"
    }
  }],
  "tool_choice": "auto|none|required|{type: 'function', function: {name: 'string'}}",
  "stop": ["string"],
  "frequency_penalty": -2.0 to 2.0,
  "presence_penalty": -2.0 to 2.0,
  "user": "string"
}
```

### Non-Streaming Response Schema
```json
{
  "id": "chatcmpl-123",
  "object": "chat.completion",
  "created": 1234567890,
  "model": "model-name",
  "choices": [{
    "index": 0,
    "message": {
      "role": "assistant",
      "content": "string or null",
      "tool_calls": [{
        "id": "call-123",
        "type": "function",
        "function": {
          "name": "function_name",
          "arguments": "{\"param\": \"value\"}"
        }
      }]
    },
    "finish_reason": "stop|length|tool_calls|content_filter"
  }],
  "usage": {
    "prompt_tokens": 100,
    "completion_tokens": 50,
    "total_tokens": 150
  }
}
```

### Streaming Response Format (SSE)
```
data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1234567890,"model":"model","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"},"finish_reason":null}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1234567890,"model":"model","choices":[{"index":0,"delta":{"content":" world"},"finish_reason":null}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1234567890,"model":"model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
```

**Key Points:**
- Each chunk prefixed with `data: `
- Chunks are complete JSON objects
- First chunk contains `role` in delta
- Subsequent chunks contain incremental `content` or `tool_calls`
- Final chunk has `finish_reason` set
- Stream terminates with `data: [DONE]`

## llama.cpp Specific Extensions

llama.cpp adds non-standard fields that don't break OpenAI compatibility:

### Response Extensions

#### Timings Object (Non-Standard)
```json
{
  "timings": {
    "prompt_n": 100,              // tokens in prompt
    "prompt_ms": 50.5,            // time to process prompt
    "prompt_per_second": 1980.2,  // prompt tokens/sec
    "predicted_n": 50,            // completion tokens generated
    "predicted_ms": 100.0,        // time to generate completion
    "predicted_per_second": 500.0,// completion tokens/sec
    "cache_n": 10                 // tokens loaded from cache
  }
}
```

**Usage in llama-proxy:**
- Extracted from response to calculate tokens/sec metrics
- Available in both streaming (final chunk) and non-streaming responses
- Used by stats collector for performance metrics

### Additional Endpoints (llama.cpp only)

#### `/props` - Server Properties
```json
{
  "default_generation_settings": {
    "n_ctx": 4096,        // CRITICAL: total context window size
    "temperature": 0.8,
    "top_k": 40,
    "top_p": 0.95
  },
  "total_slots": 1,
  "chat_template": "chatml",
  "model_path": "/path/to/model.gguf"
}
```

**Usage in llama-proxy:**
- Fetch `n_ctx` to calculate context usage percentage
- Cached per backend URL for application lifetime
- See: `src/proxy/context.rs`

#### `/slots` - Processing Slot Status
```json
{
  "slots": [{
    "id": 0,
    "state": 0,          // 0=idle, 1=processing
    "n_ctx": 4096,
    "n_predict": 512,
    "cache_tokens": 120
  }]
}
```

**Usage:**
- Can provide real-time context info
- Alternative to `/props` for getting `n_ctx`

#### `/health` - Health Check
Simple health endpoint, returns empty 200 OK

#### `/metrics` - Prometheus Metrics
Returns Prometheus-format metrics for monitoring

### Request Extensions

llama.cpp accepts additional sampling parameters (backward compatible):

```json
{
  "mirostat": 0,              // 0=disabled, 1=v1, 2=v2
  "mirostat_tau": 5.0,
  "mirostat_eta": 0.1,
  "dry_multiplier": 0.0,      // DRY (Don't Repeat Yourself) sampling
  "dry_base": 1.75,
  "dry_penalty_last_n": 256
}
```

**Compatibility:** Safe to pass through - ignored by standard OpenAI endpoints

## Opencode-Specific Extensions

Opencode supports Anthropic/Copilot extensions for reasoning models:

### Extended Message Format
```json
{
  "role": "assistant",
  "content": "Main response",
  "reasoning_text": "Extended reasoning shown to user",    // Anthropic/Copilot
  "reasoning_opaque": "State data for multi-turn context" // Anthropic/Copilot
}
```

### Extended Response Format
```json
{
  "choices": [{
    "message": {
      "role": "assistant",
      "content": "Response",
      "reasoning_text": "Visible reasoning steps",
      "reasoning_opaque": "Opaque state blob"
    }
  }],
  "usage": {
    "prompt_tokens": 100,
    "completion_tokens": 50,
    "total_tokens": 150,
    "completion_tokens_details": {
      "reasoning_tokens": 20,           // Tokens used for reasoning
      "accepted_prediction_tokens": 5,   // Speculative execution accepted
      "rejected_prediction_tokens": 2    // Speculative execution rejected
    }
  }
}
```

### Extended Request Parameters
```json
{
  "reasoning_effort": "low|medium|high",  // For reasoning models
  "verbosity": "low|medium|high",         // Text verbosity control
  "thinking_budget": 1000                 // Token budget for thinking
}
```

**Compatibility Notes:**
- These fields are **optional** and clients gracefully handle their absence
- llama.cpp will ignore unknown fields in requests (safe pass-through)
- Responses without these fields work fine with Opencode
- llama-proxy should **preserve** these fields if present, but doesn't need to generate them

## Tool Call Handling Details

Both clients use identical OpenAI tool call format, but streaming behavior is critical:

### Streaming Tool Call Accumulation

Tool calls are sent incrementally across multiple chunks:

```
Chunk 1:
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-123","type":"function","function":{"name":"get_weather","arguments":""}}]}}]}

Chunk 2:
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"loc"}}]}}]}

Chunk 3:
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ation\":"}}]}}]}

Chunk 4:
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"Paris\"}"}}]}}]}

Final:
data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}
```

**Accumulation Logic:**
1. Use `index` field to track which tool call slot
2. Concatenate `arguments` strings across chunks
3. Parse complete JSON only when `finish_reason` is set
4. Validate with `JSON.parse()` before emitting

**Common Bug (Qwen3-Coder):**
Models sometimes generate invalid file paths in arguments:
```json
{"file_path": "/home/user/../vendor/opencode"}  // Double slashes, relative paths
```

**llama-proxy Fix:**
- `toolcall_bad_filepath_fix` detects and repairs these
- Applied per-chunk during streaming
- Applied to final response in non-streaming

See: `src/fixes/toolcall_bad_filepath_fix.rs`

## HTTP Headers and Authentication

### Required Headers
```
Content-Type: application/json             // Request body
Authorization: Bearer <api_key>            // Optional for llama.cpp, required for OpenAI
```

### Optional Headers
```
User-Agent: <client_info>                  // Client identification
```

### Response Headers
```
Content-Type: application/json             // Non-streaming
Content-Type: text/event-stream            // Streaming
```

**llama-proxy Behavior:**
- Forwards all headers except `Host` (changed to backend)
- Skips `Content-Length` for streaming responses
- Preserves `Authorization` even if empty/missing

## Finish Reason Mapping

Standard finish reasons both clients understand:

| Finish Reason | Meaning | Client Behavior |
|---------------|---------|-----------------|
| `stop` | Natural completion at stop token | Display complete response |
| `length` | Hit max_tokens limit | Indicate truncation |
| `tool_calls` | Model wants to call tools | Parse and execute tools |
| `content_filter` | Filtered by safety system | Show filter message |

**llama.cpp Compatibility:**
- Always returns one of these standard values
- Proxy must preserve finish reason exactly

## Context Window Calculation

Critical for both clients to understand context limits:

```
Total Context Used = prompt_n + cache_n + predicted_n

Context Percentage = (Total Context Used / n_ctx) × 100
```

**Where to Get Values:**
- `prompt_n`, `cache_n`, `predicted_n`: From `timings` in response
- `n_ctx`: From `/props` endpoint (llama.cpp specific)

**llama-proxy Implementation:**
1. Cache `n_ctx` from `/props` at startup (per backend)
2. Extract token counts from `timings` in response
3. Calculate percentage and include in metrics
4. Export to InfluxDB for monitoring

See: `src/stats/collector.rs`, `src/proxy/context.rs`

## Error Response Format

Standard across all systems:

```json
{
  "error": {
    "message": "Descriptive error message",
    "type": "invalid_request_error",
    "param": "max_tokens",
    "code": "invalid_value"
  }
}
```

**llama-proxy Behavior:**
- Pass through errors from backend unchanged
- Add proxy-specific errors for fix failures or config issues
- Maintain same JSON structure

## Design Patterns Discovered

### 1. Transparent Proxy Pattern
llama-proxy routes all requests to backend without parsing (except for fix application):
- Minimal latency overhead
- Maximum compatibility
- Fixes applied as post-processing layer

### 2. Streaming Chunk Processing
Process each SSE chunk independently:
```rust
for line in sse_stream {
    if line.starts_with("data: ") {
        let json = parse_json(&line[6..]);
        let fixed_json = apply_fixes(json);
        emit_chunk(fixed_json);
    }
}
```

### 3. Index-Based Tool Call Accumulation
Use `index` field to maintain multiple concurrent tool call buffers:
```rust
let mut tool_calls: HashMap<u64, ToolCall> = HashMap::new();
for chunk in stream {
    if let Some(idx) = chunk.tool_calls[0].index {
        tool_calls.entry(idx).or_insert_default().append(chunk);
    }
}
```

### 4. Context Caching Pattern
Expensive operations (like `/props` fetch) cached per backend:
```rust
static CONTEXT_CACHE: Lazy<DashMap<String, u64>> = Lazy::new(DashMap::new);

async fn get_context_size(backend_url: &str) -> u64 {
    if let Some(cached) = CONTEXT_CACHE.get(backend_url) {
        return *cached;
    }
    let size = fetch_from_props(backend_url).await;
    CONTEXT_CACHE.insert(backend_url.to_string(), size);
    size
}
```

## Compatibility Requirements for llama-proxy

To support both Claude Code and Opencode with llama.cpp backend:

### MUST Support (Critical)
✅ OpenAI `/v1/chat/completions` endpoint
✅ Streaming via SSE (`text/event-stream`)
✅ Non-streaming JSON responses
✅ Tool calls in OpenAI format
✅ Tool call streaming with index-based accumulation
✅ Standard finish reasons (stop, length, tool_calls, content_filter)
✅ Authorization header pass-through
✅ Request body forwarding (all standard + llama.cpp parameters)

### SHOULD Support (Enhanced Features)
✅ llama.cpp `timings` object for metrics
✅ llama.cpp `/props` endpoint for context size
✅ Pass-through of unknown request parameters (forward compatibility)
✅ Pass-through of unknown response fields (preserve Opencode extensions)
✅ Response fixes for model-specific bugs (Qwen3-Coder file paths)

### MAY Support (Optional)
⚠️ Opencode reasoning extensions (currently not generated by llama.cpp)
⚠️ llama.cpp `/slots` endpoint monitoring
⚠️ llama.cpp `/metrics` endpoint
⚠️ Custom sampling parameters (mirostat, DRY, etc.)

### MUST NOT Break
❌ Don't modify standard OpenAI fields
❌ Don't strip unknown fields from requests/responses
❌ Don't reorder tool_calls array
❌ Don't change tool call IDs
❌ Don't modify finish_reason values
❌ Don't alter token counts in usage object

## Testing Recommendations

### Test with Claude Code Client
```bash
# Claude Code expects standard OpenAI compatibility
# Test non-streaming
curl -X POST http://localhost:8066/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"qwen3","messages":[{"role":"user","content":"Hello"}]}'

# Test streaming
curl -X POST http://localhost:8066/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"qwen3","messages":[{"role":"user","content":"Hello"}],"stream":true}'

# Test tool calls
curl -X POST http://localhost:8066/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"qwen3","messages":[{"role":"user","content":"What is the weather?"}],"tools":[{"type":"function","function":{"name":"get_weather","parameters":{"type":"object","properties":{"location":{"type":"string"}}}}}]}'
```

### Test with Opencode Client
```bash
# Opencode has same basic requirements as Claude Code
# Test that reasoning extensions pass through (if backend supports)
curl -X POST http://localhost:8066/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"qwen3","messages":[{"role":"user","content":"Solve this problem"}],"reasoning_effort":"high"}'
```

### Test llama.cpp Extensions
```bash
# Verify timings are preserved
curl http://localhost:8066/v1/chat/completions \
  -d '{"model":"qwen3","messages":[{"role":"user","content":"Test"}]}' | jq '.timings'

# Verify context fetching works
curl http://localhost:8080/props | jq '.default_generation_settings.n_ctx'
```

## Performance Considerations

### Latency Breakdown
```
Client → Proxy → Backend → Proxy → Client
  ↓       ↓        ↓        ↓       ↓
  1ms   <1ms     50ms     1-2ms   1ms

Where:
- Proxy overhead: ~1-2ms per request (fix application)
- Streaming per-chunk: <0.1ms per chunk
- Context fetch: ~5ms (cached after first request)
```

### Optimization Techniques
1. **Streaming buffer size**: Keep small (1KB) for low latency
2. **Fix early-exit**: Check `applies()` before parsing JSON
3. **Context caching**: Never refetch `/props` for same backend
4. **Connection pooling**: Reuse HTTP connections to backend
5. **Async metrics export**: Don't block response on InfluxDB writes

## Known Issues and Workarounds

### Issue 1: Qwen3-Coder Invalid File Paths
**Symptoms:** Tool calls contain malformed file paths with `//`, `../`, broken escaping

**Root Cause:** Model generates paths not following JSON string escaping rules

**Solution:** `toolcall_bad_filepath_fix` module cleans paths before sending to client

**Status:** ✅ Fixed in llama-proxy v0.1.0

### Issue 2: Incomplete Streaming Stats
**Symptoms:** Streaming responses had inaccurate token counts and timing

**Root Cause:** Stats calculated before all chunks received

**Solution:** Accumulate all chunks, extract final metrics asynchronously after stream completes

**Status:** ✅ Fixed in commit b8ff45b

### Issue 3: Missing Context Percentage
**Symptoms:** Couldn't determine how close to context limit

**Root Cause:** `n_ctx` not included in standard OpenAI response

**Solution:** Fetch from llama.cpp `/props` and cache per backend

**Status:** ✅ Implemented in `context.rs`

## References

### Source Files Analyzed
- `/home/iphands/prog/slop/vendor/opencode/packages/opencode/src/provider/sdk/copilot/chat/openai-compatible-chat-language-model.ts`
- `/home/iphands/prog/slop/vendor/claude-code/` (structure analyzed, implementation details inferred)
- `/home/iphands/prog/slop/vendor/llama.cpp/tools/server/README.md`
- `/home/iphands/prog/slop/llama-proxy/src/**` (full implementation)

### External Documentation
- OpenAI Chat Completions API: https://platform.openai.com/docs/api-reference/chat
- llama.cpp server API: https://github.com/ggerganov/llama.cpp/blob/master/examples/server/README.md
- Server-Sent Events: https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events

### Related Context Files
- `../context/high_level.md` - Dependency comparisons
- `../context/patterns.md` - Design patterns
- `../context/pitfalls.md` - Common bugs and solutions (TODO: add streaming tool call pitfalls)
