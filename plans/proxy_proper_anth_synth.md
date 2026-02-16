# Option C: Proper Anthropic Messages API Streaming Synthesis

## Overview

This document describes how to implement **native Anthropic Messages API streaming synthesis** in the llama-proxy. Currently, the proxy converts Anthropic responses to OpenAI format before synthesis (Option A). Option C would implement proper Anthropic SSE streaming, maintaining the Anthropic format end-to-end.

## Motivation

**Why implement native Anthropic synthesis?**

1. **Format fidelity** - Preserves Anthropic-specific features (thinking blocks, signatures, etc.)
2. **Client compatibility** - Some clients may depend on exact Anthropic SSE format
3. **Performance** - Avoids unnecessary format conversion overhead
4. **Feature completeness** - Supports all Anthropic streaming features (deltas, event types)
5. **Future-proofing** - Ready for Anthropic-specific extensions

**When Option A is insufficient:**
- Client expects exact Anthropic SSE event sequence
- Thinking blocks need separate event chunks (not merged into text)
- Client relies on Anthropic-specific events (`message_start`, `message_delta`, `message_stop`)
- Signatures or other Anthropic metadata must be preserved

## Anthropic Messages API Streaming Format

### SSE Event Types

Based on llama.cpp implementation (`vendor/llama.cpp/tools/server/server-task.cpp:1073-1186`):

```
event: message_start
data: {"type": "message_start", "message": {...}}

event: content_block_start
data: {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}

event: content_block_delta
data: {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "chunk"}}

event: content_block_stop
data: {"type": "content_block_stop", "index": 0}

event: message_delta
data: {"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 10}}

event: message_stop
data: {"type": "message_stop"}
```

### Event Sequence

**For text-only response:**
```
1. message_start       - Initial message metadata
2. content_block_start - Start of content block
3. content_block_delta - Multiple chunks of text
4. content_block_delta
5. ...
6. content_block_stop  - End of content block
7. message_delta       - Final metadata (stop_reason, usage)
8. message_stop        - End of stream
```

**For response with thinking:**
```
1. message_start
2. content_block_start (index: 0, type: thinking)
3. content_block_delta (thinking chunks)
4. ...
5. content_block_stop  (index: 0)
6. content_block_start (index: 1, type: text)
7. content_block_delta (text chunks)
8. ...
9. content_block_stop  (index: 1)
10. message_delta
11. message_stop
```

### Message Structure

**message_start event:**
```json
{
  "type": "message_start",
  "message": {
    "id": "msg-123",
    "type": "message",
    "role": "assistant",
    "content": [],
    "model": "model-name",
    "stop_reason": null,
    "stop_sequence": null,
    "usage": {
      "input_tokens": 100,
      "output_tokens": 0
    }
  }
}
```

**content_block_start event:**
```json
{
  "type": "content_block_start",
  "index": 0,
  "content_block": {
    "type": "text",  // or "thinking"
    "text": ""       // empty initially
    // "thinking": "" if type is thinking
    // "signature": "" if type is thinking
  }
}
```

**content_block_delta event:**
```json
{
  "type": "content_block_delta",
  "index": 0,
  "delta": {
    "type": "text_delta",  // or "thinking_delta"
    "text": "chunk of text"
  }
}
```

**message_delta event (final):**
```json
{
  "type": "message_delta",
  "delta": {
    "stop_reason": "end_turn",  // or "max_tokens", "stop_sequence"
    "stop_sequence": null
  },
  "usage": {
    "output_tokens": 265
  }
}
```

## Implementation Design

### Architecture

```
                    ┌──────────────────────────────┐
                    │  Backend (llama-server)      │
                    │  /v1/messages?stream=false   │
                    └──────────────┬───────────────┘
                                   │ Complete JSON
                                   │ (Anthropic format)
                                   ▼
                    ┌──────────────────────────────┐
                    │  Anthropic Synthesis Module  │
                    │  - Parse complete message    │
                    │  - Extract content blocks    │
                    │  - Chunk text/thinking       │
                    │  - Generate SSE events       │
                    └──────────────┬───────────────┘
                                   │ SSE stream
                                   │ (Anthropic events)
                                   ▼
                    ┌──────────────────────────────┐
                    │  Client (Claude Code)        │
                    │  Accumulates deltas          │
                    └──────────────────────────────┘
```

