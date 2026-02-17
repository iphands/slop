# OpenAI ↔ Anthropic Format Conversion Guide

## Overview

This document provides comprehensive guidance for converting between OpenAI Chat Completions format and Anthropic Messages format. Understanding these mappings is critical for building proxies, format converters, and multi-provider AI applications.

**Key Insight**: The two formats have **different structural philosophies**:
- **OpenAI**: Message-centric with role-based structure
- **Anthropic**: Content-block-centric with flexible composition

## Message Envelope Comparison

### OpenAI Chat Completion Response

```json
{
  "id": "chatcmpl-abc123",
  "object": "chat.completion",
  "created": 1234567890,
  "model": "gpt-4",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": "Hello! How can I help?",
        "reasoning_text": "The user greeted me...",
        "tool_calls": []
      },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 10,
    "completion_tokens": 20,
    "total_tokens": 30
  }
}
```

### Anthropic Message Response

```json
{
  "id": "msg_01AbCdEfGhIjKlMnOpQrStUv",
  "type": "message",
  "role": "assistant",
  "content": [
    {
      "type": "thinking",
      "thinking": "The user greeted me..."
    },
    {
      "type": "text",
      "text": "Hello! How can I help?"
    }
  ],
  "model": "claude-sonnet-4-5-20250929",
  "stop_reason": "end_turn",
  "usage": {
    "input_tokens": 10,
    "output_tokens": 20
  }
}
```

### Key Structural Differences

| Aspect | OpenAI | Anthropic |
|--------|--------|-----------|
| Top-level wrapper | `choices[]` array | Direct message object |
| Content structure | Flat fields on message | Array of content blocks |
| Multiple content types | Multiple fields (`content`, `reasoning_text`, `tool_calls`) | Multiple content blocks in array |
| Role specification | Per-message `role` field | Top-level `role` field |
| Completion reason | `finish_reason` | `stop_reason` |
| Token counting | `prompt_tokens`, `completion_tokens`, `total_tokens` | `input_tokens`, `output_tokens` |

## Direct Mappings (Can Convert Both Ways)

### 1. Text Content

**Anthropic → OpenAI**:

```json
// Anthropic
{
  "content": [
    {"type": "text", "text": "Hello, world!"}
  ]
}

// OpenAI
{
  "message": {
    "content": "Hello, world!"
  }
}
```

**Conversion Logic**:
```rust
// Anthropic → OpenAI
fn to_openai(content: Vec<ContentBlock>) -> String {
    content.iter()
        .filter_map(|block| {
            if let ContentBlock::Text { text } = block {
                Some(text.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// OpenAI → Anthropic
fn to_anthropic(content: String) -> Vec<ContentBlock> {
    vec![ContentBlock::Text { text: content }]
}
```

**Edge Cases**:
- Empty string: Convert to empty content array or single empty text block
- Null content: Invalid in both formats
- Multiple text blocks: Concatenate with newlines for OpenAI

### 2. Thinking/Reasoning Content

**Anthropic → OpenAI**:

```json
// Anthropic
{
  "content": [
    {
      "type": "thinking",
      "thinking": "Let me analyze this...",
      "signature": "sig_abc123"
    }
  ]
}

// OpenAI
{
  "message": {
    "reasoning_text": "Let me analyze this..."
  }
}
```

**Conversion Logic**:
```rust
// Anthropic → OpenAI
fn thinking_to_openai(thinking_blocks: Vec<ThinkingBlock>) -> Option<String> {
    let reasoning = thinking_blocks.iter()
        .map(|block| block.thinking.clone())
        .collect::<Vec<_>>()
        .join("\n\n");

    if reasoning.is_empty() {
        None
    } else {
        Some(reasoning)
    }
}

// OpenAI → Anthropic
fn reasoning_to_anthropic(reasoning: String) -> ContentBlock {
    ContentBlock::Thinking {
        thinking: reasoning,
        signature: None  // No signature from OpenAI
    }
}
```

