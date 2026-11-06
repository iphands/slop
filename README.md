# Slop!

A place to house random AI-assisted experiments. Cool stuff may migrate away from here into
standalone projects.

**Start here**

- **`cache/`** — LAN package cache. Build/run/setup: [`cache/README.md`](cache/README.md)
- **`qbots/`** — Quake 2 bots. Status + measurements: [`qbots/README.md`](qbots/README.md)
- **`qctrl/`** — Quake 2 server controller: [`qctrl/AGENTS.md`](qctrl/AGENTS.md)
- Everything else is one row each in [Smaller work](#smaller-work) or [Shared resources](#shared-resources).

## The plan system — `skills/hands-plan`

`cache`, `qbots`, `qctrl` and `gpu` are planned the same way; `skills/hands-plan` — a versioned
Claude skill (`VERSION`, `CHANGELOG.md`, `bin/hp-selftest`) — scaffolds and drives that system.
Six verbs — `init`, `new`, `status`, `resume`, `close`, `resync` — in `skills/hands-plan/SKILL.md`;
`status` and `resync` never write.

A project on the system carries `context/plans/RULES.md` (its own rules, cited by section id),
`SERIES.md` (the numbered index), `NN_name.md` beside its `NN_name_tracker.md`, and `completed/` /
`abandoned/`. Current counts live in each `SERIES.md`, not here. `resume` commits one task at a
time, and the bracket format is each project's own rule — e.g. qctrl's
`[qctrl][P14][T7][stamp]`; a bare `[seat1]` prefix is just project tagging, not the plan system.

Only `qctrl` is stamped `hands-plan:v1` and carries the root `hands-plan:begin` block;
`qbots`, `cache` and `gpu` run plan systems that predate the skill and are not yet migrated
(`/hands-plan resync` prints what migrating would involve, without doing it). The directories
below are single-purpose — a crate, a patch, some scripts, a findings write-up — with no plan
history.

## Sub-projects

### cache — pkgcache: LAN package caching proxy (Debian + Fedora)

A single lightweight **nginx reverse caching proxy** caching OS package downloads for a fleet of
Debian and Fedora machines. First machine to download a
package pulls it from the internet; every machine after gets it from the local cache over the LAN.

- Cache lives on a **bind-mounted volume** (`/srv/pkgcache/data` by default).
- Runs **rootless** as `APP_UID:APP_GID` (default `1000:1000`).
- `build` / `publish` / `run` **auto-detect podman/docker and prefer podman**.
- `containers/fedora/` builds a client image with dnf pre-wired to the cache (release pinned by
  `FEDORA_RELEASE` in `cache/containers/fedora/Dockerfile`), alongside a `pkgcache-setup`
  re-pointing script.
- The optional stats dashboard (per-client hit ratios, bytes saved, top packages) is an in-tree
  **Rust workspace** at `stats/` — `crates/ingest` and `crates/stats`.

The hard requirement is caching **both** apt and dnf. Fedora's `dnf` fetches over **HTTPS +
metalink** (dynamic mirror selection), which rules out forward proxies — nginx is a **reverse
cache** that originates upstream TLS itself, so clients speak plain HTTP to it. Packages stay
GPG-verified end-to-end.

### qbots — external Quake 2 bot clients

A multi-threaded **Rust** program that connects to a real Quake 2 server over UDP and impersonates
genuine clients — external bots that log in like real players and fight in a deathmatch using only
what the server sends over the wire.

Unlike classic Q2 bots (which run *inside* the server as gamecode), qbots is a separate program on
the far end of a socket. It sees only the protocol traffic, so it rebuilds the world itself: the
wire codec, the connection handshake, frame decoding, and a `.bsp` collision model + navigation
graph parsed locally. Six crates: `q2proto`, `world`, `client`, `brain`, `qbots`, `tools`.

**The full pipeline works live against Yamagi Q2:** a bot connects, perceives, navigates, fights
and respawns. `spawn-to-spawn --count 24` on a parked q2dm1 measures **effective 13–15/24**
(2026-09-12; a regression gate, not a capability claim). An earlier **24/24** headline is retired —
`qbots/README.md` says why, and lists the constants a run must match to be comparable.

### qctrl — Quake 2 server controller

Rust REST API (`crates/api`, `crates/rcon`) + React/TypeScript frontend (`frontend/`, built with
Vite) for managing a Quake 2 deathmatch server over RCON.
Mobile-responsive UI to send commands, edit server config, pick maps without typing, and watch a
live log stream over WebSocket (`/api/logs/ws`) — which is qctrl's own activity log; **the Q2
server console is not streamed.**

## Smaller work

*Current state of any directory: `git log -1 --format=%cd -- <dir>`.*

Homelab and machine work:

| Dir | What |
|---|---|
| `immich/` | Job-throughput TUI for Immich queues (`immich-jobrate`) — Prometheus counters and a REST backlog-drain fallback, plus a worked NVENC-over-IP setup |
| `gpu/` | Performance investigation on one named Intel Arc box — profiling and power-limit scripts, Mesa/kernel patch series, capture summarizers |
| `affinity/` | CPU/GPU contention study of a two-seat shared box, with `game-affinity` to pin each user's game tree, desktop and GPU IRQ to one CCD |
| `seat1/` | Multi-seat bring-up helpers — a seat-aware labwc launcher (GPU from the *seat*, not the username) and a lightdm non-seat0 greeter patch |
| `air/` | Laptop s2idle drain scripts — a `systemd-sleep` hook that unloads Wi-Fi/BT and disarms wakeup sources, plus drain and hook tests |
| `readahead-opt/` | Measured write-up on btrfs/NFS readahead under contention, from an overnight fio sweep |
| `cron/` | Container that toggles a Home Assistant plug on a schedule |

Experiments and toys:

| Dir | What |
|---|---|
| `qcontainer/` | Fedora images of optimized Quake II dedicated servers (Fedora pinned for SONAME parity), published as `iphands/quake2` (`yquake`, `q2repro`) |
| `rt/ascii-rt-glm5/` | Terminal ray tracer — Vulkan rendering with Unicode half-blocks and a CPU fallback |
| `rt/rt-rs/` | Hardware ray tracing over Vulkan KHR (BLAS/TLAS). **WIP:** pipeline setup and image output still TODO |
| `statusline/` | Fork of ClaudeCodeStatusLine, showing the full working directory path |
| `modeltest/sarvam/` | Sieve of Eratosthenes in Hindi and Telugu — Sarvam-105B script-handling test artifacts |

## Shared resources

- **`context/`** — cross-project knowledge base: library notes (`ash`, `rayon`, `glam`,
  `crossterm`), `algo.md`, `patterns.md`, `high_level.md` pros/cons tables, and `pitfalls.md` — the
  cross-project bug log; `qbots`, `cache`, `qctrl` and `gpu` keep mirrored ones
- **`skills/hands-plan/`** — the plan system, above
- **`scripts/`** — shared utilities (`migrate.sh` for git migration, `target-sizes.sh`,
  `lightdm-restart-seat`)
- **`plans/`** — **archived.** Plans for `llama-proxy`, a sub-project that no longer lives here.
  Kept as history; `prompts/oapi_proxy.md` is the same era
- **`prompts/`** — prompt templates

## Git discipline

See [`AGENTS.md`](AGENTS.md). History is **append-only** — no `--amend`, `rebase`, `reset --hard`,
`push --force`, or `revert`; a wrong commit is corrected by a new one. Chain edit-then-commit with
`&&`. **Never push** — the human pushes after review.