### Module Structure

Create new module: `src/proxy/synthesis_anthropic.rs`

```rust
pub struct AnthropicSynthesizer {
    chunk_size: usize,  // Characters per chunk (default: 20)
}

impl AnthropicSynthesizer {
    /// Synthesize Anthropic SSE stream from complete message
    pub async fn synthesize(
        &self,
        message: AnthropicMessage,
    ) -> Result<Response, Error> {
        // Generate event stream
        let events = self.generate_events(message)?;

        // Convert to SSE format
        let stream = self.create_sse_stream(events);

        // Build response
        Ok(Response::builder()
            .header("Content-Type", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .body(Body::from_stream(stream))
            .unwrap())
    }

    fn generate_events(&self, message: AnthropicMessage) -> Vec<AnthropicEvent> {
        // ... implementation
    }

    fn create_sse_stream(&self, events: Vec<AnthropicEvent>) -> impl Stream {
        // ... implementation
    }
}
```

### Event Generation Algorithm

**Step 1: message_start event**
```rust
let message_start = AnthropicEvent::MessageStart {
    message: AnthropicMessageStart {
        id: message.id.clone(),
        message_type: "message".to_string(),
        role: message.role.clone(),
        content: vec![],  // Empty initially
        model: message.model.clone(),
        stop_reason: None,
        stop_sequence: None,
        usage: UsageStart {
            input_tokens: message.usage.input_tokens,
            output_tokens: 0,  // Zero initially
        },
    },
};
```

**Step 2: Iterate through content blocks**
```rust
for (index, content_block) in message.content.iter().enumerate() {
    // Emit content_block_start
    events.push(content_block_start_event(index, content_block));

    // Chunk and emit content_block_delta events
    let text = match content_block {
        AnthropicContentBlock::Text { text } => text,
        AnthropicContentBlock::Thinking { thinking, .. } => thinking,
    };

    for chunk in chunk_text(text, chunk_size) {
        events.push(content_block_delta_event(index, chunk, content_block.type()));
    }

    // Emit content_block_stop
    events.push(content_block_stop_event(index));
}
```

**Step 3: message_delta and message_stop**
```rust
// Final message delta with stop_reason and output_tokens
events.push(AnthropicEvent::MessageDelta {
    delta: MessageDelta {
        stop_reason: message.stop_reason.clone(),
        stop_sequence: message.stop_sequence.clone(),
    },
    usage: UsageDelta {
        output_tokens: message.usage.output_tokens,
    },
});

// End of stream
events.push(AnthropicEvent::MessageStop);
```

### Text Chunking

**Strategy:** Chunk on word boundaries when possible, fallback to character boundaries.

```rust
fn chunk_text(text: &str, chunk_size: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.len() + word.len() + 1 > chunk_size && !current.is_empty() {
            chunks.push(current.clone());
            current.clear();
        }

        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}
```

**Alternative:** Use time-based chunking (emit chunks at regular intervals) for more realistic streaming simulation.

### SSE Format

Each event must follow Server-Sent Events specification:

```
event: <event_type>\n
data: <json_payload>\n
\n
```

**Implementation:**
```rust
fn format_sse_event(event: &AnthropicEvent) -> String {
    let event_type = event.event_type();
    let data = serde_json::to_string(&event).unwrap();

    format!("event: {}\ndata: {}\n\n", event_type, data)
}
```

## Integration Points

### Handler Routing

**File:** `src/proxy/handler.rs` (synthesis section)

