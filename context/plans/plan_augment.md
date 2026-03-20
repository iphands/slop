# Augment Backend Feature Plan

## Context

Add an experimental "augment-backend" feature that uses a fast LLM to enrich request/response data before sending to the primary backend. This is a separate, non-rotating backend that:

1. Receives extracted user content wrapped in a prompt from `./augmenter/backend_prompt.md`
2. Returns enhanced content that gets concatenated with original user input and response
3. Operates independently from the normal backend rotation system

## Architecture Overview

```
Client Request → Extract user content → Wrap with prompt → Send to augment-backend
              → Receive augmentation → Concatenate: user_content + prompt + augmentation
              → Send enriched request to primary backend
              → Return enriched response to client
```

## Configuration Structure

Add to `config.yaml`:

```yaml
augment-backend:
  enabled: true                    # Optional toggle, defaults to true if present
  url: "http://cosmo.lan:8701"
  model: "cosmo-6000"
  prompt_file: "./augmenter/backend_prompt.md"  # Optional, defaults to this path
```

## Implementation Components

### 1. Configuration (`src/config/mod.rs`)

**New types:**
```rust
pub struct AugmentBackendConfig {
    pub url: String,
    pub model: String,
    pub prompt_file: Option<String>,  // Path to prompt.md file
}
```

**Integration:**
- Add `augment_backend: Option<AugmentBackendConfig>` to `AppConfig`
- Update config loader to parse this section
- Update config validation

### 2. Augment Backend Client (`src/augment/mod.rs` + `src/augment/client.rs`)

**New module structure:**
```rust
// src/augment/mod.rs
pub struct AugmentBackend {
    url: String,
    model: String,
    prompt_file: String,
    http_client: reqwest::Client,
}

impl AugmentBackend {
    async fn get_augmentation(&self, user_content: &str) -> Result<String>
    fn load_prompt(&self) -> Result<String>
    fn detect_api_format(&self) -> ApiFormat  // OpenAI or Anthropic
}
```

**Key methods:**
- `get_augmentation(user_content)`: Sends wrapped prompt to augment-backend, extracts text response
- `load_prompt()`: Reads `prompt_file` from disk
- `detect_api_format()`: Auto-detects based on URL pattern or config
- Supports both OpenAI and Anthropic API formats

### 3. Message Extraction (`src/augment/extraction.rs`)

**New module for extracting user content:**

```rust
pub fn extract_user_content(messages: &[Message]) -> Vec<String>
```

**Handles both formats:**
- OpenAI: `ChatCompletionRequest.messages` - extract all user role messages
- Anthropic: `AnthropicMessage.content` - extract user message blocks

Returns extracted text content to send to augment-backend.

### 4. Augmentation Injection (`src/augment/injection.rs`)

**New module for injecting augmentation into requests:**

```rust
pub fn inject_augmentation(
    request: ChatCompletionRequest,
    augmentation: &str
) -> ChatCompletionRequest
```

**Logic:**
- Append augmentation to the LAST user message content
- Format: `original_content\n\n<augmentation>\n\n<augmentation_result>`
- If no user message found, prepend to first message
- Preserve all other fields (tools, parameters, etc.)

### 4. Response Concatenation (`src/augment/enrichment.rs`)

**New module for enriching responses:**

```rust
pub fn enrich_response(
    original_user_content: &str,
    prompt_content: &str,
    augmentation: &str,
    response: ChatCompletionResponse
) -> ChatCompletionResponse
```

**Logic:**
- Concatenate: `original_user_content + "\n\n" + prompt_content + "\n\n" + augmentation`
- Replace/insert into response content field
- Preserve all other fields (tool_calls, reasoning_text, etc.)

### 5. Integration with Request Handler (`src/proxy/handler.rs`)

**Modified flow:**

