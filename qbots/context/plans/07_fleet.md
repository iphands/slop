# Plan 07 — Fleet (`qbots` binary)

> **Status**: pending
> **Created**: 2026-06-14
> **Depends on**: Plan 06 (brain)
> **Goal**: Run many bots from one process — config-driven roster, supervised lifecycle,
> per-bot logging, and rate-safe connection pacing — so a fleet of bots fills a deathmatch
> server and stays up.
> **Agent**: implementation agent (ralph-loop) | sub-agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Build out the `qbots` binary: a roster config, a supervisor that spawns one tokio
task per bot (sharing an `Arc<World>`), staggered/retried connects, structured logging, and
a clean CLI. Replaces the Plan 03 `connect-one` harness with a real fleet runner.

**Deliverables**:
1. Roster config (TOML): server addr + N bots (name/skin/skill) + tuning.
2. Supervisor: per-bot tasks, staggered connect, restart-on-disconnect with backoff, graceful shutdown.
3. Observability: per-bot structured logs + a periodic fleet status summary; optional qctrl integration.
4. Rate-safe pacing: distinct qports + connect/heartbeat cadence that respects `maxclients` + `rate`.
5. CLI: `qbots run --config …`, `qbots connect-one …`, signal-driven shutdown.
6. Verification: 8–16 bots stable for several minutes, frags accumulating, no server kicks.

**Estimated effort**: Medium (1 day)

---

## Context

### Why this last

Everything below it (connection, frames, world, brain) must work for one bot first —
fanning out to N is "do that, safely, many times over." The shared read-only `Arc<World>`
(plan 05) keeps N bots cheap; each bot is otherwise fully independent (AGENTS.md §Concurrency).

### Key Facts

- **One tokio task per bot**, each its own `Connection` + brain + `Usercmd` loop.
- **Shared read-only state**: the parsed `Arc<World>` (nav graph + collision) from Plan 05 —
  loaded once per map. Mutable state is local per bot; bots can't see each other except via
  server frames (just like real clients).
- **Distinct qports are mandatory** when multiple bots share one source IP (Plan 03 T1).
- **Server limits**: `maxclients` caps the fleet; `rate` caps per-client packet rate. Stagger
  connects so we don't burst the server's connectionless handler.
- **qctrl overlap**: qctrl manages the *same* server via RCON. qbots can read qctrl's server
  addr/config to avoid duplication, and qctrl's `status` is the verification lens.

---

## Step-by-Step Tasks

> **RULES.md Rule A/B**: zero warnings, clippy clean, fmt applied, tests green — **commit**
> `task(TN): <desc>` at each boundary.

### T1: Roster config

**Files**: `crates/qbots/src/config.rs`, `crates/qbots/bots.example.toml`

**What to do**: A `serde` config: server address, map search dirs (for the `world` crate),
global tuning (heartbeat cadence, connect stagger), and a roster of bots each with
`name`, `skin`, `skill`, and brain prefs (mirrors Plan 06 T6). Ship an example TOML.

**Commit**: `task(T1): add roster config schema and example`

### T2: Supervisor — spawn, stagger, restart, shutdown

**Files**: `crates/qbots/src/supervisor.rs`

**What to do**: Load config + the shared `Arc<World>`. For each bot, spawn a tokio task
that runs connection → frames → brain (Plans 03–06). **Stagger** connect starts (e.g. 100–300 ms
apart) to avoid a connectionless burst. On disconnect, restart with exponential backoff (cap
attempts). On Ctrl-C/SIGTERM, send `disconnect` stringcmds, then exit. Track per-bot health
for the status summary.

**Commit**: `task(T2): supervisor with stagger, backoff, graceful shutdown`

### T3: Observability + qctrl integration

**Files**: `crates/qbots/src/logging.rs`

**What to do**: Structured per-bot logging via `tracing` (with bot name as a field). A
periodic fleet status line (connected/fragging counts). Optional: read qctrl's config for
the server addr; expose a small status the qctrl dashboard could poll (future). Log all
`svc_print`/`svc_disconnect` reasons for debuggability.

