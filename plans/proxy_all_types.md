# Plan: Comprehensive Anthropic Content Block Type Research & Documentation

## Context

After implementing basic `tool_use` and `tool_result` support, we need to ensure our implementation is **complete and correct** by:

1. **Verifying** our recent fix is correct
2. **Identifying** ALL Anthropic content block types (not just the ones we've encountered)
3. **Documenting** which types can/cannot map to OpenAI format
4. **Creating comprehensive reference documentation** in `./context/` for future development

This is critical because we've been discovering content block types reactively (hitting errors, then fixing). We need a proactive, comprehensive approach to handle ALL types the backend (llama.cpp) might send.

---

## Research Findings

### **Complete Anthropic Content Block Types - AUTHORITATIVE LIST**

Based on research of official Anthropic SDK and `vendor/llama.cpp`:

#### **Currently Implemented in llama-proxy** ✅
1. **text** - Plain text content
2. **thinking** - Reasoning/internal monologue with optional signature
3. **tool_use** - Function/tool invocation with id, name, input
4. **tool_result** - Tool execution result with tool_use_id, content, is_error

#### **Supported by llama.cpp Backend (5 types total)** 🔧
1. **text** - Plain text (✅ implemented)
2. **thinking** - Reasoning blocks with signature (✅ implemented)
3. **tool_use** - Tool invocations (✅ implemented)
4. **tool_result** - Tool execution results (✅ implemented)
5. **image** - Visual content with base64 or URL sources (⚠️ NOT implemented)

**Location**: `/home/iphands/prog/slop/vendor/llama.cpp/tools/server/server-common.cpp` lines 1408-1468

#### **Official Anthropic API Types (16 types total)** 📚

**Response Content Blocks (6 types):**
1. **TextBlock** - `{ text, citations? }`
2. **ThinkingBlock** - `{ thinking, signature? }`
3. **RedactedThinkingBlock** - `{ data }` - Redacted reasoning
4. **ToolUseBlock** - `{ id, name, input }`
5. **ServerToolUseBlock** - `{ id, name: "web_search", input }` - Server-side tools
6. **WebSearchToolResultBlock** - `{ tool_use_id, content }` - Web search results

**Request Content Blocks (10 types):**
1. **TextBlockParam** - `{ text, cache_control?, citations? }`
2. **ImageBlockParam** - `{ source: {base64|url}, cache_control? }`
3. **DocumentBlockParam** - `{ source, title?, context?, citations?, cache_control? }` - PDFs, plaintext
4. **ToolUseBlockParam** - `{ id, name, input, cache_control? }`
5. **ToolResultBlockParam** - `{ tool_use_id, content, is_error?, cache_control? }`
6. **ThinkingBlockParam** - `{ thinking, signature? }`
7. **RedactedThinkingBlockParam** - `{ data }`
8. **SearchResultBlockParam** - `{ source, title, content, citations?, cache_control? }`
9. **ServerToolUseBlockParam** - `{ id, name: "web_search", input, cache_control? }`
10. **WebSearchToolResultBlockParam** - `{ tool_use_id, content, cache_control? }`

**Sources:**
- Official Anthropic Python SDK: https://github.com/anthropics/anthropic-sdk-python
- Official API docs: https://platform.claude.com/docs/en/api/messages

---

## OpenAI Mapping Analysis

### **Can Map to OpenAI Format** ✅

| Anthropic Type | OpenAI Equivalent | Mapping Details |
|----------------|-------------------|-----------------|
| `text` | `content` (string) or `content.text` | Direct mapping |
| `thinking` | `reasoning_text` field | Extended thinking/reasoning field |
| `tool_use` | `tool_calls` array | Convert: `id`→`id`, `name`→`function.name`, `input`→`function.arguments` |
| `image` (url) | `content[].image_url` | Convert: `source.url`→`image_url.url` |
| `image` (base64) | `content[].image_url` | Convert: `data:image/{type};base64,{data}`→`image_url.url` |

### **Cannot Map to OpenAI Format** ❌

| Anthropic Type | Reason | Handling Strategy |
|----------------|--------|-------------------|
| `tool_result` | OpenAI uses `role: "tool"` message, not content block | Convert to separate message with role="tool" |

### **Signature Field** ⚠️
- The `signature` field in `thinking` blocks is Anthropic-specific
- No OpenAI equivalent
- **Strategy**: Preserve in Anthropic format, ignore in OpenAI conversion

---

## Streaming Event Types

All 6 Anthropic SSE event types for streaming:

1. **message_start** - Initial message metadata
2. **content_block_start** - Begin content block (with index and type)
3. **content_block_delta** - Incremental updates
   - `text_delta` - For text blocks
   - `thinking_delta` - For thinking blocks
   - `signature_delta` - For thinking signature updates
   - `input_json_delta` - For tool_use input streaming
4. **content_block_stop** - End content block
5. **message_delta** - Final metadata (stop_reason, usage)
6. **message_stop** - Stream termination

**Implementation Status**: ✅ All event types implemented in `/home/iphands/prog/slop/llama-proxy/src/proxy/synthesis.rs`

---

## Stop Reason Mappings

| Anthropic | OpenAI | Notes |
|-----------|--------|-------|
| `end_turn` | `stop` | Normal completion |
| `max_tokens` | `length` | Context limit reached |
| `stop_sequence` | `stop` | Hit configured stop sequence |
| `tool_use` | `tool_calls` | Model wants to call tools |

**Implementation Status**: ✅ All mappings implemented correctly

---

## Verification of Recent Implementation

### ✅ **Correct Implementation**

Our recent fix correctly implements:

1. **AnthropicContentBlock enum** with 4 types (text, thinking, tool_use, tool_result)
2. **Bidirectional conversion**:
   - `From<AnthropicMessage> for ChatCompletionResponse`
   - `From<ChatCompletionResponse> for AnthropicMessage`
3. **Streaming synthesis** for all content block types
4. **Stop reason mapping** (both directions)
5. **Comprehensive test coverage** (274 tests passing)

### ⚠️ **Missing: Image Content Blocks**

llama.cpp backend supports image blocks but our proxy doesn't. This could cause parsing errors if:
- Backend returns Anthropic format with image content blocks
- Client sends image content in requests (multimodal)

**Impact**: Medium - Only affects multimodal/vision use cases

---

## Documentation Strategy (No Implementation Changes)

Per user decision: **Documentation only** - no code changes for missing types at this time.

### **Documentation Approach**

Create comprehensive reference documentation that clearly distinguishes:
1. **What llama.cpp supports** (5 types) - the backend we're proxying
2. **What our proxy implements** (4 types) - current state
3. **What official Anthropic supports** (16 types) - complete API surface
4. **Mapping/conversion possibilities** - what can/can't convert to OpenAI

This allows future developers to understand:
- What will work with current implementation
- What might cause parsing errors
- What needs to be added for specific use cases
- How to extend support when needed

---

## Documentation Files to Create

### **1. Complete API Reference**

**File**: `/home/iphands/prog/slop/context/anthropic_api.md`

**Complete Anthropic Messages API Reference** - 500-700 lines
- All 16 official content block types with full field specifications
- Request vs Response content blocks
- Message envelope structure
- Usage object (input_tokens, output_tokens)
- Stop reason values and meanings
- Streaming SSE event types (all 6 types)
- Delta types (text_delta, thinking_delta, signature_delta, input_json_delta)
- JSON examples for each content block type
- Citations field structure
- Cache control field structure
- Server-side tool types (web_search)

### **2. llama.cpp Support Matrix**

**File**: `/home/iphands/prog/slop/context/llama_cpp_supported_types.md`

**What llama.cpp Backend Supports** - 200-300 lines
- Clear list: 5 content block types (text, thinking, tool_use, tool_result, image)
- Implementation details from vendor/llama.cpp source
- Conversion logic (Anthropic → OpenAI internally)
- Limitations compared to official Anthropic API
- Code references with file paths and line numbers
- Testing coverage in llama.cpp
- What works, what doesn't

### **3. Conversion Mapping Guide**

**File**: `/home/iphands/prog/slop/context/openai_anthropic_mapping.md`

**Bidirectional Conversion Reference** - 400-500 lines

**Can Map (Anthropic → OpenAI):**
- text → content (string) or content[].text
- thinking → reasoning_text field
- tool_use → tool_calls[] array
- image (url) → content[].image_url
- image (base64) → content[].image_url (with data URI)

**Cannot Map (No OpenAI Equivalent):**
- document blocks (PDFs, structured docs)
- search_result blocks
- server_tool_use blocks (web_search)
- web_search_tool_result blocks
- redacted_thinking blocks
- citations field
- cache_control field

**Requires Transformation:**
- tool_result → separate message with role="tool"
- Multiple content blocks → concatenation or array

**Edge Cases:**
- Empty content arrays
- Mixed content types
- Signature field (Anthropic-only, dropped in conversion)
- Stop reason mappings

### **4. Lessons Learned & Pitfalls**

**File**: `/home/iphands/prog/slop/context/pitfalls.md` (append new entry)

**New Entry: Reactive vs. Proactive Content Block Discovery** - ~200 words

**Pitfall Title**: Discovering Content Block Types Reactively Instead of Proactively

**The Problem:**
We were adding Anthropic content block types only when we encountered parsing errors:
1. Hit error: "Failed to parse backend response - missing field `prompt_tokens`"
2. Debug and discover backend sent `tool_use` content block
3. Add `tool_use` to enum, fix parsing
4. Later, hit same error with `image` blocks (potential future issue)
5. Repeat cycle...

This reactive approach causes:
- Production parsing errors for legitimate backend responses
- Incomplete type coverage
- Multiple rounds of fixes for the same root cause
- User-facing errors when backends evolve

**The Solution:**
Research ALL content block types upfront by:
1. Reading backend source code (vendor/llama.cpp)
2. Checking official API documentation (Anthropic SDK)
3. Documenting complete type catalog before implementation
4. Adding all types together, even if not immediately needed

**Source Projects:**
- llama-proxy: `src/api/openai.rs` - Had to add tool_use/tool_result after hitting errors
- llama-proxy: Missing image blocks despite llama.cpp supporting them

**Prevention:**
Create comprehensive type catalogs (see `context/anthropic_api.md`) BEFORE implementing parsers.

---

## Files to Create (Documentation Only - No Code Changes)

All files in `/home/iphands/prog/slop/context/`:

1. **anthropic_api.md** - Complete API reference (500-700 lines)
   - All 16 official content block types
   - Message structures, streaming events, field specifications
   - Stop reasons, usage tracking
   - JSON examples for each type

2. **llama_cpp_supported_types.md** - Backend support matrix (200-300 lines)
   - 5 types llama.cpp implements
   - Implementation details from vendor source
   - Limitations vs official API
   - Code references with file paths

3. **openai_anthropic_mapping.md** - Conversion guide (400-500 lines)
   - What maps between formats
   - What can't be mapped
   - Edge cases and transformations
   - Bidirectional conversion strategies

4. **pitfalls.md** - Append new entry (~200 words)
   - Reactive vs proactive discovery lesson
   - How to prevent similar issues

**Total**: ~1,500 lines of comprehensive documentation

---

## Verification of Current Implementation (Read-Only Review)

### ✅ **What We Got Right**

Our recent tool_use/tool_result implementation is **correct and complete** for those types:

1. **Proper enum structure** - Tagged union with serde
2. **Bidirectional conversion** - Anthropic ↔ OpenAI both directions
3. **Streaming support** - All SSE events properly synthesized
4. **Stop reason mapping** - tool_use ↔ tool_calls correctly mapped
5. **Test coverage** - 274 tests passing including new tool_use tests
6. **UUID generation** - Proper toolu_ prefixed IDs when missing

### ⚠️ **Known Gaps** (Documented, Not Implemented)

**Missing from llama-proxy** (but supported by llama.cpp):
- **image** blocks - Would cause parsing errors if backend sends them

**Missing from llama-proxy** (Anthropic-only, not in llama.cpp):
- document blocks
- search_result blocks
- server_tool_use blocks
- web_search_tool_result blocks
- redacted_thinking blocks
- citations field
- cache_control field

**Impact**: Low - These won't come from llama.cpp backend currently

---

## Documentation Content Outline

### **1. anthropic_api.md** Structure

```markdown
# Anthropic Messages API - Complete Reference

## Overview
- API version, endpoint URLs
- Authentication (not applicable for proxy)

## Message Structure
- AnthropicMessage envelope
- AnthropicUsage object
- Stop reasons

## Content Block Types (16 total)

### Response Content Blocks (6 types)
1. TextBlock
2. ThinkingBlock
3. RedactedThinkingBlock
4. ToolUseBlock
5. ServerToolUseBlock
6. WebSearchToolResultBlock

### Request Content Blocks (10 types)
1. TextBlockParam
2. ImageBlockParam
3. DocumentBlockParam
4. ToolUseBlockParam
5. ToolResultBlockParam
6. ThinkingBlockParam
7. RedactedThinkingBlockParam
8. SearchResultBlockParam
9. ServerToolUseBlockParam
10. WebSearchToolResultBlockParam

[For each type: full field specs, JSON examples, usage notes]

## Streaming Events
- message_start
- content_block_start
- content_block_delta (4 delta types)
- content_block_stop
- message_delta
- message_stop

## Stop Reasons
- end_turn, max_tokens, stop_sequence, tool_use

## Extended Features
- Citations
- Cache control
- Server-side tools
```

### **2. llama_cpp_supported_types.md** Structure

```markdown
# llama.cpp Anthropic API Support

## Supported Content Block Types (5 of 16)

### Request (Inbound)
1. text
2. image (base64, url)
3. tool_use
4. tool_result

### Response (Outbound)
1. text
2. thinking (with signature)
3. tool_use

## Implementation Details
- Source file locations
- Conversion logic (Anthropic → OpenAI internal format)
- Test coverage

## Limitations vs Official Anthropic
- No document blocks
- No server-side tools
- No citations or cache_control
- No search_result blocks

## What Works
- Basic text responses
- Thinking/reasoning blocks
- Tool calling (function calling)
- Tool results in multi-turn
- Multimodal vision (images)

## What Doesn't Work
- PDF/document processing
- Web search tools
- Redacted thinking
- Citation tracking
```

### **3. openai_anthropic_mapping.md** Structure

```markdown
# OpenAI ↔ Anthropic Format Conversion

## Direct Mappings

### Text Content
Anthropic text ↔ OpenAI content string

### Thinking/Reasoning
Anthropic thinking ↔ OpenAI reasoning_text

### Tool Calling
Anthropic tool_use ↔ OpenAI tool_calls

### Images
Anthropic image ↔ OpenAI image_url

[Detailed field mappings for each]

## Cannot Map

### Anthropic → OpenAI
- document blocks → No equivalent
- search_result → No equivalent
- server_tool_use → No equivalent
- citations → No equivalent
- cache_control → No equivalent

### OpenAI → Anthropic
- system messages → system parameter (different structure)
- function calling → similar to tool_use but different schema

## Transformations Required

### tool_result
Anthropic tool_result content block → OpenAI role="tool" message

### Multiple content blocks
Anthropic [thinking, text, tool_use] → OpenAI reasoning_text + content + tool_calls

## Edge Cases
- Empty content handling
- Null vs missing fields
- Signature field passthrough
- Stop reason special cases
```

---

## Success Criteria

After creating documentation:

1. ✅ **Completeness**: All 16 official Anthropic content block types documented
2. ✅ **Clarity**: Clear distinction between llama.cpp (5 types) vs Anthropic (16 types)
3. ✅ **Accuracy**: Verified against official SDK and vendor source code
4. ✅ **Usability**: Future developers can quickly find what's supported/missing
5. ✅ **Actionable**: Clear guidance on how to extend support when needed

---

## Future Implementation Roadmap (Not in This Plan)

When image/multimodal support is needed:
1. Read `context/anthropic_api.md` for `ImageBlockParam` spec
2. Read `context/llama_cpp_supported_types.md` for backend implementation
3. Read `context/openai_anthropic_mapping.md` for conversion logic
4. Implement `Image` variant in `AnthropicContentBlock` enum
5. Add conversion functions (both directions)
6. Add streaming synthesis
7. Add tests

**Estimated effort**: 2-3 hours with documentation in place vs. 6-8 hours without
