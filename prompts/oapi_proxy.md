Do deep research and build an AWESOME plan to build this llama-proxy project

# llama-proxy

I want a sub-project called `llama-proxy`.
This should be a Rust HTTP proxy that always connects to a locally running `llama.cpp` `llama-server` instance and acts as a reverse proxy.

## Goals / requirements

We should be able to easily configure the proxy via a YAML file... there should be a `config.yaml.default` with examples and comments.
For now assume we connect to locahost over HTTP so no SSL/TLS stuff is needed just a hostname and port number (allow remote hosts or by name for now but no SSL)

NOTE: Let users enable/disable response fixes in the YAML. Also the CLI should have a sub-command(s) to list all fixes

## Read API spec

If needed see:
- `./vendor/llama.cpp`
- `./vendor/openapi-sec`

To understand the HTTP API sepcs

The proxy should work as a generic OpenAPI LLM proxy **BUT** we are primarily running this on top of llama.cpp for now
So if there are llama.cpp vendor specific APIs / shapes that support our goals lets use them!!

### Awesome stats logging

We want the proxy to be able to show and log awesome data about the current tokens persecond and performance of LLM server
We should log generation tokens per second AND prompt processing tokens per second to STDOUT
- model name
- time executed
- client id / conversation id (or some similar info)
- tokens per second (prompt processing and generation)
- context total / used (raw nums and %)
- input len / output len

### Logging to remote systems

We want to be able to stream this performance info to some remote system.
It should be have pliuggable interface but at first lets implement streaming to influxdb
We want to capture:
- model name
- time executed
- client id / conversation id (or some similar info)
- tokens per second (prompt processing and generation)
- context total / used (raw nums and %)
- input len / output len

Dont enable this plugin by default

### Fix by disabling streaming ??

Read and study: https://huggingface.co/unsloth/Qwen3-Coder-Next-GGUF/discussions/2
The see the `vendor/llama-stream` mention. The auther is not sure why BUT this seems to coincidentally fix some tool call issues!
We should try and extract whatever in theis project fixes the issue... IT MIGHT be as simple as... if we disable streaming mode the issue goes away naturally.

### Response fixes / tool call fixes

We want to fix issues in that some models have when sending back tool call responses.
THIS MIGHT be hyper specifc at first BUT we should allow for extensions.
Some easy pluggable API that lets us detect the issue in the response, and if found runs function to fix the issue.

At first our main issue to fix is this as experienced in the Qwen3-Coder-Next LLM responses. We see toolcalls errors like
```
Invalid input for tool write: JSON parsing failed: Text: {"content":"valid code","filePath":"/path/to/file","filePath"/path/to/file"}.
Error message: JSON Parse error: Unrecognized token '/'
```

Here there are issues in the toolcall `"filePath":"/path/to/file","filePath"/path/to/file"`
- filePath is defined twice in the JSON
- the second SHOULD just be removed
- the second is invalid JSON `"filePath"/path/to/file"` should be `"filePath":"/path/to/file"`

We should do two things here:
- IF we see that the keys are duplicated just remote the second and fix the JSON so it parses
- IF we see that the keys are NOT duplicated fix up the second and keep both (JSON should parse)

Call this fix/module `fix_toolcall_bad_filepath_fix.rs`
