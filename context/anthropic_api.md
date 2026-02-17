# Anthropic Messages API - Complete Reference

## Overview

This document provides a comprehensive reference for the Anthropic Messages API, covering all content block types, message structures, streaming events, and API features. This reference is based on the official Anthropic Python SDK and API documentation as of February 2026.

**API Endpoint**: `/v1/messages`
**Streaming Endpoint**: `/v1/messages` (with `stream: true`)
**Latest Model Family**: Claude 4.5/4.6

## Message Envelope Structure

### AnthropicMessage (Response)

```json
{
  "id": "msg_01AbCdEfGhIjKlMnOpQrStUv",
  "type": "message",
  "role": "assistant",
  "content": [/* array of content blocks */],
  "model": "claude-sonnet-4-5-20250929",
  "stop_reason": "end_turn",
  "stop_sequence": null,
  "usage": {
    "input_tokens": 150,
    "output_tokens": 250
  }
}
```

### AnthropicRequest (Request)

```json
{
  "model": "claude-sonnet-4-5-20250929",
  "max_tokens": 4096,
  "messages": [
    {
      "role": "user",
      "content": [/* array of content blocks */]
    }
  ],
  "system": "You are a helpful assistant.",
  "temperature": 1.0,
  "top_p": null,
  "top_k": null,
  "stop_sequences": [],
  "stream": false,
  "tools": []
}
```

## Content Block Types

The Anthropic API defines **16 different content block types** across request and response messages.

### Response Content Blocks (6 types)

These blocks appear in the `content` array of assistant responses.

#### 1. TextBlock

Plain text content from the model.

```json
{
  "type": "text",
  "text": "Here is my response to your question.",
  "citations": []  // Optional
}
```

**Fields**:
- `type`: `"text"` (required)
- `text`: String content (required)
- `citations`: Array of citation objects (optional)

#### 2. ThinkingBlock

Reasoning or internal monologue from the model (extended thinking).

```json
{
  "type": "thinking",
  "thinking": "Let me work through this step by step...",
  "signature": "sig_abc123"  // Optional
}
```

**Fields**:
- `type`: `"thinking"` (required)
- `thinking`: String containing reasoning text (required)
- `signature`: Cryptographic signature for verification (optional)

**Notes**:
- Used by models with extended thinking capabilities
- The `signature` field is Anthropic-specific and has no OpenAI equivalent
- Often appears before text blocks in multi-step reasoning

#### 3. RedactedThinkingBlock

Redacted reasoning content (thinking that has been filtered).

```json
{
  "type": "redacted_thinking",
  "data": "base64_encoded_redacted_content"
}
```

**Fields**:
- `type`: `"redacted_thinking"` (required)
- `data`: Base64-encoded redacted content (required)

**Notes**:
- Used when thinking content contains sensitive information
- Cannot be converted to OpenAI format

#### 4. ToolUseBlock

Request from the model to call a function/tool.

```json
{
  "type": "tool_use",
  "id": "toolu_01AbCdEfGhIjKlMnOpQrStUv",
  "name": "get_weather",
  "input": {
    "location": "San Francisco, CA",
    "unit": "fahrenheit"
  }
}
```

**Fields**:
- `type`: `"tool_use"` (required)
- `id`: Unique identifier with `toolu_` prefix (required)
- `name`: Name of the tool to invoke (required)
- `input`: JSON object with tool parameters (required)

**Notes**:
- Triggers `stop_reason: "tool_use"`
- Maps to OpenAI's `tool_calls` array
- Client must execute the tool and provide results in next turn

#### 5. ServerToolUseBlock

Server-side tool invocation (e.g., web search).

```json
{
  "type": "server_tool_use",
  "id": "toolu_01AbCdEfGhIjKlMnOpQrStUv",
  "name": "web_search",
  "input": {
    "query": "anthropic claude pricing 2026"
  }
}
```

**Fields**:
- `type`: `"server_tool_use"` (required)
- `id`: Unique identifier (required)
- `name`: Server-side tool name (e.g., `"web_search"`) (required)
- `input`: JSON object with tool parameters (required)

