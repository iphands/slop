# Fix Incomplete Stats Reporting in llama-proxy

## Context

The llama-proxy is currently showing incomplete stats for requests:
```
tokens=0/0
tps=0.0/0.0ms
ctx:N/A
finish=unknown
```

Instead of populated values like:
```
tokens=128/35
tps=1020.4/52.94ms
ctx:236/4096 (5.8%)
finish=stop
```

**Root Cause Analysis:**

1. **Streaming stats not collected**: The streaming handler in `src/proxy/streaming.rs` accumulates SSE data but never parses the final event to extract metrics. Helper functions `parse_accumulated_sse()` and `merge_chunk()` exist (lines 132-246) but are marked `#[allow(dead_code)]` and never invoked.

2. **Missing context_total**: `RequestMetrics::from_response()` extracts `context_used` from `timings.cache_n` but never populates `context_total` (n_ctx), leaving it as `None`.

3. **Context percentage never calculated**: The `calculate_context_percent()` helper exists but is never called after metrics extraction.

**How llama.cpp Sends Stats:**

From vendor exploration, llama.cpp streaming responses send:
- Multiple `data: {...}` events with partial deltas during generation
- A final complete event with full `usage` and `timings` objects (same format as non-streaming)
- A `data: [DONE]` marker at the end

The final streaming event contains the same fields as non-streaming responses, so we can reuse the existing `RequestMetrics::from_response()` extraction logic.

## Implementation Plan

### 1. Add Context Total Fetching with Caching

**File: `src/proxy/context.rs` (new file)**

Create a new module to fetch and cache the context size from the llama.cpp `/props` endpoint:

```rust
use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::sync::RwLock;

// Global cache: backend_url -> context_size
static CONTEXT_CACHE: OnceLock<RwLock<HashMap<String, u64>>> = OnceLock::new();

/// Fetch context total from backend /props endpoint with caching
pub async fn fetch_context_total(
    client: &reqwest::Client,
    backend_url: &str,
) -> Option<u64> {
    let cache = CONTEXT_CACHE.get_or_init(|| RwLock::new(HashMap::new()));

    // Check cache first
    {
        let read_guard = cache.read().await;
        if let Some(&ctx) = read_guard.get(backend_url) {
            return Some(ctx);
        }
    }

    // Fetch from /props endpoint
    let props_url = format!("{}/props", backend_url);
    match client.get(&props_url).send().await {
        Ok(resp) => {
            if let Ok(props) = resp.json::<serde_json::Value>().await {
                if let Some(n_ctx) = props
                    .get("default_generation_settings")
                    .and_then(|s| s.get("n_ctx"))
                    .and_then(|n| n.as_u64())
                {
                    // Cache for future requests
                    let mut write_guard = cache.write().await;
                    write_guard.insert(backend_url.to_string(), n_ctx);
                    return Some(n_ctx);
                }
            }
        }
        Err(e) => {
            tracing::debug!("Failed to fetch context size from {}: {}", props_url, e);
        }
    }

    None
}
```

**Rationale**: This approach fetches context size once per backend and caches it permanently. The `/props` endpoint provides the actual server configuration (n_ctx). Performance impact is negligible (~1-2ms on first request per backend).

### 2. Fix Streaming Stats Collection

**File: `src/proxy/streaming.rs`**

Modify the async task (lines 93-112) to extract and format metrics from the final SSE event:

**Changes needed:**
- Remove `#[allow(dead_code)]` from `parse_accumulated_sse()` (line 132)
- Pass `stats_format` configuration to `handle_streaming_response()`
- Pass `backend_url` and `http_client` for context fetching
- Modify the spawned task to:
  1. Call `parse_accumulated_sse()` to extract final event
  2. Call `RequestMetrics::from_response()` with the final event
  3. Fetch and set `context_total`
  4. Calculate `context_percent`
  5. Format and log stats (reuse `format_metrics()`)
  6. Export to remote systems