**Edge Cases**:
- **Signature field**: Anthropic-only, dropped in conversion to OpenAI
- **Multiple thinking blocks**: Concatenate with double newlines
- **Empty thinking**: Omit from OpenAI message

**Important Note**: The `signature` field is Anthropic-specific and has no OpenAI equivalent. When converting Anthropic → OpenAI, this field is lost. When converting back (round-trip), signatures cannot be preserved.

### 3. Tool Calling

**Anthropic → OpenAI**:

```json
// Anthropic
{
  "content": [
    {
      "type": "tool_use",
      "id": "toolu_01AbCdEfGhIjKlMnOpQrStUv",
      "name": "get_weather",
      "input": {
        "location": "San Francisco",
        "unit": "fahrenheit"
      }
    }
  ],
  "stop_reason": "tool_use"
}

// OpenAI
{
  "message": {
    "tool_calls": [
      {
        "id": "toolu_01AbCdEfGhIjKlMnOpQrStUv",
        "type": "function",
        "function": {
          "name": "get_weather",
          "arguments": "{\"location\":\"San Francisco\",\"unit\":\"fahrenheit\"}"
        }
      }
    ]
  },
  "finish_reason": "tool_calls"
}
```

**Conversion Logic**:
```rust
// Anthropic → OpenAI
fn tool_use_to_openai(tool_use: ToolUseBlock) -> ToolCall {
    ToolCall {
        id: tool_use.id,
        type_: "function".to_string(),
        function: FunctionCall {
            name: tool_use.name,
            arguments: serde_json::to_string(&tool_use.input).unwrap()
        }
    }
}

// OpenAI → Anthropic
fn tool_call_to_anthropic(tool_call: ToolCall) -> ContentBlock {
    ContentBlock::ToolUse {
        id: tool_call.id,
        name: tool_call.function.name,
        input: serde_json::from_str(&tool_call.function.arguments).unwrap()
    }
}
```

**Edge Cases**:
- **ID generation**: If OpenAI doesn't provide ID, generate with `toolu_` prefix
- **Arguments parsing**: OpenAI uses JSON string, Anthropic uses JSON object
- **Invalid JSON in arguments**: Handle parse errors gracefully
- **Multiple tool calls**: Both formats support arrays

**Important**:
- OpenAI's `arguments` is a JSON **string**
- Anthropic's `input` is a JSON **object**
- Must serialize/deserialize during conversion

### 4. Image Content (Multimodal)

**Anthropic → OpenAI**:

```json
// Anthropic (URL source)
{
  "content": [
    {
      "type": "image",
      "source": {
        "type": "url",
        "url": "https://example.com/image.jpg"
      }
    }
  ]
}

// OpenAI
{
  "content": [
    {
      "type": "image_url",
      "image_url": {
        "url": "https://example.com/image.jpg"
      }
    }
  ]
}
```

**Anthropic → OpenAI (base64)**:

```json
// Anthropic (base64 source)
{
  "content": [
    {
      "type": "image",
      "source": {
        "type": "base64",
        "media_type": "image/jpeg",
        "data": "iVBORw0KGgo..."
      }
    }
  ]
}

// OpenAI (converted to data URI)
{
  "content": [
    {
      "type": "image_url",
      "image_url": {
        "url": "data:image/jpeg;base64,iVBORw0KGgo..."
      }
    }
  ]
}
```