**Notes**:
- Executed server-side by Anthropic infrastructure
- Results appear as `WebSearchToolResultBlock`
- No OpenAI equivalent

#### 6. WebSearchToolResultBlock

Results from server-side web search.

```json
{
  "type": "web_search_tool_result",
  "tool_use_id": "toolu_01AbCdEfGhIjKlMnOpQrStUv",
  "content": "Search results show that..."
}
```

**Fields**:
- `type`: `"web_search_tool_result"` (required)
- `tool_use_id`: References the server_tool_use ID (required)
- `content`: String containing search results (required)

**Notes**:
- Automatically injected by Anthropic backend
- Client doesn't need to handle these manually
- No OpenAI equivalent

### Request Content Blocks (10 types)

These blocks can appear in user messages sent to the API.

#### 1. TextBlockParam

Plain text content from the user.

```json
{
  "type": "text",
  "text": "Please explain quantum computing.",
  "cache_control": {"type": "ephemeral"},  // Optional
  "citations": []  // Optional
}
```

**Fields**:
- `type`: `"text"` (required)
- `text`: String content (required)
- `cache_control`: Prompt caching directive (optional)
- `citations`: Array of citation objects (optional)

#### 2. ImageBlockParam

Visual content (images) via base64 or URL.

```json
{
  "type": "image",
  "source": {
    "type": "base64",
    "media_type": "image/jpeg",
    "data": "iVBORw0KGgoAAAANSUhEUgAA..."
  },
  "cache_control": {"type": "ephemeral"}  // Optional
}
```

**Or with URL**:

```json
{
  "type": "image",
  "source": {
    "type": "url",
    "url": "https://example.com/image.jpg"
  }
}
```

**Fields**:
- `type`: `"image"` (required)
- `source`: Image source object (required)
  - `type`: `"base64"` or `"url"` (required)
  - `media_type`: MIME type (required for base64)
  - `data`: Base64-encoded image (required for base64)
  - `url`: Image URL (required for url type)
- `cache_control`: Prompt caching directive (optional)

**Notes**:
- Maps to OpenAI's `content[].image_url`
- Supported formats: PNG, JPEG, GIF, WebP
- Maximum image size varies by model

#### 3. DocumentBlockParam

Structured document content (PDFs, plaintext files).

```json
{
  "type": "document",
  "source": {
    "type": "base64",
    "media_type": "application/pdf",
    "data": "JVBERi0xLjQKJeLjz9MKMSAwIG9ia..."
  },
  "title": "Q4 Financial Report",
  "context": "Annual report for fiscal year 2025",
  "citations": [],
  "cache_control": {"type": "ephemeral"}
}
```

**Fields**:
- `type`: `"document"` (required)
- `source`: Document source object (required)
- `title`: Document title (optional)
- `context`: Contextual description (optional)
- `citations`: Array of citation objects (optional)
- `cache_control`: Prompt caching directive (optional)

**Notes**:
- Supports PDFs and plaintext documents
- No OpenAI equivalent
- Not supported by llama.cpp backend

#### 4. ToolUseBlockParam

Tool invocation in request (for multi-turn conversations).

```json
{
  "type": "tool_use",
  "id": "toolu_01AbCdEfGhIjKlMnOpQrStUv",
  "name": "get_weather",
  "input": {
    "location": "San Francisco, CA"
  },
  "cache_control": {"type": "ephemeral"}
}
```

**Fields**:
- `type`: `"tool_use"` (required)
- `id`: Unique identifier (required)
- `name`: Tool name (required)
- `input`: JSON object with parameters (required)
- `cache_control`: Prompt caching directive (optional)

**Notes**:
- Used when echoing model's tool_use back in conversation history
- Not typically created by clients directly

#### 5. ToolResultBlockParam

Tool execution results from the client.

```json
{
  "type": "tool_result",
  "tool_use_id": "toolu_01AbCdEfGhIjKlMnOpQrStUv",
  "content": "Temperature: 72°F, Conditions: Sunny",
  "is_error": false,
  "cache_control": {"type": "ephemeral"}
}
```

