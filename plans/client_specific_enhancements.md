# llama-proxy Enhancement Plan: Multi-Client Support

## Context

The llama-proxy sits between llama.cpp server and AI coding clients (Claude Code CLI/TUI and Opencode CLI/TUI). After comprehensive research of all three codebases, the **excellent news** is that the proxy already works with both clients due to its transparent pass-through architecture.

**Why this enhancement is needed:**
- Both clients use the same base OpenAI Chat Completions API
- Opencode adds optional reasoning extensions (reasoning_text, reasoning_opaque, etc.)
- llama.cpp provides timings and context endpoints that enhance metrics
- The proxy's transparent design already handles unknown fields correctly
- We need type safety, better endpoint coverage, and enhanced metrics tracking

**Key Finding:** Opencode's reasoning extensions are **optional**. llama.cpp models don't generate them, but the proxy should preserve them if they pass through for forward compatibility.

## Proposed Changes

### 1. Extend Type Definitions for Opencode Extensions

**Files to modify:**
- `/home/iphands/prog/slop/llama-proxy/src/api/openai.rs`

**Changes:**

Add to `ResponseMessage` struct (around line 143):
```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResponseMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,

    // Opencode/Anthropic/Copilot reasoning extensions (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_opaque: Option<String>,
}
```

Add to `Delta` struct (around line 172):
```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Delta {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,

    // Reasoning extensions for streaming
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_opaque: Option<String>,
}
```

Add new struct for extended usage (after line 187):
```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompletionTokensDetails {
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    #[serde(default)]
    pub accepted_prediction_tokens: Option<u64>,
    #[serde(default)]
    pub rejected_prediction_tokens: Option<u64>,
}
```

Update `Usage` struct (around line 183):
```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,

    // Extended usage details (Opencode/Copilot)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens_details: Option<CompletionTokensDetails>,
}
```

Add to `ChatCompletionRequest` struct (around line 30):
```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    // ... existing fields ...

    // Opencode request extensions (pass-through to backend)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<u64>,
}
```

**Rationale:** All fields use `#[serde(default, skip_serializing_if = "Option::is_none")]` to maintain backward compatibility. Fields are only serialized if present.

### 2. Add Explicit Pass-Through Endpoints

**Files to modify:**
- `/home/iphands/prog/slop/llama-proxy/src/proxy/handler.rs`

**Changes:**

Add new method to `ProxyHandler` (after line 121):
```rust
/// Simple pass-through with no fix application or stats collection
async fn proxy_passthrough(&self, req: Request<Body>) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let headers = req.headers().clone();
    let path = uri.path();

    tracing::debug!(method = %method, path = %path, "Pass-through request");

    // Read body
    let body_bytes = match to_bytes(req.into_body(), 1024 * 1024 * 10).await {
        Ok(bytes) => bytes,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("Failed to read body: {}", e))
                .into_response();
        }
    };

    // Build backend URL
    let backend_url = format!("{}{}", self.state.config.backend.url(), path);

    // Forward to backend
    let mut backend_req = self.state.http_client.request(
        Method::from_bytes(method.as_str().as_bytes()).unwrap(),
        &backend_url,
    );

    // Copy headers
    for (name, value) in headers.iter() {
        if name != header::HOST {
            backend_req = backend_req.header(name, value);
        }
    }
    backend_req = backend_req.body(body_bytes);

    let backend_response = match backend_req.send().await {
        Ok(resp) => resp,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, format!("Backend error: {}", e))
                .into_response();
        }
    };

    // Pass through response
    let status = backend_response.status();
    let headers = backend_response.headers().clone();
    let body = match backend_response.bytes().await {
        Ok(b) => b,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, format!("Failed to read response: {}", e))
                .into_response();
        }
    };

    let mut response = Response::builder().status(status);
    for (name, value) in headers {
        if let Some(name) = name {
            response = response.header(name, value);
        }
    }
    response.body(Body::from(body)).unwrap()
}
```

Update `handle` method to route additional endpoints (around line 30):
```rust
pub async fn handle(&self, req: Request<Body>) -> Response {
    let path = req.uri().path();
    let method = req.method();

    // Route specific endpoints
    match (method, path) {
        // llama.cpp monitoring/status endpoints (simple pass-through)
        (&Method::GET, "/props") |
        (&Method::GET, "/slots") |
        (&Method::GET, "/health") |
        (&Method::GET, "/v1/health") |
        (&Method::GET, "/v1/models") |
        (&Method::GET, "/metrics") => {
            return self.proxy_passthrough(req).await;
        }

        // All other routes continue with existing logic
        _ => {}
    }

    // Existing handle logic continues here...
```

