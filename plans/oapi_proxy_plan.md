# Llama-Proxy Implementation Plan

## Executive Summary

Based on deep research into llama.cpp's HTTP API, OpenAI specifications, the llama-stream workaround proxy, and the Qwen3-Coder-Next tool call issues, here's a comprehensive plan for building `llama-proxy`.

---

## 1. Project Architecture

### Directory Structure
```
llama-proxy/
├── Cargo.toml
├── config.yaml.default
├── src/
│   ├── main.rs                 # CLI entry point
│   ├── lib.rs                  # Library exports
│   ├── config/
│   │   ├── mod.rs
│   │   └── loader.rs           # YAML config loading
│   ├── proxy/
│   │   ├── mod.rs
│   │   ├── server.rs           # HTTP proxy server
│   │   ├── handler.rs          # Request/response handling
│   │   └── streaming.rs        # SSE streaming synthesis
│   ├── fixes/
│   │   ├── mod.rs              # Fix registry trait
│   │   ├── registry.rs         # Dynamic fix registration
│   │   └── toolcall_bad_filepath_fix.rs  # Qwen3-Coder fix
│   ├── stats/
│   │   ├── mod.rs
│   │   ├── collector.rs        # Metrics extraction
│   │   └── formatter.rs        # STDOUT formatting
│   ├── exporters/
│   │   ├── mod.rs              # Exporter trait
│   │   └── influxdb.rs         # InfluxDB v2 exporter
│   └── api/
│       ├── mod.rs
│       ├── openai.rs           # OpenAI-compatible types
│       └── llama.rs            # llama.cpp-specific types
```

---

## 2. Core Dependencies

```toml
[dependencies]
# Async runtime
tokio = { version = "1", features = ["full"] }

# HTTP server & client
axum = "0.7"                    # High-performance HTTP server
reqwest = { version = "0.11", default-features = false, features = ["json", "stream"] }
tower = "0.4"
tower-http = { version = "0.5", features = ["cors", "trace"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"

# Configuration
config = "0.14"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["json", "env-filter"] }

# CLI
clap = { version = "4", features = ["derive"] }

# Time
chrono = { version = "0.4", features = ["serde"] }

# InfluxDB exporter (optional)
influxdb2 = { version = "0.5", optional = true }

# Async utilities
futures = "0.3"
async-trait = "0.1"

[features]
default = []
influxdb = ["influxdb2"]
```

---

## 3. Configuration Schema

### `config.yaml.default`
```yaml
# llama-proxy configuration
# Copy to config.yaml and modify as needed

# Proxy server settings
server:
  # Port to listen on
  port: 8066
  # Bind address
  host: "0.0.0.0"

# Backend llama-server configuration
backend:
  # Hostname or IP of llama-server
  host: "localhost"
  # Port of llama-server
  port: 8080
  # Request timeout in seconds
  timeout_seconds: 300

# Response fix modules (enable/disable)
fixes:
  # Enable all fixes by default
  enabled: true
  # Individual fix toggles
  modules:
    # Fix duplicate/malformed filePath in Qwen3-Coder tool calls
    toolcall_bad_filepath:
      enabled: true
      # If true, remove duplicate keys; if false, fix and keep both
      remove_duplicate: true

# Stats logging to STDOUT
stats:
  # Enable stats logging
  enabled: true
  # Log format: "pretty" | "json" | "compact"
  format: "pretty"
  # Log every N requests (1 = every request)
  log_interval: 1

# Remote exporters for metrics
exporters:
  # InfluxDB v2 exporter
  influxdb:
    enabled: false
    # InfluxDB server URL
    url: "http://localhost:8086"
    # Organization
    org: "my-org"
    # Bucket name
    bucket: "llama-metrics"
    # Authentication token
    token: "your-token-here"
    # Batch writes (0 = immediate)
    batch_size: 10
    # Flush interval in seconds
    flush_interval_seconds: 5
```

---