**Or with structured content**:

```json
{
  "type": "tool_result",
  "tool_use_id": "toolu_01AbCdEfGhIjKlMnOpQrStUv",
  "content": [
    {"type": "text", "text": "Temperature: 72°F"},
    {"type": "image", "source": {"type": "url", "url": "https://..."}}
  ]
}
```

**Fields**:
- `type`: `"tool_result"` (required)
- `tool_use_id`: References the tool_use ID (required)
- `content`: String or array of content blocks (required)
- `is_error`: Boolean indicating tool execution failure (optional, default: false)
- `cache_control`: Prompt caching directive (optional)

**Notes**:
- Must be provided in response to `tool_use` blocks
- In OpenAI format, requires separate message with `role: "tool"`
- `content` can be simple string or array of blocks for multimodal results

#### 6. ThinkingBlockParam

Thinking content in request (for multi-turn conversations).

```json
{
  "type": "thinking",
  "thinking": "The user is asking about...",
  "signature": "sig_abc123"
}
```

**Fields**:
- `type`: `"thinking"` (required)
- `thinking`: String containing reasoning (required)
- `signature`: Cryptographic signature (optional)

**Notes**:
- Used when echoing model's thinking back in conversation history
- Not typically created by clients directly

#### 7. RedactedThinkingBlockParam

Redacted thinking in request.

```json
{
  "type": "redacted_thinking",
  "data": "base64_encoded_redacted_content"
}
```

**Fields**:
- `type`: `"redacted_thinking"` (required)
- `data`: Base64-encoded content (required)

#### 8. SearchResultBlockParam

Search result content from external sources.

```json
{
  "type": "search_result",
  "source": "Google Search",
  "title": "Anthropic Claude API Documentation",
  "content": "The Claude API provides...",
  "citations": [],
  "cache_control": {"type": "ephemeral"}
}
```

**Fields**:
- `type`: `"search_result"` (required)
- `source`: Search source identifier (required)
- `title`: Result title (required)
- `content`: Result content/snippet (required)
- `citations`: Array of citation objects (optional)
- `cache_control`: Prompt caching directive (optional)

**Notes**:
- Used for providing search results to the model
- No OpenAI equivalent

#### 9. ServerToolUseBlockParam

Server-side tool in request (for multi-turn).

```json
{
  "type": "server_tool_use",
  "id": "toolu_01AbCdEfGhIjKlMnOpQrStUv",
  "name": "web_search",
  "input": {"query": "claude pricing"},
  "cache_control": {"type": "ephemeral"}
}
```

**Fields**:
- `type`: `"server_tool_use"` (required)
- `id`: Unique identifier (required)
- `name`: Server tool name (required)
- `input`: JSON object with parameters (required)
- `cache_control`: Prompt caching directive (optional)

#### 10. WebSearchToolResultBlockParam

Web search results in request.

```json
{
  "type": "web_search_tool_result",
  "tool_use_id": "toolu_01AbCdEfGhIjKlMnOpQrStUv",
  "content": "Search results...",
  "cache_control": {"type": "ephemeral"}
}
```

**Fields**:
- `type`: `"web_search_tool_result"` (required)
- `tool_use_id`: References server_tool_use ID (required)
- `content`: Search results string (required)
- `cache_control`: Prompt caching directive (optional)

## Streaming Events

The Anthropic API uses Server-Sent Events (SSE) for streaming responses. There are **6 event types**:

### 1. message_start

Initial message metadata at stream start.

```json
{
  "type": "message_start",
  "message": {
    "id": "msg_01AbCdEfGhIjKlMnOpQrStUv",
    "type": "message",
    "role": "assistant",
    "content": [],
    "model": "claude-sonnet-4-5-20250929",
    "stop_reason": null,
    "stop_sequence": null,
    "usage": {
      "input_tokens": 150,
      "output_tokens": 0
    }
  }
}
```

### 2. content_block_start

Beginning of a new content block.

```json
{
  "type": "content_block_start",
  "index": 0,
  "content_block": {
    "type": "text",
    "text": ""
  }
}
```

