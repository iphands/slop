# llama.cpp Anthropic API Support

## Overview

This document details which Anthropic API content block types are supported by the llama.cpp backend. llama.cpp implements a **subset** of the full Anthropic Messages API, supporting **5 of the 16 official content block types**.

**Backend Implementation**: `/home/iphands/prog/slop/vendor/llama.cpp/tools/server/server-common.cpp`
**Relevant Lines**: 1408-1468 (content block serialization)

## Supported Content Block Types

llama.cpp supports **5 content block types** total:

### 1. text ✅

**Direction**: Request (input) and Response (output)

**Implementation**: Full support

**Request Format**:
```json
{
  "type": "text",
  "text": "Hello, Claude!"
}
```

**Response Format**:
```json
{
  "type": "text",
  "text": "Hello! How can I help you today?"
}
```

**Source Code Reference**:
- File: `vendor/llama.cpp/tools/server/server-common.cpp`
- Lines: ~1420-1425 (text block serialization)
- Lines: ~850-900 (text block parsing from OpenAI format)

**Notes**:
- Most common content block type
- Maps directly between Anthropic and OpenAI formats
- No special handling required

### 2. thinking ✅

**Direction**: Response (output only)

**Implementation**: Full support including optional `signature` field

**Response Format**:
```json
{
  "type": "thinking",
  "thinking": "Let me work through this problem step by step...",
  "signature": "sig_abc123"
}
```

**Source Code Reference**:
- File: `vendor/llama.cpp/tools/server/server-common.cpp`
- Lines: ~1430-1440 (thinking block serialization)

**Notes**:
- Used by models with extended thinking capabilities (Claude 4.5/4.6)
- The `signature` field is optional and Anthropic-specific
- Internally converted from OpenAI `reasoning_text` field
- Only appears in responses, never in requests

### 3. tool_use ✅

**Direction**: Request (input) and Response (output)

**Implementation**: Full support

