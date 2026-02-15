# Fix Streaming Metrics Collection Race Condition

## Context

The llama-proxy currently has a critical bug in streaming metrics collection that causes incorrect/missing metrics to be logged. When clients (Opencode or Claude Code) send streaming requests, the proxy logs show:

```
DEBUG llama_proxy::stats::collector: No usage field found in response
DEBUG llama_proxy::stats::collector: No timings field found in response
model=unknown tokens=0/0 tps=0.0/0.0ms ctx:N/A stream finish=unknown
```

**Root Cause**: The metrics collection task uses an arbitrary 100ms sleep to "wait" for stream completion, causing a race condition where metrics are extracted before the stream finishes.

**Impact**:
- Metrics show 0 tokens/0 TPS (meaningless for monitoring)
- Model name appears as "unknown" (breaks per-model analytics)
- InfluxDB exports contain incorrect data (breaks dashboards/alerts)
- Affects both Opencode and Claude Code clients

**Why This Happens**:
1. Stream processor accumulates SSE chunks into a shared `Arc<Mutex<String>>`
2. Metrics task spawns immediately and sleeps for 100ms
3. After sleep, it locks the accumulated data and extracts metrics
4. BUT: Stream may not be complete yet (race condition)
5. llama.cpp sends usage/timings in the **final chunk** after all content
6. If metrics task reads before final chunk arrives, it gets partial data

**Technical Detail**:
- Streaming format: Multiple `data: {...}` lines, ending with `data: [DONE]`
- The code already detects `[DONE]` (line 70 in streaming.rs) but doesn't use it as a signal
- Instead, it uses sleep which is unreliable for fast/slow streams

## Solution Overview

Replace the 100ms sleep with proper async signaling using `tokio::sync::oneshot` channel:

1. **Create oneshot channel** when starting stream processing
2. **Send completion signal** when `data: [DONE]` is detected
3. **Wait for signal** in metrics task (instead of sleeping)
4. **Add timeout fallback** (30s) for edge cases (missing [DONE], connection errors)
5. **Make model field explicit** in chunk merging to prevent accidental loss

**Why This Fixes It**:
- No race condition: Metrics task waits for actual completion
- No arbitrary timing: Signal sent exactly when stream ends
- Faster for fast streams: No 100ms delay when stream completes in 10ms
- Reliable for slow streams: Waits as long as needed (up to 30s timeout)
- Better error handling: Timeout ensures metrics task doesn't hang forever

## Implementation Plan

### Step 1: Add Oneshot Channel Creation

**File**: `src/proxy/streaming.rs`
**Location**: Lines 38-40 (after accumulated mutex creation)

**Change**:
```rust
// Accumulate response for stats
let accumulated = Arc::new(tokio::sync::Mutex::new(String::new()));
let accumulated_clone = accumulated.clone();

// ADD: Create oneshot channel for stream completion signaling
let (completion_tx, completion_rx) = tokio::sync::oneshot::channel::<()>();
let completion_tx = Arc::new(tokio::sync::Mutex::new(Some(completion_tx)));
let completion_tx_clone = completion_tx.clone();
```

**Rationale**:
- `Arc<Mutex<Option<Sender>>>` allows moving sender into stream closure
- `Option` wrapper enables consuming sender via `take()` (can only signal once)
- Clone before moving into stream processor closure

### Step 2: Signal Completion on [DONE] Detection

**File**: `src/proxy/streaming.rs`
**Location**: Lines 70-76 (where [DONE] is currently detected)

**Current Code**:
```rust
if data == "[DONE]" {
    output.push_str(line);
    if i < lines.len() - 1 {
        output.push('\n');
    }
    continue;
}
```

**Change To**:
```rust
if data == "[DONE]" {
    // MODIFY: Signal completion to metrics task
    if stats_enabled {
        if let Some(tx) = completion_tx_clone.lock().await.take() {
            let _ = tx.send(()); // Ignore errors if receiver dropped
        }
    }

    output.push_str(line);
    if i < lines.len() - 1 {
        output.push('\n');
    }
    continue;
}
```

**Rationale**:
- Signal sent immediately when [DONE] detected
- `take()` consumes sender (prevents multiple signals)
- Send errors ignored (if metrics task dropped, no harm)
- Signal sent **before** appending output (ensures correct ordering)

### Step 3: Replace Sleep with Signal Wait in Metrics Task

**File**: `src/proxy/streaming.rs`
**Location**: Lines 116-130 (metrics task spawn)