**Rationale:** These endpoints don't need fix application or stats collection, so simple pass-through is sufficient. This allows test tools and monitoring to query llama.cpp status through the proxy.

### 3. Enhanced Metrics Collection

**Files to modify:**
- `/home/iphands/prog/slop/llama-proxy/src/stats/collector.rs`

**Changes:**

Add to `RequestMetrics` struct:
```rust
#[derive(Debug, Clone, Serialize)]
pub struct RequestMetrics {
    // ... existing fields ...

    // Extended token details (Opencode/Copilot extensions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_prediction_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejected_prediction_tokens: Option<u64>,
}
```

Update `from_response` method to extract extended usage:
```rust
// After extracting basic usage (around line in from_response)
if let Some(details) = usage.get("completion_tokens_details") {
    metrics.reasoning_tokens = details.get("reasoning_tokens")
        .and_then(|t| t.as_u64());
    metrics.accepted_prediction_tokens = details.get("accepted_prediction_tokens")
        .and_then(|t| t.as_u64());
    metrics.rejected_prediction_tokens = details.get("rejected_prediction_tokens")
        .and_then(|t| t.as_u64());
}
```

**Files to modify:**
- `/home/iphands/prog/slop/llama-proxy/src/stats/formatter.rs`

Update `format_pretty` to show reasoning tokens if present:
```rust
if let Some(reasoning) = metrics.reasoning_tokens {
    output.push_str(&format!("  Reasoning Tokens: {}\n", reasoning));
}
```

**Files to modify:**
- `/home/iphands/prog/slop/llama-proxy/src/exporters/influxdb.rs`

Add fields to InfluxDB export:
```rust
if let Some(reasoning) = metrics.reasoning_tokens {
    builder = builder.field("reasoning_tokens", reasoning as i64);
}
```

**Rationale:** These metrics are optional but valuable when reasoning models are used. They'll be tracked if present but won't break if absent.

### 4. Streaming Accumulator Updates

**Files to modify:**
- `/home/iphands/prog/slop/llama-proxy/src/proxy/streaming.rs`

**Changes:**

Update the chunk merging logic to handle reasoning fields. In the section where delta content is concatenated, add:

```rust
// Merge reasoning_text (concatenate like content)
if let Some(reasoning) = delta.get("reasoning_text").and_then(|r| r.as_str()) {
    if let Some(acc_msg) = accumulated_message.get_mut("reasoning_text") {
        if let Some(existing) = acc_msg.as_str() {
            *acc_msg = serde_json::Value::String(format!("{}{}", existing, reasoning));
        }
    } else {
        accumulated_message["reasoning_text"] = serde_json::Value::String(reasoning.to_string());
    }
}

// Merge reasoning_opaque (replace, not concat - it's a state blob)
if let Some(opaque) = delta.get("reasoning_opaque") {
    accumulated_message["reasoning_opaque"] = opaque.clone();
}
```

**Rationale:** `reasoning_text` is incremental like content, so concatenate. `reasoning_opaque` is a state blob, so replace with latest.

## Architecture Principles

### Maintain Clean Separation
- **Transparency**: Continue pass-through design, only modify responses for fixes
- **Modularity**: Keep fix registry, stats, exporters independent
- **Type Safety**: Use Option types for all optional fields
- **Backward Compatibility**: All new fields use `#[serde(default)]`

### Code Organization
```
src/
├── api/openai.rs          ← Add extension fields (types only)
├── proxy/
│   ├── handler.rs         ← Add pass-through routing
│   └── streaming.rs       ← Update reasoning field accumulation
├── stats/
│   ├── collector.rs       ← Add extended metrics fields
│   └── formatter.rs       ← Display extended metrics
└── exporters/influxdb.rs  ← Export extended metrics
```

**No new modules needed** - all changes fit cleanly into existing architecture.

## Unit Tests to Implement

### Test File: `src/api/openai.rs` (add to existing test module)

