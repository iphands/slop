# Help Enumerates Brains/Navmodes — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-07-11
- Scope: 1 source file. Type competition's plural args so clap lists possible values in help.

## Resume Instructions
T1 = convert args to value_enum Vecs + simplify handler; T2 = verify + close. Each task:
implement → `cargo fmt` → `cargo clippy -- -D warnings` → `cargo test` → commit `task(TN): …`.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: typed args + handler | `crates/qbots/src/main.rs` | pending | value_enum + value_delimiter ',' |
| 2 | T2: verify + close | `context/` | pending | help lists values; comma parse works; move to completed/ |