**Current Code**:
```rust
tokio::spawn(async move {
    // Wait a bit for stream to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let acc = accumulated.lock().await;
    if !acc.is_empty() {
        // ... extract metrics
    }
});
```

**Change To**:
```rust
tokio::spawn(async move {
    // MODIFY: Wait for stream completion signal with timeout fallback
    let wait_result = tokio::time::timeout(
        tokio::time::Duration::from_secs(30),
        completion_rx
    ).await;

    match wait_result {
        Ok(Ok(())) => {
            tracing::debug!("Stream completion signal received");
        },
        Ok(Err(_)) => {
            tracing::warn!("Stream completion channel closed unexpectedly");
        },
        Err(_) => {
            tracing::warn!("Stream completion timeout after 30s, extracting metrics anyway");
        },
    }

    let acc = accumulated.lock().await;
    if !acc.is_empty() {
        // ... rest unchanged
    }
});
```

**Rationale**:
- `tokio::time::timeout` provides 30s safety fallback
- Handles edge cases: missing [DONE], connection errors, very slow streams
- Logging helps diagnose issues in production
- Metrics extraction proceeds regardless (best-effort)

### Step 4: Make Model and Timings Preservation Explicit

**File**: `src/proxy/streaming.rs`
**Location**: Lines 310-317 (end of `merge_chunk` function)

**Current Code**:
```rust
// Merge usage if present
if let Some(usage) = chunk.get("usage") {
    acc["usage"] = usage.clone();
}

acc
```

**Change To**:
```rust
// Merge usage if present
if let Some(usage) = chunk.get("usage") {
    acc["usage"] = usage.clone();
}

// ADD: Explicitly preserve model field (llama.cpp includes in first chunk)
if let Some(model) = chunk.get("model") {
    if !model.is_null() {
        acc["model"] = model.clone();
    }
}

// ADD: Preserve timings if present (llama.cpp extension, in final chunk)
if let Some(timings) = chunk.get("timings") {
    acc["timings"] = timings.clone();
}

acc
```

**Rationale**:
- Makes model field handling explicit (no longer relies on implicit preservation)
- Adds timings merging (llama.cpp sends in final chunk alongside usage)
- Follows same pattern as usage merging
- Last-chunk-wins strategy (later chunks override earlier ones)
- Defensive: checks for null before overwriting

## Edge Cases Handled

### Case 1: Stream Without [DONE] (Connection Drop)
- **Handling**: 30s timeout triggers metrics extraction anyway
- **Result**: Partial metrics better than none, logged as warning

### Case 2: Very Fast Streams (<10ms)
- **Handling**: Oneshot wakes immediately (no artificial 100ms delay)
- **Result**: Faster metrics logging, no unnecessary wait

### Case 3: Very Slow Streams (10+ seconds)
- **Handling**: Oneshot waits indefinitely within 30s timeout
- **Result**: Correct metrics even for large generations

### Case 4: Multiple [DONE] Markers (Malformed Response)
- **Handling**: `take()` ensures sender consumed on first signal
- **Result**: Only first signal reaches metrics task, subsequent attempts are no-ops

### Case 5: Metrics Disabled (stats_enabled = false)
- **Handling**: Signal sending wrapped in `if stats_enabled` check
- **Result**: No overhead when metrics disabled

## Testing Strategy

### Unit Tests (add to `src/proxy/streaming.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_chunk_preserves_model() {
        let chunk1 = json!({"model": "qwen3", "choices": []});
        let chunk2 = json!({"choices": [{"delta": {"content": "hi"}}]});

        let merged = merge_chunk(None, chunk1);
        let merged = merge_chunk(Some(merged), chunk2);

        assert_eq!(merged["model"].as_str().unwrap(), "qwen3");
    }

    #[test]
    fn test_merge_chunk_preserves_timings() {
        let chunk1 = json!({"model": "qwen3", "choices": []});
        let chunk2 = json!({
            "usage": {"prompt_tokens": 10},
            "timings": {"prompt_ms": 50.5}
        });

        let merged = merge_chunk(None, chunk1);
        let merged = merge_chunk(Some(merged), chunk2);

        assert!(merged.get("timings").is_some());
        assert_eq!(merged["timings"]["prompt_ms"].as_f64().unwrap(), 50.5);
    }

    #[test]
    fn test_parse_accumulated_sse_complete() {
        let sse = "data: {\"model\":\"qwen3\",\"choices\":[]}\n\
                   data: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\
                   data: [DONE]\n";

        let result = parse_accumulated_sse(sse).unwrap();
        assert_eq!(result["model"].as_str().unwrap(), "qwen3");
        assert_eq!(result["usage"]["prompt_tokens"].as_u64().unwrap(), 10);
    }
}
```

### Manual Integration Test

```bash
# 1. Start llama-proxy with debug logging
cargo run --release -- run --debug