**Commit**: `task(T3): structured per-bot logging and status summary`

### T4: Rate-safe pacing

**Files**: `crates/qbots/src/pacing.rs`

**What to do**: Enforce distinct qports per bot (Plan 03) and cap connect rate globally; cap
per-bot `clc_move` cadence to the server's `rate`/sv_fps. A simple token bucket on connects.
Defend against exceeding `maxclients` (don't spawn more than slots − human headroom).

**Commit**: `task(T4): add connection and heartbeat rate pacing`

### T5: CLI surface

**Files**: `crates/qbots/src/main.rs`

**What to do**: `clap` subcommands: `run --config <toml>` (fleet), `connect-one --addr …
--name …` (from Plan 03, kept for debugging), and `status` (print live fleet state). Wire
signal handling (Ctrl-C/SIGTERM) to the supervisor.

**Commit**: `task(T5): add run / connect-one / status CLI`

### T6: Verify — a stable fleet

**What to do**: Launch 8–16 bots on a test server for several minutes. Assert: all connect
(staggered, no connectionless flood), all stay up (no timeout kicks), frags accumulate
across the fleet, `maxclients` respected. Capture per-bot logs; tune stagger/backoff.
Record fleet-tuning notes in `context/distilled.md`.

**Commit**: `task(T6): verify stable multi-bot fleet`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/qbots/src/config.rs` | roster schema | P0 |
| `crates/qbots/src/supervisor.rs` | lifecycle | P0 |
| `crates/qbots/src/logging.rs` | observability | P1 |
| `crates/qbots/src/pacing.rs` | rate safety | P0 |
| `crates/qbots/src/main.rs` | CLI | P0 |
| `crates/qbots/bots.example.toml` | example roster | P1 |

---

## Open Questions / Risks

1. **CPU at scale.** N bots × (frame decode + brain + trace) can saturate cores.
   *Mitigation*: shared `Arc<World>` (read-only), keep traces tight; profile at 8/16/32 bots.
2. **Server anti-flood / `maxclients`.** Aggressive connects get the source IP temp-banned.
   *Mitigation*: T4 staggering + backoff; leave human headroom in slot count.
3. **qctrl coupling depth.** How much should qbots depend on qctrl? *Mitigation*: keep it
   optional (read addr if pointed at qctrl's config); qbots must run standalone.
4. **Restart storms.** A bad server/map could make all bots reconnect-loop. *Mitigation*:
   per-bot + global backoff caps; circuit-break if connect-fail rate exceeds a threshold.

---

## Verification Checklist

- [ ] T1: example TOML parses; roster renders N distinct bots.
- [ ] T2: staggered connect; a killed/disconnected bot restarts with backoff; Ctrl-C shuts down cleanly.
- [ ] T3: per-bot logs are distinct and filterable; status summary is accurate.
- [ ] T4: distinct qports; no `maxclients` overflow; no connectionless flood detected.
- [ ] T5: `run`/`connect-one`/`status` all work from the CLI.
- [ ] T6: 8–16 bots run several minutes — all up, frags accumulating, no kicks.

---

> **⚠️ CRITICAL REMINDERS ⚠️**
> 
> - **COMMIT AT EVERY TASK COMPLETION** — Format: `task(TN): <description>`. DO NOT WAIT!
> - **FIX ALL WARNINGS BEFORE EACH COMMIT** — `cargo clippy -- -D warnings` must pass.
> - **RUN ALL TESTS BEFORE EACH COMMIT** — `cargo test` must pass.
> - **MOVE COMPLETED PLANS TO `completed/` IMMEDIATELY** — When 100% done, `git mv` to `completed/`.
> - **NEVER batch multiple tasks into one commit** — One task per commit, always.
> - **Reread RULES.md AFTER EACH TASK** — Re-read RULES.md at the end of every task to stay on track.