## 4. Key Implementation Details

### 4.1 Fix System Architecture

```rust
// src/fixes/mod.rs
use async_trait::async_trait;
use serde_json::Value;

/// Trait for response fix modules
#[async_trait]
pub trait ResponseFix: Send + Sync {
    /// Unique identifier for the fix
    fn name(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// Check if this fix applies to the response
    fn applies(&self, response: &Value) -> bool;

    /// Apply the fix to the response
    fn apply(&self, response: Value) -> Value;
}

/// Registry for all available fixes
pub struct FixRegistry {
    fixes: Vec<Box<dyn ResponseFix>>,
    enabled: HashMap<String, bool>,
}
```

### 4.2 Qwen3-Coder filePath Fix

The fix handles malformed JSON in tool call arguments like:
```
{"content":"valid code","filePath":"/path/to/file","filePath"/path/to/file"}
```

**Algorithm:**
1. Parse tool call `arguments` string as JSON
2. Detect duplicate `filePath` keys (or similar patterns)
3. If duplicate: Remove the second occurrence
4. If malformed but not duplicate: Fix the JSON syntax
5. Re-serialize to valid JSON string

```rust
// src/fixes/toolcall_bad_filepath_fix.rs
pub struct ToolcallBadFilepathFix {
    remove_duplicate: bool,
}

impl ResponseFix for ToolcallBadFilepathFix {
    fn name(&self) -> &str { "toolcall_bad_filepath" }

    fn description(&self) -> &str {
        "Fixes duplicate/malformed filePath in Qwen3-Coder tool calls"
    }

    fn applies(&self, response: &Value) -> bool {
        // Check for tool_calls with potentially malformed arguments
        response.get("choices")
            .and_then(|c| c.as_array())
            .map(|choices| {
                choices.iter().any(|choice| {
                    choice.get("message")
                        .and_then(|m| m.get("tool_calls"))
                        .and_then(|tc| tc.as_array())
                        .map(|calls| calls.iter().any(|call| {
                            call.get("function")
                                .and_then(|f| f.get("arguments"))
                                .and_then(|a| a.as_str())
                                .map(|args| self.is_malformed(args))
                                .unwrap_or(false)
                        }))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    }

    fn apply(&self, mut response: Value) -> Value {
        // Fix the malformed arguments
        if let Some(choices) = response.get_mut("choices").and_then(|c| c.as_array_mut()) {
            for choice in choices {
                if let Some(tool_calls) = choice.get_mut("message")
                    .and_then(|m| m.get_mut("tool_calls"))
                    .and_then(|tc| tc.as_array_mut())
                {
                    for call in tool_calls {
                        if let Some(args) = call.get_mut("function")
                            .and_then(|f| f.get_mut("arguments"))
                            .and_then(|a| a.as_str_mut())
                        {
                            *args = self.fix_arguments(args);
                        }
                    }
                }
            }
        }
        response
    }
}
```

### 4.3 Stats Collection

Extract metrics from llama.cpp's response `timings` field:

```rust
// src/stats/collector.rs
#[derive(Debug, Serialize)]
pub struct RequestMetrics {
    pub timestamp: DateTime<Utc>,
    pub model: String,
    pub client_id: Option<String>,
    pub conversation_id: Option<String>,
    // Token metrics
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    // Performance
    pub prompt_tps: f64,        // Tokens per second (prompt processing)
    pub generation_tps: f64,    // Tokens per second (generation)
    pub prompt_ms: f64,
    pub generation_ms: f64,
    // Context
    pub context_total: Option<u64>,
    pub context_used: Option<u64>,
    pub context_percent: Option<f64>,
    // Request info
    pub input_len: usize,
    pub output_len: usize,
    pub streaming: bool,
    pub finish_reason: String,
}

impl RequestMetrics {
    pub fn from_response(response: &Value, request: &Value) -> Option<Self> {
        let timings = response.get("timings")?;
        let usage = response.get("usage")?;

        Some(Self {
            timestamp: Utc::now(),
            model: response.get("model").and_then(|m| m.as_str()).unwrap_or("unknown").to_string(),
            // ... extract all fields from timings and usage
        })
    }
}
```