# 2. In another terminal, send streaming request
curl -X POST http://localhost:8066/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"qwen3","messages":[{"role":"user","content":"Count to 5"}],"stream":true}'

# 3. Verify in proxy logs:
#    ✅ "Stream completion signal received" appears
#    ✅ Model name is correct (not "unknown")
#    ✅ Tokens are non-zero (e.g., "tokens=10/5")
#    ✅ TPS is non-zero (e.g., "tps=50.0")
#    ✅ No "No usage field found" or "No timings field found" warnings
```

### Verification Checklist

After implementation:
- [ ] Opencode client shows correct model name
- [ ] Opencode client shows non-zero tokens/TPS
- [ ] Claude Code client shows correct model name
- [ ] Claude Code client shows non-zero tokens/TPS
- [ ] Debug log shows "Stream completion signal received"
- [ ] No "No usage field found" warnings
- [ ] No "No timings field found" warnings
- [ ] Metrics exported to InfluxDB are correct
- [ ] Fast streams (<100ms) don't have artificial delay
- [ ] Slow streams (>100ms) still get correct metrics

## Critical Files

1. **`src/proxy/streaming.rs`** (Lines 20-320)
   - Main streaming handler with metrics race condition
   - Contains `handle_streaming_response`, `parse_accumulated_sse`, `merge_chunk`
   - All modifications happen in this file

2. **`src/stats/collector.rs`** (Lines 95-266)
   - Metrics extraction logic (`RequestMetrics::from_response`)
   - No changes needed, but verify it handles merged structure correctly
   - Already handles both OpenAI and Anthropic formats

3. **`src/api/openai.rs`**
   - Type definitions for OpenAI API (StreamChunk, Usage, etc.)
   - Reference for test fixtures
   - No changes needed

## Risks and Backward Compatibility

**Performance Impact**: Negligible
- One extra mutex lock per stream (on [DONE] detection)
- Lock held briefly (just to take() sender)
- Offset by removing 100ms sleep for fast streams

**API Compatibility**: No Breaking Changes
- External API unchanged (client requests/responses identical)
- Metrics format unchanged (same fields logged)
- Only internal timing of metrics collection improves

**Configuration**: No Changes Required
- No new config options needed
- Existing config.yaml works unchanged
- Could add timeout config in future if needed (default 30s is reasonable)

**Deployment**: Zero Downtime
- No database migrations
- No config changes required
- Simple cargo rebuild and restart

## Alternative Approaches Considered

1. **Increase sleep duration to 500ms or 1s**
   - Rejected: Doesn't solve race condition, just makes it less likely
   - Still adds artificial latency

2. **Parse metrics incrementally from each chunk**
   - Rejected: llama.cpp sends usage/timings in final chunk only
   - Would need buffering anyway, more complex

3. **Use broadcast channel instead of oneshot**
   - Rejected: Overkill for one-to-one signaling
   - Oneshot is simpler and more idiomatic

4. **Poll with AtomicBool**
   - Rejected: Not truly async, would still need sleep loop
   - Oneshot is more efficient

**Chosen Approach**: Oneshot channel is idiomatic Rust async, zero-overhead, correct semantics.

## Success Criteria

After implementation, the debug logs should show:

**Before (broken)**:
```
DEBUG llama_proxy::stats::collector: No usage field found in response
DEBUG llama_proxy::stats::collector: No timings field found in response
model=unknown tokens=0/0 tps=0.0/0.0ms ctx:N/A
```

**After (fixed)**:
```
DEBUG Stream completion signal received
model=Qwen3-14B-128K-Q3_K_S.gguf tokens=125/42 tps=850.5/750.2ms ctx:15.2%
```

This validates that:
- ✅ Completion signal works (stream fully processed before metrics)
- ✅ Model name extracted correctly (explicit preservation works)
- ✅ Token counts non-zero (usage from final chunk captured)
- ✅ TPS calculated correctly (timings from final chunk captured)
- ✅ Context usage available (background fetch had time to complete)
