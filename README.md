# Slop!

A place to house random AI-assisted experiments. Cool stuff may migrate away from here into standalone projects.

---

## Sub-projects

### llama-proxy

`llama-proxy` is a Rust HTTP reverse proxy that sits between AI coding tools (like Claude Code or Opencode) and a locally-running [llama.cpp](https://github.com/ggerganov/llama.cpp) server. It transparently proxies all requests while adding three layers of value on top: a pluggable response-fix system that repairs malformed LLM output (like broken JSON in tool calls from models such as Qwen3-Coder), a metrics collector that captures tokens/sec, context window usage, and timing data, and a telemetry exporter that ships those metrics off to InfluxDB for dashboarding. Output can be formatted as pretty terminal boxes, JSON, or compact single-line logs.

What makes it fun is that it's a genuine daily-driver tool — point your AI coding assistant at it instead of llama.cpp and you get real-time insight into exactly how your local model is performing, with automatic fixes for the quirks that would otherwise silently break your workflow.

---

### rt/ascii-rt-glm5

`ascii-rt-glm5` is a Vulkan-accelerated ray tracer that renders entirely into your terminal using Unicode half-block characters for 2x vertical resolution. It renders a Cornell box scene with a bouncing sphere, supports multiple light bounces, and runs at ~10 FPS in interactive mode. Arrow keys control the light height and bounce count, bracket keys zoom the camera, and spacebar pauses the scene.

It's a delightful experiment in taking something that usually requires a GUI (ray tracing) and shoving it stubbornly into a terminal. The Vulkan path is optional — it falls back to CPU rendering gracefully, so it runs anywhere. Great for staring at on a second monitor while you pretend to work.

---

### rt/rt-rs

`rt-rs` is a work-in-progress minimal Rust ray tracer built on top of NVIDIA RTX hardware via the Vulkan Ray Tracing (`KHR`) extensions. It handles Vulkan instance setup, device selection, queue management, and the acceleration structure framework (BLAS + TLAS). Shader compilation, pipeline setup, and image output are still on the TODO list.

It's the more "serious" counterpart to `ascii-rt-glm5` — a ground-up exploration of how hardware ray tracing actually works at the Vulkan level, without a framework hiding the details.