**Conversion Logic**:
```rust
// Anthropic → OpenAI
fn image_to_openai(image: ImageBlock) -> ImageUrl {
    let url = match image.source.type_ {
        "url" => image.source.url.unwrap(),
        "base64" => {
            format!(
                "data:{};base64,{}",
                image.source.media_type.unwrap(),
                image.source.data.unwrap()
            )
        }
        _ => panic!("Unknown image source type")
    };

    ImageUrl { url }
}

// OpenAI → Anthropic
fn image_url_to_anthropic(image_url: ImageUrl) -> ContentBlock {
    if image_url.url.starts_with("data:") {
        // Parse data URI
        let parts: Vec<&str> = image_url.url.split(',').collect();
        let media_type = parts[0]
            .strip_prefix("data:")
            .unwrap()
            .strip_suffix(";base64")
            .unwrap();

        ContentBlock::Image {
            source: ImageSource {
                type_: "base64".to_string(),
                media_type: Some(media_type.to_string()),
                data: Some(parts[1].to_string()),
                url: None
            }
        }
    } else {
        // Plain URL
        ContentBlock::Image {
            source: ImageSource {
                type_: "url".to_string(),
                url: Some(image_url.url),
                media_type: None,
                data: None
            }
        }
    }
}
```

**Edge Cases**:
- **Data URI parsing**: Must handle malformed data URIs
- **Media type detection**: Extract from data URI or guess from URL extension
- **Image size limits**: Both APIs have limits, may differ
- **Unsupported formats**: Handle format mismatches

## Requires Transformation (Not Direct Mapping)

### 1. Tool Results

Tool results have **fundamentally different structures**:

**Anthropic**: Content block within message

```json
{
  "role": "user",
  "content": [
    {
      "type": "tool_result",
      "tool_use_id": "toolu_01ABC",
      "content": "Temperature: 72°F",
      "is_error": false
    }
  ]
}
```

**OpenAI**: Separate message with `role: "tool"`

```json
{
  "role": "tool",
  "tool_call_id": "toolu_01ABC",
  "content": "Temperature: 72°F"
}
```

**Conversion Strategy**:

```rust
// Anthropic → OpenAI: Extract tool_result blocks into separate messages
fn convert_anthropic_to_openai(messages: Vec<AnthropicMessage>) -> Vec<OpenAIMessage> {
    let mut result = Vec::new();

    for msg in messages {
        let mut tool_results = Vec::new();
        let mut other_content = Vec::new();

        for block in msg.content {
            match block {
                ContentBlock::ToolResult { tool_use_id, content, is_error } => {
                    tool_results.push(OpenAIMessage {
                        role: "tool".to_string(),
                        tool_call_id: Some(tool_use_id),
                        content: content,
                        // If is_error, could prefix content with "Error: "
                    });
                }
                other => other_content.push(other)
            }
        }

        // Add main message if it has non-tool-result content
        if !other_content.is_empty() {
            result.push(convert_content_to_openai(msg.role, other_content));
        }

        // Add tool result messages
        result.extend(tool_results);
    }

    result
}

// OpenAI → Anthropic: Merge tool messages into previous user message
fn convert_openai_to_anthropic(messages: Vec<OpenAIMessage>) -> Vec<AnthropicMessage> {
    let mut result = Vec::new();
    let mut pending_tool_results = Vec::new();

    for msg in messages {
        if msg.role == "tool" {
            pending_tool_results.push(ContentBlock::ToolResult {
                tool_use_id: msg.tool_call_id.unwrap(),
                content: msg.content,
                is_error: None  // OpenAI doesn't have explicit error flag
            });
        } else {
            // If we have pending tool results, add them to this message
            if !pending_tool_results.is_empty() {
                let mut content = pending_tool_results.clone();
                content.extend(convert_openai_content(msg.content));

                result.push(AnthropicMessage {
                    role: msg.role,
                    content
                });

                pending_tool_results.clear();
            } else {
                result.push(convert_openai_message(msg));
            }
        }
    }

    result
}
```

**Edge Cases**:
- **Orphaned tool results**: OpenAI tool messages with no following user message
- **Error indication**: OpenAI lacks `is_error` field; could parse from content
- **Ordering**: Tool messages must follow assistant message with tool_calls
- **Multiple results**: Group all consecutive tool messages together

### 2. System Messages

