# Workspace Scaffold — Tracker

## Overview
- Status: 100% complete — DONE (2026-06-14)
- Start date: 2026-06-14
- Plan: `01_workspace_scaffold.md`
- Exit criterion: `just all` green on a clean clone; `git status` shows no `target/`/`vendor/`.

## Resume Instructions
1. `just all` — if it fails, the workspace isn't green; fix before any new task.
2. `git status --porcelain | grep -E 'target|vendor'` — must be empty (artifacts gitignored).
3. Pick the lowest-numbered `pending` task below; mark it `in-progress`; follow the plan's task section verbatim.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: workspace manifest | `Cargo.toml` | done | resolver="2", 6 members |
| 2 | T2: six stub crates | `crates/*/` | done | 4 lib + 2 bin |
| 3 | T3: `.gitignore` | `.gitignore` | done | target*/, vendor/, rs.bk |
| 4 | T4: `justfile` gates | `justfile` | done | fmt/clippy/test/build/all |
| 5 | T5: toolchain pin | `rust-toolchain.toml` | done | stable + rustfmt + clippy |
| 6 | T6: verify green | — | done | `just all` green; target/ ignored; Cargo.lock committed |
