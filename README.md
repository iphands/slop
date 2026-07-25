# Slop!

A place to house random AI-assisted experiments. Cool stuff may migrate away from here into standalone projects.

---

## Sub-projects

### cache — pkgcache: LAN package caching proxy (Debian + Fedora)

`cache` (aka `pkgcache`) is a single, lightweight **nginx reverse caching proxy** that caches OS package downloads for a fleet of Debian 13 (Trixie) and Fedora 44 machines. First machine to download a package pulls it from the internet; every machine after that gets it from the local cache over the LAN.

- Cache lives on a **bind-mounted volume** (`/srv/pkgcache/data` by default).
- Runs **rootless** as `APP_UID:APP_GID` (default `1000:1000`).
- `build` / `publish` / `run` **auto-detect podman/docker and prefer podman**.
- An optional second container serves a **stats dashboard** — per-client hit ratios, bytes saved, top packages.

The hard requirement is caching **both** apt and dnf. Fedora's `dnf` fetches over **HTTPS + metalink** (dynamic mirror selection), which rules out forward proxies — nginx is a **reverse cache** that originates upstream TLS itself, so clients speak plain HTTP to it. Packages stay GPG-verified end-to-end.

> **Note:** This project was previously named `llama-proxy` and has been renamed to `cache`.

---

### qbots — external Quake 2 bot clients

`qbots` is a multi-threaded **Rust** program that connects to a real Quake 2 server over UDP and impersonates genuine clients — external bots that log in like real players and fight in a deathmatch using only what the server sends over the wire.

Unlike classic Q2 bots (which run *inside* the server as gamecode), qbots is a separate program on the far end of a socket. It sees only the protocol traffic, so it rebuilds the world itself: the wire codec, the connection handshake, frame decoding, and — the novel part — a `.bsp` collision model + navigation graph parsed locally.

**Status: full pipeline works live against Yamagi Q2.** A bot connects, perceives, navigates, fights, and respawns; an N-bot fleet fills a server. `spawn-to-spawn` reaches **24/24** on q2dm1 at the default grid spacing.

---

### qctrl — Quake 2 server controller

`qctrl` is a Rust REST API + React TypeScript frontend for managing a Quake 2 deathmatch server via RCON. It provides a mobile-responsive web interface to send commands, manage server config, select maps via UI (no typing), and stream real-time server logs.

---

### qcontainer — optimized Quake II dedicated server images

`qcontainer` builds Fedora 44 containers with optimized Quake II dedicated servers:

```
-O3 -march=sandybridge -mtune=sandybridge -O3 -pipe -falign-functions=32 -fomit-frame-pointer
```

Two flavors published as `iphands/quake2`:
- **`yquake`** — yquake2 `q2ded`, native vanilla protocol (34)
- **`q2repro`** — q2repro `q2reproded`, patched to also accept vanilla/R1Q2/Q2PRO clients

---

### cron — plug automation

`cron` is a self-contained Docker container that automates plug control via the Home Assistant API on a schedule (e.g. Mon 17:30 → ON, Tue 04:00 → OFF).

---

### statusline — Claude Code status line fork

`statusline` is a fork of [daniel3303/ClaudeCodeStatusLine](https://github.com/daniel3303/ClaudeCodeStatusLine), customized to always show the full working directory path (not just the basename), with `$HOME` collapsed to `~` boundary-aware and the `basename@branch` segment dropped outside git repos.

---

### rt/ascii-rt-glm5 — terminal ray tracer

`ascii-rt-glm5` is a Vulkan-accelerated ray tracer that renders entirely into your terminal using Unicode half-block characters for 2x vertical resolution. It renders a Cornell box scene with a bouncing sphere, supports multiple light bounces, and runs at ~10 FPS in interactive mode. Arrow keys control the light height and bounce count, bracket keys zoom the camera, and spacebar pauses the scene.

The Vulkan path is optional — it falls back to CPU rendering gracefully, so it runs anywhere. Great for staring at on a second monitor while you pretend to work.

---

### rt/rt-rs — RTX ray tracer (WIP)

`rt-rs` is a work-in-progress minimal Rust ray tracer built on top of NVIDIA RTX hardware via the Vulkan Ray Tracing (`KHR`) extensions. It handles Vulkan instance setup, device selection, queue management, and the acceleration structure framework (BLAS + TLAS). Shader compilation, pipeline setup, and image output are still on the TODO list.

It's the more "serious" counterpart to `ascii-rt-glm5` — a ground-up exploration of how hardware ray tracing actually works at the Vulkan level, without a framework hiding the details.

---

### modeltest/sarvam — prime sieve experiments

`modeltest/sarvam` contains simple Sieve of Eratosthenes implementations written in Hindi (Devanagari script) and Telugu — artifacts from testing the Sarvam-105B model's ability to handle non-Latin scripts.

---

## Shared resources

- **`vendor/`** — cloned upstream source for deep reading (ash, llama.cpp, opencode, q2repro, yquake2, etc.)
- **`context/`** — knowledge base: library docs, algorithms, design patterns, pitfalls
- **`scripts/`** — shared utilities (e.g. `migrate.sh` for git migration)
- **`plans/`** — work plans for the proxy project
- **`prompts/`** — prompt templates

---

## Git discipline

See [`AGENTS.md`](AGENTS.md) for the full git discipline rules. Key points:
- History is **append-only** — never `--amend`, `rebase`, `reset --hard`, `push --force`, or `revert`
- Chain edit-then-commit with `&&`
- Small, frequent commits — one logical change each
- **Never push** — the human pushes after review