```rust
pub async fn handle(&self, req: Request<Body>) -> Response {
    // 1. Parse request
    let request_json = parse_request(&body_bytes)?;

    // 2. Check augment-backend config
    let augment_config = self.state.config.augment_backend.as_ref();

    // 3. Extract user content if augment is enabled
    let user_content = if let Some(cfg) = augment_config {
        if cfg.enabled && has_user_messages(&request_json.messages) {
            Some(extract_user_content(&request_json.messages))
        } else {
            None
        }
    } else {
        None
    };

    // 4. Call augment-backend (blocks if enabled and user content exists)
    let augmentation = if let Some(cfg) = augment_config {
        if cfg.enabled && user_content.is_some() {
            // This will block on augment-backend response
            augment_backend.get_augmentation(user_content.unwrap()).await?
        } else {
            None
        }
    } else {
        None
    };

    // 5. Inject augmentation into request (appends to user message)
    let enriched_request = if let Some(aug) = augmentation {
        inject_augmentation(request_json, &aug)?
    } else {
        request_json
    };

    // 6. Send to primary backend with enriched request
    let backend_response = send_to_backend(enriched_request).await?;

    // 7. Return response (augmentation already in context)
    Ok(backend_response)
}
```

### 6. Prompt File Structure (`augmenter/backend_prompt.md`)

**User-created file with this format:**

```
<augmenter_prompt>
<user_content>
```

**Example prompt.md content:**
```
You are an augmentation assistant. Provide a brief, relevant context
or expansion for the following user input. Keep responses concise.

User input:
```
```

The proxy will append user content after this prompt.

## Critical Files to Modify/Create

| File | Action | Purpose |
|------|--------|---------|
| `src/config/mod.rs` | Modify | Add `AugmentBackendConfig` struct with `enabled` flag |
| `src/config/loader.rs` | Modify | Parse augment-backend config section |
| `src/augment/mod.rs` | Create | Module root, `AugmentBackend` struct, `ApiFormat` enum |
| `src/augment/client.rs` | Create | HTTP client for augment-backend with format detection |
| `src/augment/extraction.rs` | Create | Extract user content from OpenAI/Anthropic requests |
| `src/augment/injection.rs` | Create | Inject augmentation into user message content |
| `src/proxy/handler.rs` | Modify | Integrate augment-backend flow before primary backend call |
| `config.yaml.default` | Modify | Add augment-backend example config |
| `augmenter/backend_prompt.md` | Create (user) | User creates this prompt file with `<augmenter_prompt>` wrapper |

## Key Design Decisions

### 1. When to Augment

**Decision:** Augment on every request (when configured and enabled)

**Rationale:** Simple, predictable behavior. User can enable/disable via config flag.

### 2. How to Inject Augmentation

**Decision:** Append augmentation to user message content

**Rationale:**
- User explicitly requested this behavior
- Keeps augmentation contextually tied to user input
- Format: `<original>\n\n<augmenter_prompt>\n\n<augmentation_result>`

### 3. API Format for Augment Backend

**Decision:** Auto-detect format (OpenAI or Anthropic)

**Rationale:**
- Config flag or URL pattern determines format
- Most flexible for different backend providers
- Can default to OpenAI if undetectable

### 4. Error Handling

**Decision:** Fail closed - if augment-backend fails, return error to client

**Rationale:**
- Makes augmentation failure explicit and visible
- Better for debugging the experimental feature
- User can disable augment-backend if they want fallback behavior

## Verification Steps

1. **Config loading:**
   ```bash
   cargo run -- check-config --config config.yaml
   ```

2. **Augment backend connectivity:**
   ```bash
   cargo run -- test-augment-backend --config config.yaml
   ```

3. **End-to-end test:**
   ```bash
   # Start proxy
   cargo run -- run --config config.yaml

   # Send request through proxy
   curl -X POST http://localhost:8066/v1/chat/completions \
     -H "Content-Type: application/json" \
     -d '{"model":"qwen3","messages":[{"role":"user","content":"Hello"}]}'
   ```

4. **Verify augmentation in response:**
   - Check logs for augment-backend request/response
   - Verify response contains enriched content
