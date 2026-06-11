# qctrl — Quake 2 Server Controller

A Rust REST API + React TypeScript frontend for managing a Quake 2 deathmatch server via RCON.

## Project Goal

Provide a mobile-responsive web interface to control a running Quake 2 server (q2pro) hosted in a Podman container.

### Core Features
- **RCON Command Execution**: Send commands (`dmflags`, `map`, `kick`, `ban`, etc.) to the server.
- **Server Configuration**: Read/write `server.cfg` and map settings.
- **Map Management**: List available `.bsp` maps from `baseq2/maps/` and select via UI (no typing).
- **Real-time Logs**: Stream server console output to the frontend (WebSocket/SSE).
- **Config-Driven**: API reads a local YAML config pointing to `server.cfg` path and `baseq2` directory.

---

## Architecture

### Backend (Rust)
- **Framework**: `axum` or `actix-web` (REST + WebSockets).
- **RCON Client**: Custom implementation based on Quake 2 RCON protocol (UDP/TCP).
- **Config**: `serde_yaml` for loading server paths.
- **Testing**: `cargo test` (Unit + Integration).
- **Docs**: `rustdoc` (Public API documentation).

### Frontend (TypeScript + React)
- **Stack**: Vite + React + TypeScript.
- **UI Library**: TailwindCSS + shadcn/ui (or similar) for mobile-first components.
- **State**: React Query / Zustand.
- **Docs**: Inline JSDoc / TSDoc.

### Directory Structure
```text
qctrl/
├── AGENTS.md              # This file
├── context/               # Knowledge base
│   ├── plans/             # Active plans (NN_name.md)
│   ├── distilled.md       # Summarized learnings (RCON, Protocol, Patterns)
│   ├── pitfalls.md        # Known issues & corrections
│   └── high_level.md      # High-level architecture notes
├── vendor/                # External source/docs
│   ├── quakeiicom.html    # RCON command reference (MANDATORY READ)
│   └── q2pro/             # q2pro source code (MANDATORY READ for protocol)
├── crates/                # Rust workspace
│   ├── api/               # Main REST/WS server
│   ├── rcon/              # RCON client logic
│   └── tools/             # CLI utilities (NO tmp scripts)
└── frontend/              # React TS app
```

---

## Development Workflow

### 1. Planning (MANDATORY)
Before writing code for any non-trivial feature:
1. **Create a Plan**: `context/plans/NN_name.md`
   - Follow `context/plans/RULES.md`.
   - Use `context/plans/NN_example.md` as template.
   - Include `TL;DR`, `Context`, `Tasks`, `Files`, `Verification`.
2. **Update Tracker**: `context/plans/NN_name_tracker.md`.
3. **Execute**: Follow the plan step-by-step.

### 2. Knowledge Management
- **Distilled Learning**: After reading `vendor/` or solving hard problems, summarize findings in `context/distilled.md`.
  - *Example*: RCON packet structure, `dmflags` bitmasks.
- **Pitfalls**: Document bugs, mistakes, or corrections in `context/pitfalls.md`.
  - *Template*: `# Pitfall Name → Problem → Fix → Source`.
- **Re-use**: Always read `distilled.md` and `pitfalls.md` before starting new tasks.

### 3. Code Quality
- **Tests**: Write tests FIRST (Red → Green → Refactor).
  - Rust: `cargo test --all-features`.
  - TS: `npm test`.
- **Linting**: `cargo clippy`, `cargo fmt`, `eslint`, `prettier`.
- **Commits**:
  - Pass all tests/lints before committing.
  - Message format: `task(TN): <description>` (e.g., `task(T1): add rcon client`).
  - Commit small, frequent changes.

### 4. Tooling & Scripts
- **NO `tmp/` Scripts**: All helper tools must live in `crates/tools/`.
  - Create a binary: `cargo run --bin tools -- <command>`.
  - Keep tools reusable and documented.

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
**READ**: `./vendor/q2pro/src/`
- **Files to Study**:
  - `cl_console.c`, `sv_main.c`: RCON handling logic.
  - `common.h`, `q_shared.h`: Protocol definitions.
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

---

## Getting Started

1. **Read Rules**: `context/plans/RULES.md`.
2. **Read Vendor**: `vendor/quakeiicom.html` (RCON section).
3. **Create Plan**: `context/plans/01_setup.md` (Project scaffolding).
4. **Initialize**: `cargo init`, `npm init`, etc.

---

## Status

- **Phase**: Planning / Scaffolding
- **Next Step**: Create Plan T1 (Project Setup & Config Loading).