```rust
// Determine synthesis format
if client_wants_streaming {
    let synthesis_result = if is_anthropic_api {
        // Use native Anthropic synthesis
        match serde_json::from_value::<AnthropicMessage>(json.clone()) {
            Ok(msg) => {
                tracing::debug!("Synthesizing Anthropic streaming response");
                let synthesizer = AnthropicSynthesizer::new();
                synthesizer.synthesize(msg).await
            }
            Err(e) => {
                tracing::warn!(error = %e, "Cannot parse as AnthropicMessage");
                Err(anyhow!("Parse failed"))
            }
        }
    } else {
        // Use OpenAI synthesis (existing)
        match serde_json::from_value::<ChatCompletionResponse>(json.clone()) {
            Ok(response) => synthesize_streaming_response(response).await,
            Err(e) => {
                tracing::warn!(error = %e, "Cannot parse as ChatCompletionResponse");
                Err(anyhow!("Parse failed"))
            }
        }
    };

    match synthesis_result {
        Ok(response) => return response,
        Err(e) => {
            tracing::error!(error = %e, "Synthesis failed, falling back");
            // Fall through to non-streaming response
        }
    }
}
```

### Configuration

Add synthesis options to config:

```yaml
# config.yaml
synthesis:
  enabled: true

  # OpenAI synthesis options
  openai:
    chunk_size: 20  # Characters per chunk

  # Anthropic synthesis options
  anthropic:
    chunk_size: 20
    separate_thinking_blocks: true  # Emit thinking as separate content blocks
    include_signatures: false        # Include signature field in thinking blocks
```

**Config struct:**
```rust
#[derive(Debug, Deserialize)]
pub struct SynthesisConfig {
    pub enabled: bool,
    pub openai: OpenAISynthesisConfig,
    pub anthropic: AnthropicSynthesisConfig,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicSynthesisConfig {
    pub chunk_size: usize,
    pub separate_thinking_blocks: bool,
    pub include_signatures: bool,
}
```

## Type Definitions

### Event Types

```rust
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum AnthropicEvent {
    #[serde(rename = "message_start")]
    MessageStart {
        message: AnthropicMessageStart,
    },

    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: usize,
        content_block: ContentBlockStart,
    },

    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        index: usize,
        delta: ContentDelta,
    },

    #[serde(rename = "content_block_stop")]
    ContentBlockStop {
        index: usize,
    },

    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: MessageDelta,
        usage: UsageDelta,
    },

    #[serde(rename = "message_stop")]
    MessageStop,
}

impl AnthropicEvent {
    pub fn event_type(&self) -> &str {
        match self {
            Self::MessageStart { .. } => "message_start",
            Self::ContentBlockStart { .. } => "content_block_start",
            Self::ContentBlockDelta { .. } => "content_block_delta",
            Self::ContentBlockStop { .. } => "content_block_stop",
            Self::MessageDelta { .. } => "message_delta",
            Self::MessageStop => "message_stop",
        }
    }
}
```

### Delta Types

```rust
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum ContentDelta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },

    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
}

#[derive(Debug, Serialize)]
pub struct MessageDelta {
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UsageDelta {
    pub output_tokens: u32,
}
```

### Start Types

```rust
#[derive(Debug, Serialize)]
pub struct AnthropicMessageStart {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub role: String,
    pub content: Vec<serde_json::Value>,  // Empty initially
    pub model: String,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub usage: UsageStart,
}

#[derive(Debug, Serialize)]
pub struct UsageStart {
    pub input_tokens: u32,
    pub output_tokens: u32,  // Always 0 at start
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum ContentBlockStart {
    #[serde(rename = "text")]
    Text { text: String },  // Empty string

    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,  // Empty string
        signature: String,  // Empty or from original
    },
}
```

## Testing Strategy

### Unit Tests

**Test 1: Event generation**
```rust
#[test]
fn test_generate_anthropic_events() {
    let message = AnthropicMessage {
        id: "msg-123".to_string(),
        message_type: "message".to_string(),
        role: "assistant".to_string(),
        content: vec![
            AnthropicContentBlock::Text {
                text: "Hello world".to_string(),
            }
        ],
        model: "test-model".to_string(),
        stop_reason: Some("end_turn".to_string()),
        stop_sequence: None,
        usage: AnthropicUsage {
            input_tokens: 10,
            output_tokens: 5,
        },
    };

    let synthesizer = AnthropicSynthesizer::new();
    let events = synthesizer.generate_events(message);

    // Verify event sequence
    assert_eq!(events[0].event_type(), "message_start");
    assert_eq!(events[1].event_type(), "content_block_start");
    assert_eq!(events.last().unwrap().event_type(), "message_stop");
}
```