**OpenAI**: System messages are regular messages with `role: "system"`

```json
{
  "messages": [
    {
      "role": "system",
      "content": "You are a helpful assistant."
    },
    {
      "role": "user",
      "content": "Hello!"
    }
  ]
}
```

**Anthropic**: System is a top-level parameter, not a message

```json
{
  "system": "You are a helpful assistant.",
  "messages": [
    {
      "role": "user",
      "content": [{"type": "text", "text": "Hello!"}]
    }
  ]
}
```

**Conversion Strategy**:

```rust
// OpenAI → Anthropic: Extract system messages
fn extract_system(messages: Vec<OpenAIMessage>) -> (Option<String>, Vec<OpenAIMessage>) {
    let system_messages: Vec<String> = messages.iter()
        .filter(|m| m.role == "system")
        .map(|m| m.content.clone())
        .collect();

    let non_system: Vec<OpenAIMessage> = messages.into_iter()
        .filter(|m| m.role != "system")
        .collect();

    let system = if system_messages.is_empty() {
        None
    } else {
        Some(system_messages.join("\n\n"))
    };

    (system, non_system)
}

// Anthropic → OpenAI: Inject system as first message
fn inject_system(system: Option<String>, messages: Vec<OpenAIMessage>) -> Vec<OpenAIMessage> {
    if let Some(sys) = system {
        let mut result = vec![OpenAIMessage {
            role: "system".to_string(),
            content: sys,
            ..Default::default()
        }];
        result.extend(messages);
        result
    } else {
        messages
    }
}
```

**Edge Cases**:
- **Multiple system messages**: Concatenate with newlines
- **System message in middle**: OpenAI allows, Anthropic doesn't (must be top-level)
- **Empty system**: Omit entirely

### 3. Multiple Content Blocks to Flat Fields

**Anthropic**: Can have mixed content blocks

```json
{
  "content": [
    {"type": "thinking", "thinking": "Let me think..."},
    {"type": "text", "text": "Here's my answer"},
    {"type": "tool_use", "id": "toolu_01", "name": "search", "input": {}}
  ]
}
```

**OpenAI**: Separate fields

```json
{
  "message": {
    "reasoning_text": "Let me think...",
    "content": "Here's my answer",
    "tool_calls": [
      {"id": "toolu_01", "function": {"name": "search", "arguments": "{}"}}
    ]
  }
}
```

**Conversion Strategy**:

```rust
// Anthropic → OpenAI: Separate by type
fn separate_content_blocks(content: Vec<ContentBlock>) -> OpenAIMessage {
    let mut reasoning_parts = Vec::new();
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in content {
        match block {
            ContentBlock::Thinking { thinking, .. } => {
                reasoning_parts.push(thinking);
            }
            ContentBlock::Text { text } => {
                text_parts.push(text);
            }
            ContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(ToolCall {
                    id,
                    type_: "function".to_string(),
                    function: FunctionCall {
                        name,
                        arguments: serde_json::to_string(&input).unwrap()
                    }
                });
            }
            _ => {}
        }
    }

    OpenAIMessage {
        role: "assistant".to_string(),
        reasoning_text: if reasoning_parts.is_empty() {
            None
        } else {
            Some(reasoning_parts.join("\n\n"))
        },
        content: text_parts.join("\n"),
        tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
        ..Default::default()
    }
}

// OpenAI → Anthropic: Combine into content array
fn combine_to_content_blocks(message: OpenAIMessage) -> Vec<ContentBlock> {
    let mut content = Vec::new();

    // Add reasoning first (if present)
    if let Some(reasoning) = message.reasoning_text {
        content.push(ContentBlock::Thinking {
            thinking: reasoning,
            signature: None
        });
    }

    // Add text content
    if !message.content.is_empty() {
        content.push(ContentBlock::Text {
            text: message.content
        });
    }

    // Add tool calls
    if let Some(tool_calls) = message.tool_calls {
        for tc in tool_calls {
            content.push(ContentBlock::ToolUse {
                id: tc.id,
                name: tc.function.name,
                input: serde_json::from_str(&tc.function.arguments).unwrap()
            });
        }
    }

    content
}
```