```rust
#[test]
fn test_response_message_with_reasoning() {
    let json = r#"{
        "role": "assistant",
        "content": "Answer",
        "reasoning_text": "Thinking steps",
        "reasoning_opaque": "state_blob"
    }"#;
    let msg: ResponseMessage = serde_json::from_str(json).unwrap();
    assert_eq!(msg.reasoning_text, Some("Thinking steps".to_string()));
    assert_eq!(msg.reasoning_opaque, Some("state_blob".to_string()));
}

#[test]
fn test_response_message_without_reasoning() {
    let json = r#"{
        "role": "assistant",
        "content": "Answer"
    }"#;
    let msg: ResponseMessage = serde_json::from_str(json).unwrap();
    assert_eq!(msg.reasoning_text, None);
    assert_eq!(msg.reasoning_opaque, None);
}

#[test]
fn test_usage_with_extended_details() {
    let json = r#"{
        "prompt_tokens": 100,
        "completion_tokens": 50,
        "total_tokens": 150,
        "completion_tokens_details": {
            "reasoning_tokens": 20
        }
    }"#;
    let usage: Usage = serde_json::from_str(json).unwrap();
    assert_eq!(usage.completion_tokens_details.unwrap().reasoning_tokens, Some(20));
}

#[test]
fn test_request_with_reasoning_effort() {
    let json = r#"{
        "model": "test",
        "messages": [{"role": "user", "content": "Test"}],
        "reasoning_effort": "high"
    }"#;
    let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.reasoning_effort, Some("high".to_string()));
}

#[test]
fn test_delta_with_reasoning() {
    let json = r#"{
        "role": "assistant",
        "content": "Text",
        "reasoning_text": "Thinking"
    }"#;
    let delta: Delta = serde_json::from_str(json).unwrap();
    assert_eq!(delta.reasoning_text, Some("Thinking".to_string()));
}
```

### Test File: `src/stats/collector.rs` (add to existing test module)

```rust
#[test]
fn test_extended_usage_extraction() {
    let response = serde_json::json!({
        "choices": [{
            "message": {"role": "assistant", "content": "test"},
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "total_tokens": 150,
            "completion_tokens_details": {
                "reasoning_tokens": 20,
                "accepted_prediction_tokens": 5
            }
        }
    });

    let metrics = RequestMetrics::from_response(
        &response,
        &serde_json::json!({"messages": []}),
        false,
        100.0
    );

    assert_eq!(metrics.reasoning_tokens, Some(20));
    assert_eq!(metrics.accepted_prediction_tokens, Some(5));
}
```