**Or for thinking**:

```json
{
  "type": "content_block_start",
  "index": 0,
  "content_block": {
    "type": "thinking",
    "thinking": "",
    "signature": null
  }
}
```

**Or for tool_use**:

```json
{
  "type": "content_block_start",
  "index": 1,
  "content_block": {
    "type": "tool_use",
    "id": "toolu_01AbCdEfGhIjKlMnOpQrStUv",
    "name": "get_weather",
    "input": {}
  }
}
```

### 3. content_block_delta

Incremental content updates. There are **4 delta types**:

#### text_delta

```json
{
  "type": "content_block_delta",
  "index": 0,
  "delta": {
    "type": "text_delta",
    "text": "Here is "
  }
}
```

#### thinking_delta

```json
{
  "type": "content_block_delta",
  "index": 0,
  "delta": {
    "type": "thinking_delta",
    "thinking": "Let me consider"
  }
}
```

#### signature_delta

```json
{
  "type": "content_block_delta",
  "index": 0,
  "delta": {
    "type": "signature_delta",
    "signature": "sig_abc123"
  }
}
```

#### input_json_delta

```json
{
  "type": "content_block_delta",
  "index": 1,
  "delta": {
    "type": "input_json_delta",
    "partial_json": "{\"location\": \"San"
  }
}
```

### 4. content_block_stop

End of a content block.

```json
{
  "type": "content_block_stop",
  "index": 0
}
```

### 5. message_delta

Final message metadata (stop reason, final token counts).

```json
{
  "type": "message_delta",
  "delta": {
    "stop_reason": "end_turn",
    "stop_sequence": null
  },
  "usage": {
    "output_tokens": 250
  }
}
```

### 6. message_stop

Stream termination (no data, just event type).

```json
{
  "type": "message_stop"
}
```

## Stop Reasons

The `stop_reason` field indicates why the model stopped generating:

| Stop Reason | Description |
|-------------|-------------|
| `end_turn` | Model naturally completed its response |
| `max_tokens` | Reached the `max_tokens` limit |
| `stop_sequence` | Hit a configured stop sequence |
| `tool_use` | Model wants to call a tool/function |

**Notes**:
- `null` during streaming (set in `message_delta`)
- Always present in final non-streaming response
- Maps to OpenAI's `finish_reason` field

## Usage Object

Token counting information.

```json
{
  "input_tokens": 150,
  "output_tokens": 250,
  "cache_creation_tokens": 0,
  "cache_read_tokens": 0
}
```

**Fields**:
- `input_tokens`: Tokens in the request (required)
- `output_tokens`: Tokens in the response (required)
- `cache_creation_tokens`: Tokens written to cache (optional)
- `cache_read_tokens`: Tokens read from cache (optional)

**Notes**:
- In streaming, `output_tokens` starts at 0 in `message_start`
- Final count provided in `message_delta`
- Cache fields only present when prompt caching is active

## Citations

Citations provide source attribution for model responses.

```json
{
  "type": "citation",
  "source": {
    "type": "document",
    "document_id": "doc_123",
    "title": "Annual Report 2025",
    "page": 42
  },
  "text": "Revenue increased 25% year-over-year",
  "start": 150,
  "end": 187
}
```

**Fields**:
- `type`: `"citation"` (required)
- `source`: Source object with metadata (required)
- `text`: Cited text excerpt (optional)
- `start`: Character offset in content (optional)
- `end`: Character offset in content (optional)

**Notes**:
- Experimental feature
- No OpenAI equivalent
- Not supported by llama.cpp

## Cache Control

Prompt caching directives for optimization.

```json
{
  "type": "ephemeral"
}
```

**Types**:
- `ephemeral`: Cache for 5 minutes (current default)

**Usage**:
```json
{
  "type": "text",
  "text": "Long context here...",
  "cache_control": {"type": "ephemeral"}
}
```

**Notes**:
- Reduces latency and cost for repeated prompts
- Cache keys based on exact content match
- Applies to any content block with `cache_control` field

## Server-Side Tools

Built-in tools executed by Anthropic infrastructure.

