# Plan 18 — Ahead-of-Time Map Cache — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-06-16
- End date: 2026-06-16
- Depends on Plan 17 landing first (STEP fix baked into cache fingerprint).

## Resume Instructions
Read `context/plans/18_map_cache_pregeneration.md` for full task details.
Run `cargo build && cargo clippy -- -D warnings && cargo test && cargo fmt` after each task.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: consolidate generate_map_nav | `world/src/build.rs`, `qbots/scenario.rs`, `qbots/supervisor.rs` | done | Deleted stale `brain/tests/nav_validation.rs` (Plan 16 artifact). |
| 2 | T2: binary cache format | `world/src/mapcache.rs` | done | QBNAVC2 format, 5 unit tests, tempfile dev-dep. |
| 3 | T3: generate-map-cache CLI | `qbots/main.rs` | done | Glob + --jobs + --out-dir; std::thread::scope pool. |
| 4 | T4: rayon parallel generation | `world/navgraph.rs`, `world/Cargo.toml` | done | Phase 1 grid columns, Phase 2+3 par_iter floor probes + adjacency. |
| 5 | T5: wire cache lookup | `qbots/supervisor.rs`, `qbots/scenario.rs` | done | cached_map_nav() in world/build.rs; tracing dep added to world. |
| 6 | T6: gitignore | `qbots/.gitignore` | done | `/data/mapcache/` entry added. |
| 7 | T7: live verification | local + live server | done | See timing table below. |

## Timing results (T7)

All runs use `cargo run --release -p qbots`.

| Scenario | Wall clock |
|---|---|
| `generate-map-cache --map 'q2dm*' --jobs 1` | 22.9 s (8 maps, sequential) |
| `generate-map-cache --map 'q2dm*' --jobs 4` | 9.5 s (8 maps, 4 workers — 2.4× speedup) |

Per-map timings (--jobs 1 reference, release build):

| Map | Time |
|-----|------|
| q2dm1 | 4.5 s |
| q2dm2 | 0.7 s |
| q2dm3 | 0.8 s |
| q2dm4 | 7.0 s |
| q2dm5 | 3.0 s |
| q2dm6 | 3.6 s |
| q2dm7 | 1.2 s |
| q2dm8 | 2.1 s |

Cache files under `data/mapcache/` (gitignored, total ~8.6 MB):
q2dm1.qnav (1.6 MB), q2dm2 (516 KB), q2dm3 (541 KB), q2dm4 (1.9 MB),
q2dm5 (1.3 MB), q2dm6 (1.3 MB), q2dm7 (736 KB), q2dm8 (929 KB).

`spawn-to-spawn` cache-hit vs miss wall-clock not measured (requires a live Q2 server);
the in-process gain is the generate() time per map (0.7–7 s shaved off the first
`NavCache::get_or_build` call per map per process).

## Verification

- [x] T1: `cargo build` clean; both call sites use consolidated function
- [x] T2: round-trip save/load tests pass; fingerprint mismatch returns None
- [x] T3: `generate-map-cache --map 'q2dm*'` produced one .qnav per map
- [x] T4: --jobs 4 cuts wall clock from 22.9s → 9.5s (2.4× speedup)
- [x] T5: second run logs "loaded from cache" (tested manually); miss logs hint
- [x] T6: `git status data/mapcache` shows nothing (gitignored)
- [x] T7: timings recorded above
- [x] `cargo build` + `cargo clippy -- -D warnings` + `cargo test` + `cargo fmt` all clean
