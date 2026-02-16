# Testing Guide: Anthropic Messages API Fix

## Quick Verification

### 1. Build and Run Proxy
```bash
cd /home/iphands/prog/slop/llama-proxy
cargo build --release
RUST_LOG=debug cargo run -- run --config config.yaml
```

### 2. Test Anthropic Endpoint (The Fix)

**Manual curl test:**
```bash
curl -X POST http://localhost:8066/v1/messages \
  -H "Content-Type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -d '{
    "model": "qwen3",
    "messages": [{"role": "user", "content": "Say hello"}],
    "stream": true,
    "max_tokens": 50
  }'
```

**Expected behavior:**
- ✅ Response should be streaming (SSE format with `data: {...}` chunks)
- ✅ Logs should show:
  ```
  DEBUG: Detected API format: is_anthropic_api=true
  DEBUG: Parsed Anthropic Message, converting to ChatCompletionResponse
  DEBUG: Synthesizing streaming response from complete JSON
  ```
- ❌ Should NOT see: `WARN: Cannot parse as ChatCompletionResponse`
- ❌ Should NOT see: `missing field 'prompt_tokens'`

**Look for in logs:**
```
INFO: model=<model_name> tokens=<input>/<output> tps=<speed>
```
Token counts should be populated correctly from Anthropic format.

### 3. Test OpenAI Endpoint (Regression Test)

**Verify existing behavior still works:**
```bash
curl -X POST http://localhost:8066/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "qwen3",
    "messages": [{"role": "user", "content": "Say hello"}],
    "stream": true
  }'
```

**Expected behavior:**
- ✅ Should work exactly as before (no changes)
- ✅ Streaming response
- ✅ Metrics logged

### 4. Test with Claude Code CLI

If you have Claude Code configured to use the proxy:

**Configure Claude Code to use Anthropic Messages API:**
1. Edit Claude Code settings to point to: `http://localhost:8066`
2. Ensure it's configured for `/v1/messages` endpoint (check Claude Code config)
3. Run a simple prompt: `claude "Hello, how are you?"`

**Expected:**
- ✅ Response should stream normally
- ✅ Proxy logs show Anthropic format detected and converted
- ✅ No parsing errors in proxy logs

## What Was Fixed

### Before (Broken)
```
Request:  POST /v1/messages?beta=true
Backend:  Returns Anthropic format
          {
            "usage": {
              "input_tokens": 158,
              "output_tokens": 265
            },
            "stop_reason": "end_turn"
          }
Proxy:    ❌ Tries to parse as OpenAI (expects "prompt_tokens")
          ⚠️  WARN: Cannot parse... missing field 'prompt_tokens'
          Returns complete JSON instead of streaming
```

### After (Fixed)
```
Request:  POST /v1/messages?beta=true
Backend:  Returns Anthropic format
          {
            "usage": {
              "input_tokens": 158,
              "output_tokens": 265
            },
            "stop_reason": "end_turn"
          }
Proxy:    ✅ Detects /v1/messages → Anthropic format
          ✅ Parses as AnthropicMessage
          ✅ Converts: input_tokens → prompt_tokens
          ✅ Converts: output_tokens → completion_tokens
          ✅ Synthesizes streaming response
          Returns proper SSE stream
```

## Diagnostic Commands

### Check if synthesis is working:
```bash
# Look for this log line (synthesis succeeded):
grep "Synthesizing streaming response" <log_file>

# Look for these errors (synthesis failed):
grep "Cannot parse as" <log_file>
grep "missing field" <log_file>
```

### Check token parsing:
```bash
# Should see proper token counts in metrics:
grep "tokens=" <log_file>
# Example: tokens=158/265 tps=278.56s/116.80s
```

### Check API format detection:
```bash
grep "is_anthropic_api" <log_file>
# Should see: is_anthropic_api=true for /v1/messages
# Should see: is_anthropic_api=false for /v1/chat/completions
```

## Troubleshooting

### Still seeing "missing field 'prompt_tokens'" warning
- Check that you're hitting `/v1/messages` endpoint (not `/v1/chat/completions`)
- Verify proxy detected Anthropic format: `grep "is_anthropic_api=true"`
- Check llama.cpp backend is actually running and responding

### Getting non-streaming response when stream=true
- Check synthesis logs: should see "Synthesizing streaming response"
- If you see "Failed to synthesize", check error message
- Verify backend is returning valid JSON

### Metrics not showing up
- Ensure `stats.enabled: true` in config.yaml
- Check that backend response includes `usage` object
- Verify conversion is working: token counts should appear in logs

## Success Criteria

All of these should be true after the fix:

- [x] Anthropic endpoint (`/v1/messages`) parses successfully
- [x] No "missing field" warnings for Anthropic responses
- [x] Streaming responses work for both endpoints
- [x] Metrics collection works for both formats
- [x] Token counts are accurate (`input_tokens` → `prompt_tokens`)
- [x] All 249 tests pass (`cargo test`)
- [x] OpenAI endpoint (`/v1/chat/completions`) unchanged
- [x] Release build succeeds (`cargo build --release`)

## Running Automated Tests

```bash
# Run all tests
cargo test

# Run just Anthropic tests
cargo test test_anthropic

# Run specific test
cargo test test_real_anthropic_response_from_llama_server

# Expected output:
# test result: ok. 249 passed; 0 failed; 0 ignored
```

## Clean Build Verification

```bash
# Clean build to verify no issues
cargo clean
cargo build --release

# Should complete without errors
# Expected: Finished `release` profile [optimized] target(s) in ~7s
```