**Test 2: Text chunking**
```rust
#[test]
fn test_chunk_text_word_boundaries() {
    let text = "This is a test message";
    let chunks = chunk_text(text, 10);

    // Should split on word boundaries
    assert!(chunks.iter().all(|c| c.len() <= 10));
    assert_eq!(chunks.join(" "), text);
}
```

**Test 3: SSE formatting**
```rust
#[test]
fn test_sse_event_format() {
    let event = AnthropicEvent::MessageStop;
    let formatted = format_sse_event(&event);

    assert!(formatted.starts_with("event: message_stop\n"));
    assert!(formatted.ends_with("\n\n"));
}
```

### Integration Tests

**Test 1: Full synthesis flow**
```rust
#[tokio::test]
async fn test_anthropic_synthesis_end_to_end() {
    let message = create_test_anthropic_message();
    let synthesizer = AnthropicSynthesizer::new();

    let response = synthesizer.synthesize(message).await.unwrap();

    // Verify headers
    assert_eq!(
        response.headers().get("Content-Type").unwrap(),
        "text/event-stream"
    );

    // Collect stream and verify events
    let body_bytes = to_bytes(response.into_body()).await.unwrap();
    let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

    // Should contain all event types
    assert!(body_str.contains("event: message_start"));
    assert!(body_str.contains("event: content_block_start"));
    assert!(body_str.contains("event: content_block_delta"));
    assert!(body_str.contains("event: message_stop"));
}
```

**Test 2: Client accumulation**
```rust
#[tokio::test]
async fn test_client_side_accumulation_anthropic() {
    // Simulate client accumulating deltas
    let events = vec![
        content_block_delta("Hello"),
        content_block_delta(" world"),
    ];

    let mut accumulated = String::new();
    for event in events {
        if let AnthropicEvent::ContentBlockDelta { delta, .. } = event {
            if let ContentDelta::TextDelta { text } = delta {
                accumulated.push_str(&text);
            }
        }
    }

    assert_eq!(accumulated, "Hello world");
}
```

### Manual Testing

```bash
# Start proxy
RUST_LOG=debug cargo run -- run --config config.yaml

# Send Anthropic streaming request
curl -N -X POST http://localhost:8066/v1/messages \
  -H "Content-Type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -d '{
    "model": "test-model",
    "messages": [{"role": "user", "content": "Hello"}],
    "max_tokens": 100,
    "stream": true
  }'

# Expected output:
event: message_start
data: {"type":"message_start","message":{...}}

event: content_block_start
data: {"type":"content_block_start","index":0,...}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"chunk"}}

...

event: message_stop
data: {"type":"message_stop"}
```

## Performance Considerations

### Chunking Strategy

**Small chunks (10-20 chars):**
- ✅ More realistic streaming feel
- ✅ Lower latency to first byte
- ❌ More events = higher overhead
- ❌ More processing time

**Large chunks (50-100 chars):**
- ✅ Fewer events = lower overhead
- ✅ Faster total processing
- ❌ Less realistic streaming
- ❌ Higher latency to first byte

**Recommendation:** Start with 20 chars, make configurable.

### Memory Usage

**Concern:** Holding entire response in memory before streaming.

**Mitigation:**
- Already doing this for synthesis (unavoidable with stream:false backend)
- For very large responses, consider chunking the chunking (process content blocks incrementally)
- Monitor memory usage in production

### Network Efficiency

**SSE overhead:** Each event adds ~30-50 bytes of overhead (event type, data prefix, newlines).