### 4.4 STDOUT Stats Formatting

```rust
// src/stats/formatter.rs
impl RequestMetrics {
    pub fn format_pretty(&self) -> String {
        format!(
            r#"
┌─────────────────────────────────────────────────────────────┐
│ LLM Request Metrics                                         │
├─────────────────────────────────────────────────────────────┤
│ Model: {:52}│
│ Time:  {:52}│
├─────────────────────────────────────────────────────────────┤
│ Performance                                                 │
│   Prompt Processing: {:8.2} tokens/sec ({:6.0}ms)          │
│   Generation:        {:8.2} tokens/sec ({:6.0}ms)          │
├─────────────────────────────────────────────────────────────┤
│ Tokens                                                      │
│   Input:  {:5} │ Output: {:5} │ Total: {:5}              │
├─────────────────────────────────────────────────────────────┤
│ Context: {}/{} ({:.1}%)                                    │
└─────────────────────────────────────────────────────────────┘
"#,
            self.model,
            self.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
            self.prompt_tps, self.prompt_ms,
            self.generation_tps, self.generation_ms,
            self.prompt_tokens, self.completion_tokens, self.total_tokens,
            self.context_used.unwrap_or(0),
            self.context_total.unwrap_or(0),
            self.context_percent.unwrap_or(0.0)
        )
    }
}
```

### 4.5 InfluxDB Exporter

```rust
// src/exporters/influxdb.rs
use influxdb2::Client;
use influxdb2::models::DataPoint;

pub struct InfluxDbExporter {
    client: Client,
    bucket: String,
}

#[async_trait]
impl MetricsExporter for InfluxDbExporter {
    async fn export(&self, metrics: &RequestMetrics) -> Result<()> {
        let point = DataPoint::builder("llama_request")
            .tag("model", &metrics.model)
            .field("prompt_tps", metrics.prompt_tps)
            .field("generation_tps", metrics.generation_tps)
            .field("prompt_tokens", metrics.prompt_tokens as f64)
            .field("completion_tokens", metrics.completion_tokens as f64)
            .field("total_tokens", metrics.total_tokens as f64)
            .field("prompt_ms", metrics.prompt_ms)
            .field("generation_ms", metrics.generation_ms)
            .timestamp(metrics.timestamp.timestamp_nanos())
            .build()?;

        self.client.write(&self.bucket, vec![point]).await?;
        Ok(())
    }
}
```

---

## 5. Proxy Handler Flow

```
┌──────────────────────────────────────────────────────────────┐
│                     Client Request                           │
└─────────────────────────────┬────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────┐
│                    Proxy Server (axum)                       │
│  - Parse request                                             │
│  - Extract client/conversation ID                            │
└─────────────────────────────┬────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────┐
│               Forward to llama-server                        │
│  - HTTP connection to backend                                │
│  - Stream or non-stream based on request                     │
└─────────────────────────────┬────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────┐
│                   Response Processing                        │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ For each fix module (if enabled):                      │ │
│  │   if fix.applies(response) {                           │ │
│  │     response = fix.apply(response)                     │ │
│  │   }                                                    │ │
│  └────────────────────────────────────────────────────────┘ │
└─────────────────────────────┬────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────┐
│                    Stats Collection                          │
│  - Extract metrics from response                             │
│  - Log to STDOUT (if enabled)                                │
│  - Send to exporters (if enabled)                            │
└─────────────────────────────┬────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────┐
│                   Return to Client                           │
└──────────────────────────────────────────────────────────────┘
```

---

## 6. CLI Interface