### Test File: `src/proxy/handler.rs` (new test module)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_passthrough_routes() {
        // Mock setup would test that /props, /slots, /health
        // route through proxy_passthrough method
        // (Integration test would be better for this)
    }
}
```

**Test Coverage Goal:** Maintain 80%+ overall coverage. New code should have 90%+ coverage.

## TODO List

### Phase 1: Type Extensions (2-3 hours)
- [ ] Add reasoning fields to `ResponseMessage` in `src/api/openai.rs`
- [ ] Add reasoning fields to `Delta` in `src/api/openai.rs`
- [ ] Add `CompletionTokensDetails` struct in `src/api/openai.rs`
- [ ] Update `Usage` struct with `completion_tokens_details`
- [ ] Add Opencode request parameters to `ChatCompletionRequest`
- [ ] Add unit tests for new fields (5 tests)
- [ ] Run `cargo test` - verify all pass
- [ ] Run `cargo build` - verify no errors

### Phase 2: Endpoint Pass-Through (2-3 hours)
- [ ] Add `proxy_passthrough` method to `ProxyHandler`
- [ ] Update `handle` method with endpoint routing
- [ ] Test `/props` endpoint through proxy (curl)
- [ ] Test `/slots` endpoint through proxy (curl)
- [ ] Test `/health` endpoint through proxy (curl)
- [ ] Test `/v1/models` endpoint through proxy (curl)
- [ ] Run `cargo test` - verify all pass

### Phase 3: Enhanced Metrics (2-3 hours)
- [ ] Add extended fields to `RequestMetrics` in `src/stats/collector.rs`
- [ ] Update `from_response` to extract `completion_tokens_details`
- [ ] Update `format_pretty` in `src/stats/formatter.rs`
- [ ] Update `format_json` in `src/stats/formatter.rs`
- [ ] Update InfluxDB exporter fields in `src/exporters/influxdb.rs`
- [ ] Add unit test for extended usage extraction
- [ ] Run `cargo test` - verify all pass

### Phase 4: Streaming Updates (1-2 hours)
- [ ] Update streaming.rs to handle `reasoning_text` accumulation
- [ ] Update streaming.rs to handle `reasoning_opaque` replacement
- [ ] Test streaming with simulated reasoning chunks
- [ ] Run `cargo test` - verify all pass

### Phase 5: Documentation (1-2 hours)
- [ ] Update `CLAUDE.md` with Opencode extension support
- [ ] Update `CLAUDE.md` with endpoint coverage
- [ ] Document reasoning field behavior (pass-through, not generated)
- [ ] Update compatibility matrix
- [ ] Add testing examples for both clients

### Phase 6: Integration Testing (2-4 hours)
- [ ] Test with Claude Code client (non-streaming)
- [ ] Test with Claude Code client (streaming)
- [ ] Test with Claude Code client (tool calls)
- [ ] Test with Opencode client (non-streaming)
- [ ] Test with Opencode client (streaming with reasoning params)
- [ ] Test with vanilla OpenAI client (curl)
- [ ] Test /props, /slots, /health endpoints
- [ ] Verify metrics logging includes extended fields when present
- [ ] Verify InfluxDB export (if configured)

### Phase 7: Final Validation
- [ ] Run full test suite: `cargo test`
- [ ] Run with debug logging: `RUST_LOG=debug cargo run -- run --config config.yaml`
- [ ] Check memory usage (no leaks from accumulation)
- [ ] Check latency overhead (<2ms per request)
- [ ] Code review for edge cases
- [ ] Update version in Cargo.toml (if releasing)

## Verification Checklist

### Compatibility Testing
- [ ] Claude Code: non-streaming chat ✓
- [ ] Claude Code: streaming chat ✓
- [ ] Claude Code: tool calls ✓
- [ ] Opencode: non-streaming chat ✓
- [ ] Opencode: streaming with reasoning_effort ✓
- [ ] Opencode: tool calls ✓
- [ ] Vanilla curl: basic requests ✓
- [ ] Unknown request fields preserved ✓
- [ ] Unknown response fields preserved ✓

### Metrics Validation
- [ ] Basic token counts extracted ✓
- [ ] Extended usage details tracked (if present) ✓
- [ ] Timings from llama.cpp used ✓
- [ ] Context percentage calculated ✓
- [ ] Reasoning tokens logged (if present) ✓

### Endpoint Coverage
- [ ] /v1/chat/completions works ✓
- [ ] /props accessible through proxy ✓
- [ ] /slots accessible through proxy ✓
- [ ] /health accessible through proxy ✓
- [ ] /v1/models accessible through proxy ✓

### Code Quality
- [ ] No clippy warnings: `cargo clippy`
- [ ] Code formatted: `cargo fmt`
- [ ] Test coverage ≥80%
- [ ] Documentation complete
- [ ] No TODOs in code

## Estimated Effort

**Total Time: 10-15 hours**

- Type extensions: 2-3 hours
- Endpoint routing: 2-3 hours
- Enhanced metrics: 2-3 hours
- Streaming updates: 1-2 hours
- Documentation: 1-2 hours
- Testing: 2-4 hours

Can be completed over 2-3 coding sessions.

## Risk Assessment

**All changes are LOW RISK:**

✅ Additive only (no breaking changes)
✅ Optional fields with defaults (backward compatible)
✅ Pass-through endpoints (no transformation)
✅ Existing tests continue to pass
✅ No changes to core proxy logic
✅ No changes to fix application logic

**Mitigation Strategies:**
- Run full test suite after each phase
- Test with real clients frequently
- Keep changes in separate commits for easy rollback
- Maintain existing behavior as fallback

## Success Criteria

1. **Both clients work perfectly:**
   - Claude Code: standard OpenAI compatibility ✓
   - Opencode: same + reasoning extensions preserved ✓
   - Vanilla clients: unaffected ✓

2. **Enhanced observability:**
   - Extended metrics tracked when available
   - All llama.cpp endpoints accessible
   - Better monitoring capabilities

3. **Maintainable architecture:**
   - Clean separation of concerns maintained
   - Type safety improved
   - Documentation up to date
   - Test coverage ≥80%

4. **Performance maintained:**
   - Latency overhead <2ms
   - No memory leaks
   - Connection pooling works

## Future Enhancements (Not in This Plan)

These are out of scope for now but documented for future consideration:

- Request validation layer (optional schema checking)
- Additional response fixes for other model issues
- Circuit breaker for backend failures
- Rate limiting support
- Multi-backend load balancing
- Request/response caching
- Authentication layer
- More exporters (Prometheus, Datadog, etc.)
- Graceful shutdown with task cleanup

These can be added incrementally as needs arise.
