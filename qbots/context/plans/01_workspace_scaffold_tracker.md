# Workspace Scaffold — Tracker

## Overview
- Status: 0% complete
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
| 1 | T1: workspace manifest | `Cargo.toml` | pending | resolver="2", 6 members |
| 2 | T2: six stub crates | `crates/*/` | pending | 4 lib + 2 bin |
| 3 | T3: `.gitignore` | `.gitignore` | pending | target*/, vendor/ |
| 4 | T4: `justfile` gates | `justfile` | pending | fmt/clippy/test/build/all |
| 5 | T5: toolchain pin | `rust-toolchain.toml` | pending | stable + rustfmt + clippy |
| 6 | T6: verify green | — | pending | clean clone → `just all` |