**Updated async task logic:**
```rust
// After line 94, modify the spawned task:
if stats_enabled {
    let exporter_manager = exporter_manager.clone();
    let client = http_client.clone();
    let backend_url = backend_url.to_string();
    let stats_format = stats_format;

    tokio::spawn(async move {
        // Wait briefly for stream to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let acc = accumulated.lock().await;
        if !acc.is_empty() {
            // Parse final SSE event
            if let Some(final_event) = parse_accumulated_sse(&acc) {
                // Extract metrics from final event
                let mut metrics = if let Some(ref req_json) = request_json {
                    RequestMetrics::from_response(
                        &final_event,
                        req_json,
                        true, // streaming
                        start.elapsed().as_millis() as f64,
                    )
                } else {
                    return;
                };

                // Fetch and set context_total
                if let Some(ctx_total) = fetch_context_total(&client, &backend_url).await {
                    metrics.context_total = Some(ctx_total);
                    metrics.calculate_context_percent();
                }

                // Format and log stats
                let formatted = format_metrics(&metrics, stats_format);
                tracing::info!("\n{}", formatted);

                // Export to remote systems
                exporter_manager.export_all(&metrics).await;
            } else {
                tracing::debug!(
                    duration_ms = start.elapsed().as_millis() as u64,
                    "Streaming completed (unable to parse final event)"
                );
            }
        }
    });
}
```

**Function signature changes:**
- Add parameters: `stats_format: StatsFormat`, `http_client: reqwest::Client`, `backend_url: String`

### 3. Update Non-Streaming Path

**File: `src/proxy/handler.rs`**

Add context fetching after metrics extraction (around line 152-157):

```rust
// After creating metrics with from_response():
let mut metrics = RequestMetrics::from_response(
    &json,
    req_json,
    is_streaming_request,
    start.elapsed().as_millis() as f64,
);

// NEW: Fetch and set context_total
if let Some(ctx_total) = fetch_context_total(
    &self.state.http_client,
    &self.state.config.backend.url(),
).await {
    metrics.context_total = Some(ctx_total);
    metrics.calculate_context_percent();
}
```

### 4. Update Module Structure

**File: `src/proxy/mod.rs`**

Add the new context module:
```rust
mod context;
pub use context::fetch_context_total;
```

**File: `src/proxy/server.rs`**

Update the `handle_streaming_response()` call (around line 98-105) to pass additional parameters:
```rust
handle_streaming_response(
    backend_response,
    self.state.fix_registry.clone(),
    self.state.config.stats.enabled,
    self.state.config.stats.format,  // NEW
    self.state.exporter_manager.clone(),
    request_json,
    start,
    self.state.http_client.clone(),  // NEW
    self.state.config.backend.url().to_string(),  // NEW
)
.await
```

## Critical Files to Modify

1. **src/proxy/context.rs** (NEW) - Context size fetching with caching
2. **src/proxy/streaming.rs** - Fix streaming stats extraction (lines 17-112)
3. **src/proxy/handler.rs** - Add context fetching to non-streaming path (lines 150-163)
4. **src/proxy/mod.rs** - Export new context module
5. **src/main.rs** - May need import updates

## Verification Plan

### Manual Testing

**Test non-streaming request:**
```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "llama",
    "messages": [{"role": "user", "content": "Hello"}],
    "stream": false
  }'
```

**Test streaming request:**
```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "llama",
    "messages": [{"role": "user", "content": "Hello"}],
    "stream": true
  }'
```

**Expected output** (in logs):
```
tokens=128/35
tps=1020.4/52.94ms
ctx:236/4096 (5.8%)
finish=stop
dur=786ms
```

### Error Cases to Test

1. **Backend /props unavailable**: Stats should still work, just without context percentage
2. **Malformed final SSE event**: Should log completion without stats (graceful degradation)
3. **Multiple backends**: Each should cache its own context size independently

### Unit Tests

Run existing tests to ensure no regressions:
```bash
cargo test
```

Consider adding test for `parse_accumulated_sse()` with real llama.cpp SSE format.

## Implementation Notes

- **Performance**: Minimal impact - one cached HTTP call per backend
- **Error handling**: Graceful degradation - missing stats shouldn't break proxying
- **Code reuse**: Maximizes use of existing helpers (`parse_accumulated_sse`, `from_response`, `format_metrics`)
- **Consistency**: Both streaming and non-streaming paths now format stats identically

## Alternative Approaches Considered

1. **Extract context from response JSON**: llama.cpp doesn't include n_ctx in responses ❌
2. **Call /slots per request**: Too expensive, adds latency to every request ❌
3. **Store in config file**: Requires manual maintenance, can drift from actual server config ⚠️

The `/props` endpoint with caching provides the best balance of accuracy, performance, and automation.