**Calculation:**
- 1000 char response
- 20 chars per chunk = 50 chunks
- ~50 events total
- Overhead: ~2500 bytes (2.5x content size)

**Trade-off acceptable** for improved client experience.

## Edge Cases

### Empty Content

If `content` array is empty or all blocks have empty text:
- Still emit message_start
- Skip content_block events
- Emit message_delta and message_stop

```rust
if message.content.is_empty() {
    events.push(message_start);
    events.push(message_delta);
    events.push(message_stop);
    return events;
}
```

### Thinking Blocks with No Text

If thinking block has empty `thinking` field:
- Emit content_block_start (with empty thinking)
- Skip content_block_delta events
- Emit content_block_stop

### Multiple Content Blocks

Anthropic responses can have multiple content blocks (thinking + text, or multiple text blocks):
- Use `index` field to distinguish blocks
- Increment index for each block
- Emit complete sequence for each block

### Unicode and Multibyte Characters

**Risk:** Chunking might split multibyte UTF-8 characters.

**Solution:**
```rust
fn chunk_text_safe(text: &str, chunk_size: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for grapheme in text.graphemes(true) {  // Use unicode-segmentation crate
        if current.len() + grapheme.len() > chunk_size && !current.is_empty() {
            chunks.push(current.clone());
            current.clear();
        }
        current.push_str(grapheme);
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}
```

## Migration Path (from Option A to Option C)

### Phase 1: Implement alongside Option A
- Add `synthesis_anthropic.rs` module
- Keep Option A as default
- Add config flag: `anthropic.native_synthesis: false`

### Phase 2: Test with real clients
- Enable for specific endpoints or test users
- Monitor for issues
- Compare metrics (latency, throughput)

### Phase 3: Make default
- Set `native_synthesis: true` by default
- Keep Option A as fallback for compatibility

### Phase 4: Deprecate Option A (optional)
- Remove conversion logic
- Simplify code

## Future Enhancements

### Time-based Chunking
Instead of character-based, emit chunks at regular time intervals (50ms, 100ms) for more realistic streaming simulation.

### Adaptive Chunking
Adjust chunk size based on content type:
- Smaller chunks for code (feels more realistic)
- Larger chunks for prose (more efficient)

### Tool Calls Support
Anthropic Messages API supports tool use in content blocks. Extend synthesizer to handle:
```json
{
  "type": "tool_use",
  "id": "toolu_123",
  "name": "get_weather",
  "input": {}
}
```

### Caching
For repeated requests (e.g., during testing), cache synthesized event streams.

## Dependencies

**Required crates:**
- `tokio` (already present) - async runtime
- `futures` (already present) - stream utilities
- `serde` (already present) - serialization
- `serde_json` (already present) - JSON handling

**Optional crates:**
- `unicode-segmentation` - safe grapheme chunking
- `tokio-stream` - stream utilities (might already be present)

## References

- llama.cpp Anthropic implementation: `vendor/llama.cpp/tools/server/server-task.cpp:1073-1186`
- Anthropic Messages API docs: https://docs.anthropic.com/claude/reference/messages-streaming
- SSE specification: https://html.spec.whatwg.org/multipage/server-sent-events.html
- Existing OpenAI synthesis: `src/proxy/synthesis.rs`
- Format detection (inspiration): `src/proxy/streaming.rs:359-417`

## Estimated Effort

- **Implementation:** 4-6 hours
- **Testing:** 2-3 hours
- **Documentation:** 1 hour
- **Total:** ~8-10 hours

## Summary

Option C provides native Anthropic SSE streaming synthesis, maintaining format fidelity and supporting all Anthropic-specific features. While more complex than Option A (conversion approach), it offers:

1. **Full compatibility** with Anthropic Messages API clients
2. **Feature completeness** (thinking blocks, signatures, etc.)
3. **No conversion overhead**
4. **Future-proof** architecture

The implementation is straightforward: parse complete Anthropic message, chunk content, generate SSE events in proper sequence, and stream to client. With thorough testing and proper edge case handling, this provides a robust solution for Anthropic endpoint synthesis.
