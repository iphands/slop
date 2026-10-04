# qctrl — Quake 2 Server Controller

A Rust REST API + React TypeScript frontend for managing a Quake 2 deathmatch server via RCON.

## Project Goal

Provide a mobile-responsive web interface to control a running Quake 2 server (yquake2) hosted in a Podman container.

### Core Features
- **RCON Command Execution**: Send commands (`dmflags`, `map`, `kick`, `ban`, etc.) to the server.
- **Server Configuration**: Read/write `server.cfg` and map settings.
- **Map Management**: List available `.bsp` maps from `baseq2/maps/` and select via UI (no typing).
- **Real-time Logs**: Stream qctrl's RCON activity (commands sent, replies, refusals) to the frontend
  over WebSocket (`/api/logs/ws`). The Q2 server console itself is not streamed.
- **Config-Driven**: API reads a local YAML config pointing to `server.cfg` path and `baseq2` directory.

---

## Architecture

### Backend (Rust)
- **Framework**: `axum` 0.7 (REST + WebSockets); `tower-http` for CORS, tracing and static files.
- **RCON Client**: Custom implementation based on Quake 2 RCON protocol (UDP/TCP).
- **Config**: `serde_yaml` for loading server paths.
- **Testing**: `cargo test` (Unit + Integration).
- **Docs**: `rustdoc` (Public API documentation).

### Frontend (TypeScript + React)
- **Stack**: Vite + React + TypeScript.
- **UI Library**: TailwindCSS 3 (no component library) for mobile-first components.
- **State**: TanStack React Query v5.
- **Docs**: Inline JSDoc / TSDoc.

### Directory Structure
```text
qctrl/
├── AGENTS.md              # This file
├── CLAUDE.md              # → AGENTS.md (symlink — the name Claude Code loads)
├── context/               # living memory — READ context/AGENTS.md before new work
│   ├── plans/             # plan system — RULES.md is authoritative; read in full
│   │   ├── RULES.md       #   format + per-task rules + project rules
│   │   ├── SERIES.md      #   dependency chain, status, next free plan number
│   │   ├── NN_example*.md #   skeletons to copy for a new plan + tracker
│   │   ├── completed/     #   closed plans (git mv here at 100%)
│   │   └── abandoned/     #   dropped plans, reason recorded in SERIES
│   ├── AGENTS.md          # what lives where in context/ (CLAUDE.md links here)
│   ├── distilled.md       # confirmed facts (provenance-tagged)
│   └── pitfalls.md        # bugs & gotchas, especially multi-attempt fixes
├── vendor/                # External source/docs
│   ├── quakeiicom.html    # RCON command reference (MANDATORY READ)
│   └── yquake2/           # yquake2 source — the live server (MANDATORY READ for protocol)
├── crates/                # Rust workspace
│   ├── api/               # Main REST/WS server
│   ├── rcon/              # RCON client logic
│   └── tools/             # CLI utilities (NO tmp scripts)
└── frontend/              # React TS app
```

---

## Development Workflow

### 1. Planning (MANDATORY)
Non-trivial work gets a plan: see *Plans & Context* below and `context/plans/RULES.md` (its first
table says what needs a plan).

### 2. Knowledge Management
Read `context/AGENTS.md` for what lives where (`distilled.md`, `pitfalls.md`, the slop-wide
`../context/`). Read the relevant files before new tasks; record findings as you go.

### 3. Code Quality
- **Tests**: Write tests FIRST (Red → Green → Refactor).
  - Rust: `cargo test --all-features`.
  - TS: `npm test`.
- **Linting**: `cargo clippy`, `cargo fmt`, `eslint`, `prettier`.
- **Build Verification**: **NEVER commit broken code.** The pre-commit gate is RULES Rule A
  (*Project gate*: backend, frontend, docs-only). Never use `just fe-test` / `npm run testall` as a
  gate — they drive the live server.
  - **If build fails, FIX IT FIRST. Do NOT claim "done" if build is broken.**