### web_search

```json
{
  "type": "server_tool_use",
  "id": "toolu_01XYZ",
  "name": "web_search",
  "input": {
    "query": "latest AI research 2026"
  }
}
```

**Result**:

```json
{
  "type": "web_search_tool_result",
  "tool_use_id": "toolu_01XYZ",
  "content": "Search results:\n1. Paper: ...\n2. Article: ..."
}
```

**Notes**:
- No client implementation needed
- Results automatically injected into conversation
- No OpenAI equivalent

## Model-Specific Features

### Extended Thinking (Claude 4.5/4.6)

Models with extended thinking capabilities generate `thinking` blocks:

```json
{
  "content": [
    {
      "type": "thinking",
      "thinking": "To answer this question, I need to...",
      "signature": "sig_abc123"
    },
    {
      "type": "text",
      "text": "Based on my analysis, the answer is..."
    }
  ]
}
```

**Behavior**:
- `thinking` blocks appear before `text` responses
- Can be multiple thinking blocks for complex reasoning
- `signature` field provides cryptographic verification
- Thinking not counted toward `max_tokens` limit

### Multimodal (Vision)

Models can process images:

```json
{
  "messages": [
    {
      "role": "user",
      "content": [
        {"type": "text", "text": "What's in this image?"},
        {
          "type": "image",
          "source": {
            "type": "base64",
            "media_type": "image/jpeg",
            "data": "..."
          }
        }
      ]
    }
  ]
}
```

**Supported formats**: PNG, JPEG, GIF, WebP
**Maximum size**: Varies by model (typically 5-10MB)

## Error Handling

### Error Response Format

```json
{
  "type": "error",
  "error": {
    "type": "invalid_request_error",
    "message": "max_tokens is required"
  }
}
```

**Error Types**:
- `invalid_request_error`: Invalid request parameters
- `authentication_error`: Invalid API key
- `permission_error`: Insufficient permissions
- `not_found_error`: Endpoint or resource not found
- `rate_limit_error`: Rate limit exceeded
- `api_error`: Internal server error
- `overloaded_error`: Service temporarily overloaded

### Tool Execution Errors

When a tool fails, use `is_error: true`:

```json
{
  "type": "tool_result",
  "tool_use_id": "toolu_01ABC",
  "content": "Failed to fetch weather: Connection timeout",
  "is_error": true
}
```

## API Compatibility Notes

### Differences from OpenAI API

1. **Message structure**: Anthropic uses `content` arrays; OpenAI uses `content` strings (or arrays for multimodal)
2. **Tool results**: Anthropic uses content blocks; OpenAI uses separate `role: "tool"` messages
3. **Thinking**: Anthropic has dedicated `thinking` blocks; OpenAI uses `reasoning_text` field
4. **System messages**: Anthropic uses `system` parameter; OpenAI uses `role: "system"` messages
5. **Stop reasons**: Different naming (`end_turn` vs `stop`, `tool_use` vs `tool_calls`)

### What Maps Between Formats

**Direct mappings**:
- `text` ↔ `content` (string or text content)
- `thinking` ↔ `reasoning_text`
- `tool_use` ↔ `tool_calls`
- `image` ↔ `image_url`

**Cannot map**:
- `document` blocks (no OpenAI equivalent)
- `search_result` blocks (no OpenAI equivalent)
- `server_tool_use` blocks (no OpenAI equivalent)
- `citations` field (no OpenAI equivalent)
- `cache_control` field (no OpenAI equivalent)
- `redacted_thinking` blocks (no OpenAI equivalent)

## References

- **Official SDK**: https://github.com/anthropics/anthropic-sdk-python
- **API Documentation**: https://platform.claude.com/docs/en/api/messages
- **Streaming Guide**: https://platform.claude.com/docs/en/api/streaming
- **Tool Use Guide**: https://platform.claude.com/docs/en/api/tool-use
- **Prompt Caching**: https://platform.claude.com/docs/en/api/prompt-caching

---

*Last updated: February 2026*
*Based on Anthropic API version 2023-06-01*
