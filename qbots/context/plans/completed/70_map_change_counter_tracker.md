# Aggregated map-change count — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-07-16 · Closed: 2026-07-16
- Scope: fleet-wide map-change count + sequence in the competition FINAL report

## Resume Instructions
Done. Live-verified 2026-07-16 on noir.lan (2 bots, two rcon `map q2dm1` restarts during the
run): FINAL emitted `run went through 2 map change(s) map_changes=2 levels=3 maps=q2dm1 → q2dm1
→ q2dm1` — deduped from 4 per-bot detections (2 bots × 2 changes) to 2 via the servercount key.
NOTE for future live checks: `cargo build` before running `./target/debug/qbots` — the first
attempt ran a stale P69 binary and printed nothing; a rebuild fixed it.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: FleetStats map log + queries | `crates/qbots/src/stats.rs` | done | `75aba0b0c` (with T2 — inseparable: recorders are dead code without the call site) |
| 2 | T2: record at map load + report line | `crates/qbots/src/{main,supervisor}.rs` | done | `75aba0b0c` |

## Verification
- [x] T1: dedup (36 bots→1), count (3 levels→2), sequence chronological, revisit-counts, empty→0 — unit tests
- [x] T2: live rotation → correct deduped count + sequence in the FINAL report
- [x] fmt + clippy `-D warnings` + full workspace tests green
