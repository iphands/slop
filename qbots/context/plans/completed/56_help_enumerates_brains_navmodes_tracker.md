# Help Enumerates Brains/Navmodes — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-07-11
- Completed: 2026-07-11
- Scope: 1 source file. Type competition's plural args so clap lists possible values in help.

## Resume Instructions
Done. `competition --help` now renders `Possible values:` for `--navmodes`/`--brains`/
`--chars` (matching `run`); clap validates tokens; manual comma parsing deleted; defaults +
runtester rejection preserved.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: typed args + handler | `crates/qbots/src/main.rs` | done | value_enum + value_delimiter ','; ~55 lines of parse loops removed |
| 2 | T2: verify + close | `context/` | done | help lists all values; comma parse + clap errors + runtester rejection verified |