**Edge Cases**:
- **Empty content array**: Create empty string for OpenAI
- **Order preservation**: Anthropic has explicit order, OpenAI has implicit (reasoning → content → tool_calls)
- **Interleaved content**: Anthropic allows `[text, tool_use, text]`; OpenAI cannot represent this

## Cannot Map (No Equivalent)

### Anthropic → OpenAI (No OpenAI Equivalent)

#### 1. Document Blocks

```json
{
  "type": "document",
  "source": {
    "type": "base64",
    "media_type": "application/pdf",
    "data": "..."
  },
  "title": "Report",
  "context": "Annual report"
}
```

**Why**: OpenAI has no document processing API
**Workaround**: Extract text from PDF externally, send as text content
**Impact**: Loss of structured document understanding

#### 2. Search Result Blocks

```json
{
  "type": "search_result",
  "source": "Google",
  "title": "Article Title",
  "content": "Article snippet..."
}
```

**Why**: OpenAI has no search_result content type
**Workaround**: Format as text: `"From {source}: {title}\n{content}"`
**Impact**: Loss of structured search metadata

#### 3. Server-Side Tools

```json
{
  "type": "server_tool_use",
  "id": "toolu_01",
  "name": "web_search",
  "input": {"query": "..."}
}
```

```json
{
  "type": "web_search_tool_result",
  "tool_use_id": "toolu_01",
  "content": "Results..."
}
```