- **Commits**: format, cadence and git rules live in RULES Rule B / B2
  (`[qctrl][P<n>][T<n>][<topic>] <summary>`). Commit small, frequent changes.

### 4. Tooling & Scripts
- **NO `tmp/` Scripts**: All helper tools must live in `crates/tools/`.
  - Create a binary: `cargo run --bin tools -- <command>`.
  - Keep tools reusable and documented.

---

<!-- hands-plan:begin v1 -->
## Plans & Context

`context/` is this project's living memory and `context/plans/` its plan system.
**`context/plans/RULES.md` is authoritative** — where this file and RULES.md disagree, RULES.md wins.
This is a sub-project of `slop`: `../CLAUDE.md` (git discipline, shared conventions) also applies.

- **Before non-trivial work:** read `context/plans/RULES.md` in full (its first table says what
  needs a plan), then `context/plans/SERIES.md` (active plans, next free number, north star).
  New plan = copy `NN_example.md` + `NN_example_tracker.md`, register it in SERIES.
- **Working a plan:** follow the tracker's Resume Instructions → pass the project gate
  (Rule A) → commit `[qctrl][P<n>][T<n>][<topic>] <summary>` with the tracker row in the same
  commit. One task per commit.
- **Git:** never push; no co-author trailers; history is append-only — fix a bad commit with a
  new one, and chain `edit && git commit`.
- **Knowledge:** read `context/AGENTS.md` for what lives where; record findings as you go, and
  harvest them before a plan moves to `completed/`.
- **Current state lives in `SERIES.md`**, not in this file.
- **Honesty:** never claim something is done, verified or recorded unless it is — on disk, in the
  command output.
<!-- hands-plan:end -->

---

## Domain Knowledge

### RCON Protocol (Vendor Reference)
**READ**: `./vendor/quakeiicom.html`
- **Command**: `rcon <password> <command>`
- **Variables**: `rcon_password`, `rcon_address`.
- **Key Commands**:
  - `status`: List players (for kick/ban).
  - `kick <player>`: Remove player.
  - `clientkick <num>`: Ban by client number.
  - `dmflags <val>`: Set deathmatch flags.
  - `map <name>`: Change map.
  - `timelimit <mins>` / `fraglimit <score>`.

### Server Source (Vendor Reference)
**READ**: `./vendor/yquake2/src/` — the live server runs yquake2 8.70.
- **Files to Study**:
  - `server/sv_conless.c` (`SVC_RemoteCommand`), `server/sv_main.c`: RCON handling logic.
  - `common/header/common.h`, `common/header/shared.h`: Protocol definitions.
- **Goal**: Understand how the server parses RCON packets and logs output.

### Mobile UI Requirements
- **Map Selection**: Dropdown or grid of buttons (no text input).
- **Log Stream**: Auto-scrolling terminal view (WebSocket preferred).
- **Controls**: Large touch targets (44px+).

---

## Constraints & Rules

1. **No Type Suppression**: Never use `as any`, `@ts-ignore`, or `unwrap()` without handling.
2. **Small Modules**: Keep functions < 50 lines. Single responsibility.
3. **Documentation**:
   - Rust: `///` doc comments on public items.
   - TS: JSDoc on complex components/functions.
4. **Delegation**:
   - If stuck on RCON protocol details, search `vendor/` first.
   - If protocol unclear, consult `librarian` agent with `vendor/` context.
5. **Verification**:
   - Before marking a plan task `done`:
     - Tests pass.
     - Clippy/Lint clean.
     - Feature works locally.
6. **Honesty**:
   - When you say you'll do something, DO IT. Then say "done."
   - **NEVER** say you're "speaking metaphorically" about "recording to memory" when you haven't actually done the work.
   - **NEVER** claim you've "recorded" something in "memory" or "AGENTS.md" unless you've actually written it to a file.
   - Be direct. Be honest. Do the thing. Say "done."

---

## Current State

Current state — active plans and what's next — lives in `context/plans/SERIES.md`, not in this
file. New here? Read `context/plans/RULES.md`, then `vendor/quakeiicom.html` (RCON section).