```rust
// src/main.rs
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "llama-proxy")]
#[command(about = "HTTP reverse proxy for llama.cpp server")]
struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "config.yaml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the proxy server
    Run,

    /// List all available response fix modules
    ListFixes {
        /// Show detailed information
        #[arg(short, long)]
        verbose: bool,
    },

    /// Validate configuration file
    CheckConfig,

    /// Test connection to backend
    TestBackend,
}
```

**CLI Usage:**
```bash
# Start proxy
llama-proxy run --config config.yaml

# List all fix modules
llama-proxy list-fixes
llama-proxy list-fixes --verbose

# Validate config
llama-proxy check-config

# Test backend connection
llama-proxy test-backend
```

---

## 7. Implementation Phases

### Phase 1: Core Proxy (MVP)
- [ ] Project setup with Cargo.toml
- [ ] YAML configuration loading
- [ ] Basic HTTP proxy with axum
- [ ] Forward requests to llama-server
- [ ] Return responses to client

### Phase 2: Fix System
- [ ] Fix trait and registry
- [ ] `toolcall_bad_filepath_fix` implementation
- [ ] Configuration for enabling/disabling fixes
- [ ] CLI `list-fixes` command

### Phase 3: Stats & Logging
- [ ] Metrics extraction from responses
- [ ] STDOUT pretty printing
- [ ] JSON logging format option
- [ ] Request timing/tracking

### Phase 4: Exporters
- [ ] Exporter trait
- [ ] InfluxDB v2 exporter
- [ ] Batch writing support
- [ ] Graceful shutdown with flush

### Phase 5: Polish
- [ ] Error handling
- [ ] Graceful shutdown
- [ ] Health check endpoint
- [ ] Documentation

---

## 8. Key Insights from Research

### From llama.cpp API:
- `timings` field in responses contains `prompt_per_second` and `predicted_per_second`
- `/slots` endpoint shows context usage via KV cache
- `/metrics` (Prometheus format) has `llamacpp:kv_cache_usage_ratio`

### From llama-stream:
- Forces `stream: false` to backend for tool calls
- Synthesizes SSE streaming from complete response
- This workaround may fix tool call issues

### From HuggingFace discussion:
- Qwen3-Coder-Next has malformed JSON in tool calls
- Duplicate `filePath` keys with broken JSON syntax
- Disabling streaming may naturally fix the issue

---

## 9. Future Considerations

- **Streaming synthesis**: Optionally force non-streaming to backend and synthesize SSE (like llama-stream)
- **Additional fix modules**: Easy to add more fixes via the plugin system
- **More exporters**: Prometheus, Grafana Loki, etc.
- **Authentication pass-through**: Forward auth headers to backend
- **Request/response logging**: Optional full request logging for debugging

---

## 10. API Types Reference

### OpenAI Chat Completion Response
```json
{
  "id": "chatcmpl-xxx",
  "object": "chat.completion",
  "created": 1735142223,
  "model": "model-name",
  "choices": [{
    "index": 0,
    "message": {
      "role": "assistant",
      "content": "response text",
      "tool_calls": [{
        "id": "call_xxx",
        "type": "function",
        "function": {
          "name": "function_name",
          "arguments": "{\"param\": \"value\"}"
        }
      }]
    },
    "finish_reason": "stop"
  }],
  "usage": {
    "prompt_tokens": 10,
    "completion_tokens": 5,
    "total_tokens": 15
  },
  "timings": {
    "prompt_n": 10,
    "prompt_ms": 100.5,
    "prompt_per_second": 99.5,
    "predicted_n": 5,
    "predicted_ms": 50.2,
    "predicted_per_second": 99.6
  }
}
```

### Streaming SSE Format
```
data: {"id":"chatcmpl-xxx","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}

data: {"id":"chatcmpl-xxx","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}

data: {"id":"chatcmpl-xxx","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
```

---

This plan provides a solid foundation for building a robust, extensible llama-proxy that addresses the immediate needs (fixing Qwen3-Coder tool calls, logging stats) while being designed for future extensibility.