**Why**: OpenAI has no server-side tool execution
**Workaround**: Not applicable (feature doesn't exist in OpenAI)
**Impact**: Cannot use Anthropic's built-in web search

#### 4. Redacted Thinking

```json
{
  "type": "redacted_thinking",
  "data": "base64_redacted_content"
}
```

**Why**: OpenAI has no content redaction
**Workaround**: Omit entirely or replace with placeholder
**Impact**: Loss of (redacted) reasoning context

#### 5. Citations

```json
{
  "type": "text",
  "text": "Revenue increased 25%",
  "citations": [
    {
      "type": "citation",
      "source": {"document_id": "doc_123", "page": 42},
      "text": "Revenue increased 25%",
      "start": 0,
      "end": 23
    }
  ]
}
```

**Why**: OpenAI has no citation tracking
**Workaround**: Include citations as footnotes in text
**Impact**: Loss of structured source attribution

#### 6. Cache Control

```json
{
  "type": "text",
  "text": "Long prompt...",
  "cache_control": {"type": "ephemeral"}
}
```

**Why**: OpenAI has no prompt caching API
**Workaround**: Field is simply ignored
**Impact**: No caching optimization

### OpenAI → Anthropic (No Anthropic Equivalent)

#### 1. Function Calling (Legacy)

OpenAI has legacy `functions` parameter (deprecated in favor of `tools`):

```json
{
  "functions": [
    {
      "name": "get_weather",
      "description": "Get weather",
      "parameters": {...}
    }
  ],
  "function_call": "auto"
}
```

**Workaround**: Convert to `tools` format first, then map to Anthropic tools
**Impact**: Extra conversion step required

#### 2. Logprobs

OpenAI can return token probabilities:

```json
{
  "logprobs": {
    "content": [
      {
        "token": "Hello",
        "logprob": -0.5,
        "top_logprobs": [...]
      }
    ]
  }
}
```

**Why**: Anthropic doesn't expose token probabilities
**Workaround**: Not applicable
**Impact**: Cannot get probability information from Anthropic

## Stop Reason Mappings

| Anthropic | OpenAI | Notes |
|-----------|--------|-------|
| `end_turn` | `stop` | Natural completion |
| `max_tokens` | `length` | Hit token limit |
| `stop_sequence` | `stop` | Hit stop sequence |
| `tool_use` | `tool_calls` | Model wants to call tools |
| `null` | `null` | During streaming, not yet determined |

**Conversion Logic**:

```rust
fn anthropic_to_openai_stop_reason(reason: Option<String>) -> Option<String> {
    reason.map(|r| match r.as_str() {
        "end_turn" => "stop",
        "max_tokens" => "length",
        "stop_sequence" => "stop",
        "tool_use" => "tool_calls",
        other => other  // Pass through unknown
    }.to_string())
}

fn openai_to_anthropic_stop_reason(reason: Option<String>) -> Option<String> {
    reason.map(|r| match r.as_str() {
        "stop" => "end_turn",  // Assumption: most stops are natural
        "length" => "max_tokens",
        "tool_calls" => "tool_use",
        other => other
    }.to_string())
}
```

**Edge Case**: OpenAI `stop` can mean both `end_turn` and `stop_sequence`. Without additional context, assume `end_turn`.

## Token Usage Mapping

**OpenAI**:
```json
{
  "usage": {
    "prompt_tokens": 10,
    "completion_tokens": 20,
    "total_tokens": 30
  }
}
```

**Anthropic**:
```json
{
  "usage": {
    "input_tokens": 10,
    "output_tokens": 20
  }
}
```

**Conversion**:
```rust
// OpenAI → Anthropic
fn openai_to_anthropic_usage(usage: OpenAIUsage) -> AnthropicUsage {
    AnthropicUsage {
        input_tokens: usage.prompt_tokens,
        output_tokens: usage.completion_tokens
    }
}

// Anthropic → OpenAI
fn anthropic_to_openai_usage(usage: AnthropicUsage) -> OpenAIUsage {
    OpenAIUsage {
        prompt_tokens: usage.input_tokens,
        completion_tokens: usage.output_tokens,
        total_tokens: usage.input_tokens + usage.output_tokens
    }
}
```

**Note**: Anthropic doesn't provide `total_tokens`, must be calculated.

## Streaming Conversion

### Event Type Mappings

| Anthropic Event | OpenAI Equivalent | Notes |
|-----------------|-------------------|-------|
| `message_start` | First `chunk` with empty delta | Contains initial metadata |
| `content_block_start` | N/A | OpenAI doesn't signal block boundaries |
| `content_block_delta` | `chunk` with delta | Actual content increments |
| `content_block_stop` | N/A | OpenAI doesn't signal block boundaries |
| `message_delta` | `chunk` with `finish_reason` | Final metadata |
| `message_stop` | `chunk` with `[DONE]` | Stream termination |

### Streaming Text

**Anthropic**:
```json
{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}
{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}
{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world"}}
{"type":"content_block_stop","index":0}
```

**OpenAI**:
```json
{"choices":[{"index":0,"delta":{"content":"Hello"}}]}
{"choices":[{"index":0,"delta":{"content":" world"}}]}
{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}
```

**Conversion Strategy**:
- Skip `content_block_start` and `content_block_stop` when converting to OpenAI
- Map `text_delta` → `delta.content`
- Synthesize `content_block_start`/`stop` when converting to Anthropic

### Streaming Thinking

**Anthropic**:
```json
{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}
{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me"}}
{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":" think"}}
{"type":"content_block_stop","index":0}
```

**OpenAI**:
```json
{"choices":[{"index":0,"delta":{"reasoning_text":"Let me"}}]}
{"choices":[{"index":0,"delta":{"reasoning_text":" think"}}]}
```

**Note**: OpenAI doesn't have incremental reasoning updates in official API; this is based on extended thinking models.

### Streaming Tool Calls

**Anthropic**:
```json
{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_01","name":"search","input":{}}}
{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"qu"}}
{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"ery\":"}}
{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"test\"}"}}
{"type":"content_block_stop","index":0}
```

**OpenAI**:
```json
{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"toolu_01","function":{"name":"search"}}]}}]}
{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"qu"}}]}}]}
{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ery\":"}}]}}]}
{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"test\"}"}}]}}]}
```

**Conversion Complexity**:
- Anthropic sends full tool metadata in `content_block_start`
- OpenAI sends metadata in first delta
- Must track state to properly synthesize events

## Empty Content Handling

### Empty Anthropic Content

```json
{
  "content": []
}
```

**Convert to OpenAI**:
```json
{
  "message": {
    "content": ""
  }
}
```

### Empty OpenAI Content

```json
{
  "message": {
    "content": "",
    "tool_calls": null
  }
}
```

**Convert to Anthropic**:
```json
{
  "content": []
}
```

**Or**:
```json
{
  "content": [
    {"type": "text", "text": ""}
  ]
}
```

**Recommendation**: Use empty array rather than array with empty text block.

## Round-Trip Conversion Caveats

### Information Loss (Anthropic → OpenAI → Anthropic)

**Lost**:
- `signature` field in thinking blocks
- Content block ordering (if interleaved)
- `citations` metadata
- `cache_control` directives
- Document/search result structure

**Preserved**:
- Text content
- Thinking/reasoning content
- Tool calls (id, name, input)
- Stop reasons
- Token counts

### Information Loss (OpenAI → Anthropic → OpenAI)

**Lost**:
- `total_tokens` (must recalculate)
- Legacy `function_call` format
- `logprobs` data
- Multiple system messages (merged into one)

**Preserved**:
- All message content
- Tool calls
- Reasoning text
- Stop reasons

## Best Practices for Conversion

### 1. Preserve IDs

Always preserve tool call IDs when converting:
- Anthropic `tool_use.id` ↔ OpenAI `tool_calls[].id`
- Anthropic `tool_result.tool_use_id` ↔ OpenAI `tool_call_id`

**Why**: Breaks multi-turn tool calling if IDs don't match

### 2. Handle Missing IDs

If OpenAI doesn't provide tool call ID, generate with `toolu_` prefix:

```rust
let id = tool_call.id.unwrap_or_else(|| {
    format!("toolu_{}", Uuid::new_v4().to_string())
});
```

### 3. Validate JSON in Tool Arguments

Always validate JSON parsing:

```rust
let input = match serde_json::from_str(&tool_call.function.arguments) {
    Ok(json) => json,
    Err(e) => {
        tracing::error!("Invalid tool arguments JSON: {}", e);
        return Err(ConversionError::InvalidToolArguments);
    }
};
```

### 4. Normalize Stop Reasons

Map all stop reasons, but log unknown values:

```rust
let stop_reason = match anthropic_reason.as_str() {
    "end_turn" => "stop",
    "max_tokens" => "length",
    "tool_use" => "tool_calls",
    unknown => {
        tracing::warn!("Unknown Anthropic stop_reason: {}", unknown);
        "stop"  // Safe default
    }
};
```

### 5. Test Round-Trip Conversion

For critical use cases, test that:
```
Original → Convert → Convert back → Compare with original
```

Accept that some fields will be lost (like `signature`), but core content should match.

### 6. Log Conversions

Log when encountering edge cases:
- Unsupported content block types
- Missing required fields
- Invalid JSON
- Unknown stop reasons

This helps debug issues in production.

## References

- **OpenAI API**: https://platform.openai.com/docs/api-reference/chat
- **Anthropic API**: https://platform.claude.com/docs/en/api/messages
- **llama-proxy Implementation**: `/home/iphands/prog/slop/llama-proxy/src/api/openai.rs`
- **Conversion Tests**: `/home/iphands/prog/slop/llama-proxy/src/api/openai.rs` (test modules)

---

*Last updated: February 2026*
*Covers OpenAI Chat Completions API and Anthropic Messages API*
