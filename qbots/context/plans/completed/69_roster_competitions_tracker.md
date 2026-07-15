# Roster-file competitions — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-07-15 · Closed: 2026-07-15
- Scope: `--roster <file.yaml>` for competition + ranked roster dump at FINAL

## Resume Instructions
Done. Live-verified 2026-07-15 on noir.lan:27910 (q2dm4): a matrix run
(`--count 2 --brains main,q3 --navmodes as`) wrote `./logs/roster/<ts>.yaml` ranked exactly
like its FINAL board; a hand-edited rematch (`champs` custom tag + count 3 + `char: cam`, plus
a bare `mai` group at file-default count) fielded exactly those two groups (total=5), derived
the legend from the specs, and grouped the board by the custom tag. Its own dump preserved the
custom tag + char, round-tripping.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: GroupSpec + matrix_specs + visibility | `crates/qbots/src/supervisor.rs` | done | `0a8e06898` (with T2 — inseparable) |
| 2 | T2: run_competition consumes Vec<GroupSpec> | `crates/qbots/src/{supervisor,main}.rs` | done | `0a8e06898` |
| 3 | T3: roster.rs loader + validation | `crates/qbots/src/roster.rs` | done | `a8758911c` (with T4 — inseparable) |
| 4 | T4: --roster flag + dispatch | `crates/qbots/src/main.rs` | done | `a8758911c` |
| 5 | T5: emit_ranked_yaml + FINAL dump hook | `crates/qbots/src/{roster,supervisor}.rs` | done | `bce95e521` |
| 6 | T6: docs + SERIES + move to completed/ | `context/` | done | this commit |

## Verification
- [x] T1: equivalence (tags/qports) + proportional-clamp-identity tests green
- [x] T2: matrix path behavior-identical; existing scoreboard/name tests stay green
- [x] T3: 17-case validation matrix green (one red case per rule)
- [x] T4: `--roster` conflicts with every matrix switch (`try_parse_from` tests)
- [x] T5: emit → reload round-trip preserves identity in rank order; live dump matches FINAL board
- [x] fmt + clippy `-D warnings` + full workspace tests green at every commit