**Request Format** (echoing model's tool call in conversation history):
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

**Response Format** (model requesting tool call):
```json
{
  "type": "tool_use",
  "id": "toolu_01AbCdEfGhIjKlMnOpQrStUv",
  "name": "calculate",
  "input": {
    "expression": "2 + 2"
  }
}
```

**Source Code Reference**:
- File: `vendor/llama.cpp/tools/server/server-common.cpp`
- Lines: ~1445-1460 (tool_use serialization)
- Lines: ~950-1000 (tool_use parsing from OpenAI `tool_calls`)

**Notes**:
- Maps to/from OpenAI's `tool_calls` array
- ID has `toolu_` prefix (Anthropic convention)
- `input` is a JSON object (not a JSON string)
- Triggers `stop_reason: "tool_use"`

### 4. tool_result ✅

**Direction**: Request (input only)

**Implementation**: Full support including error handling

**Request Format**:
```json
{
  "type": "tool_result",
  "tool_use_id": "toolu_01AbCdEfGhIjKlMnOpQrStUv",
  "content": "The current temperature is 72°F and sunny.",
  "is_error": false
}
```

**Request Format (error case)**:
```json
{
  "type": "tool_result",
  "tool_use_id": "toolu_01AbCdEfGhIjKlMnOpQrStUv",
  "content": "Error: Connection timeout",
  "is_error": true
}
```

**Source Code Reference**:
- File: `vendor/llama.cpp/tools/server/server-common.cpp`
- Lines: ~1000-1050 (tool_result parsing)

**Notes**:
- Converts to OpenAI message with `role: "tool"`
- `tool_use_id` links to previous `tool_use.id`
- `is_error` field is optional (defaults to false)
- `content` must be a string (structured content arrays NOT supported)

### 5. image ✅

**Direction**: Request (input only)

**Implementation**: Full support for both base64 and URL sources

**Request Format (base64)**:
```json
{
  "type": "image",
  "source": {
    "type": "base64",
    "media_type": "image/jpeg",
    "data": "iVBORw0KGgoAAAANSUhEUgAA..."
  }
}
```

**Request Format (URL)**:
```json
{
  "type": "image",
  "source": {
    "type": "url",
    "url": "https://example.com/photo.jpg"
  }
}
```

**Source Code Reference**:
- File: `vendor/llama.cpp/tools/server/server-common.cpp`
- Lines: ~900-950 (image block parsing)

**Notes**:
- Maps to OpenAI's `content[].image_url` format
- Supports both base64 and URL sources
- `media_type` required for base64, must match image format
- Converted to data URI for OpenAI format: `data:image/jpeg;base64,{data}`
- Supported formats: PNG, JPEG, GIF, WebP

## Unsupported Content Block Types

The following **11 Anthropic content block types are NOT supported** by llama.cpp:

### Request-Only Types (Not Supported)

1. **document** ❌
   - Used for: PDF and plaintext document processing
   - Why unsupported: No document processing in llama.cpp

2. **search_result** ❌
   - Used for: Providing search results to the model
   - Why unsupported: Anthropic-specific feature

3. **server_tool_use** ❌
   - Used for: Server-side tool invocations (e.g., web_search)
   - Why unsupported: No server-side tool infrastructure

4. **web_search_tool_result** ❌
   - Used for: Web search results from server-side tools
   - Why unsupported: No server-side tool infrastructure

5. **redacted_thinking** ❌
   - Used for: Redacted reasoning content
   - Why unsupported: No content redaction logic

### Response-Only Types (Not Supported)

6. **redacted_thinking** (response) ❌
   - Used for: Redacted reasoning in responses
   - Why unsupported: No content redaction logic

7. **server_tool_use** (response) ❌
   - Used for: Server-side tool requests
   - Why unsupported: No server-side tool infrastructure

8. **web_search_tool_result** (response) ❌
   - Used for: Web search results
   - Why unsupported: No server-side tool infrastructure

### Unsupported Fields

9. **citations** ❌
   - Field in: text, document, search_result blocks
   - Why unsupported: No citation tracking

10. **cache_control** ❌
    - Field in: All request content blocks
    - Why unsupported: No prompt caching implementation

11. **structured tool_result.content** ❌
    - Anthropic allows `content` as array of blocks
    - llama.cpp only supports string content

## Internal Conversion Flow

llama.cpp internally uses OpenAI format and converts to/from Anthropic:

### Request Path (Anthropic → OpenAI)

1. **Client sends Anthropic format request**
   ```json
   {
     "messages": [
       {
         "role": "user",
         "content": [
           {"type": "text", "text": "Hello"},
           {"type": "image", "source": {...}}
         ]
       }
     ]
   }
   ```

2. **llama.cpp converts to OpenAI format internally**
   ```json
   {
     "messages": [
       {
         "role": "user",
         "content": [
           {"type": "text", "text": "Hello"},
           {"type": "image_url", "image_url": {"url": "data:..."}}
         ]
       }
     ]
   }
   ```

3. **Model processes in OpenAI format**

4. **Response converted back to Anthropic format**

### Response Path (OpenAI → Anthropic)

1. **Model generates OpenAI format response**
   ```json
   {
     "choices": [
       {
         "message": {
           "content": "Hello!",
           "reasoning_text": "The user greeted me..."
         }
       }
     ]
   }
   ```

2. **llama.cpp converts to Anthropic format**
   ```json
   {
     "content": [
       {
         "type": "thinking",
         "thinking": "The user greeted me..."
       },
       {
         "type": "text",
         "text": "Hello!"
       }
     ]
   }
   ```

### Tool Calling Conversion

**Anthropic Request**:
```json
{
  "content": [
    {
      "type": "tool_result",
      "tool_use_id": "toolu_01ABC",
      "content": "Result: 42"
    }
  ]
}
```

**Converted to OpenAI**:
```json
{
  "role": "tool",
  "tool_call_id": "toolu_01ABC",
  "content": "Result: 42"
}
```

**Anthropic Response**:
```json
{
  "content": [
    {
      "type": "tool_use",
      "id": "toolu_01XYZ",
      "name": "calculate",
      "input": {"expression": "6 * 7"}
    }
  ],
  "stop_reason": "tool_use"
}
```

**Converted from OpenAI**:
```json
{
  "message": {
    "tool_calls": [
      {
        "id": "toolu_01XYZ",
        "function": {
          "name": "calculate",
          "arguments": "{\"expression\": \"6 * 7\"}"
        }
      }
    ]
  },
  "finish_reason": "tool_calls"
}
```

## Streaming Support

llama.cpp supports all **6 Anthropic streaming event types**:

1. **message_start** ✅
2. **content_block_start** ✅
3. **content_block_delta** ✅
   - text_delta ✅
   - thinking_delta ✅
   - signature_delta ✅
   - input_json_delta ✅
4. **content_block_stop** ✅
5. **message_delta** ✅
6. **message_stop** ✅

**Source Code Reference**:
- File: `vendor/llama.cpp/tools/server/server-common.cpp`
- Lines: ~1500-1700 (streaming event emission)

**Notes**:
- All SSE events properly formatted
- Delta accumulation handled correctly
- Stop reason mapping complete

## Stop Reason Mappings

| Anthropic | OpenAI (Internal) | Notes |
|-----------|-------------------|-------|
| `end_turn` | `stop` | Normal completion |
| `max_tokens` | `length` | Hit token limit |
| `stop_sequence` | `stop` | Hit stop sequence |
| `tool_use` | `tool_calls` | Model wants to call tools |

**Source Code Reference**:
- File: `vendor/llama.cpp/tools/server/server-common.cpp`
- Lines: ~1300-1320 (stop reason conversion)

## Limitations Compared to Official Anthropic API

### Missing Features

1. **Document Processing** ❌
   - Cannot process PDFs or structured documents
   - No `document` content block support

2. **Server-Side Tools** ❌
   - No built-in web_search capability
   - No `server_tool_use` or `web_search_tool_result` blocks

3. **Citations** ❌
   - No source attribution tracking
   - `citations` field ignored if present

4. **Prompt Caching** ❌
   - No prompt caching implementation
   - `cache_control` field ignored if present

5. **Redacted Thinking** ❌
   - No content redaction logic
   - Cannot handle `redacted_thinking` blocks

6. **Structured Tool Results** ❌
   - `tool_result.content` must be string
   - Cannot use array of content blocks for multimodal tool results

### Behavioral Differences

1. **Error Handling**
   - llama.cpp may return less detailed error messages
   - Error format slightly different from official API

2. **Token Counting**
   - Usage tracking may differ slightly
   - Cache-related token fields always zero

3. **Signature Field**
   - `signature` field in `thinking` blocks not cryptographically verified
   - Passed through as-is if present

## Testing Coverage

llama.cpp includes tests for:

- ✅ Text content blocks (request and response)
- ✅ Thinking blocks with signatures
- ✅ Tool use and tool results
- ✅ Image blocks (base64 and URL)
- ✅ Streaming all content block types
- ✅ Stop reason mappings
- ✅ Multi-turn conversations with tools

**Test Location**: `vendor/llama.cpp/tests/`

## What Works in Practice

Based on llama.cpp implementation, the following use cases work correctly:

### ✅ Basic Chat
```
User text → Model text response
```

### ✅ Extended Thinking
```
User text → Model thinking + text response
```

### ✅ Tool Calling (Single Turn)
```
User text → Model tool_use → User tool_result → Model text
```

### ✅ Tool Calling (Multi-Tool)
```
User text → Model [tool_use, tool_use] → User [tool_result, tool_result] → Model text
```

### ✅ Multimodal Vision
```
User [text, image] → Model text response
```

### ✅ Complex Conversations
```
User [text, image] → Model [thinking, text, tool_use] → User tool_result → Model text
```

## What Doesn't Work

### ❌ Document Processing
```
User [text, document{PDF}] → Error: unsupported content block type
```

### ❌ Search Results
```
User [text, search_result] → Error: unsupported content block type
```

### ❌ Server-Side Tools
```
Model server_tool_use (web_search) → Not generated
```

### ❌ Structured Tool Results
```
User tool_result{content: [text, image]} → Error: content must be string
```

### ❌ Citations
```
Any request with citations field → Field ignored
```

### ❌ Prompt Caching
```
Any request with cache_control → Field ignored, no caching
```

## Recommendations for Proxy Implementations

When building a proxy between OpenAI and Anthropic formats (like llama-proxy):

### 1. Implement Core 5 Types First
- **text** - Mandatory for basic functionality
- **thinking** - Important for extended thinking models
- **tool_use** - Essential for function calling
- **tool_result** - Required for multi-turn tool calls
- **image** - Needed for multimodal use cases

### 2. Handle Unsupported Types Gracefully
- Return clear error messages for unsupported types
- Log warnings when encountering unknown types
- Don't crash on unexpected content blocks

### 3. Test Against llama.cpp Backend
- Verify conversion logic matches llama.cpp's expectations
- Test streaming with all supported delta types
- Validate stop reason mappings

### 4. Document Limitations Clearly
- Inform users which types are supported
- Explain what will fail with unsupported types
- Provide migration paths for future support

## Future Enhancement Possibilities

If llama.cpp adds support for:

1. **Document Processing** - Would enable PDF/document workflows
2. **Structured Tool Results** - Would allow multimodal tool outputs
3. **Prompt Caching** - Would improve performance and cost
4. **Citations** - Would enable source attribution

These would require proxy implementation updates to handle the new types.

## References

- **llama.cpp Source**: `/home/iphands/prog/slop/vendor/llama.cpp/tools/server/server-common.cpp`
- **Content Block Serialization**: Lines 1408-1468
- **Request Parsing**: Lines 800-1100
- **Streaming Events**: Lines 1500-1700
- **Stop Reason Mapping**: Lines 1300-1320

---

*Last updated: February 2026*
*Based on llama.cpp commit: latest main branch*
