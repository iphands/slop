# q2dm3 Nav Fragmentation Fix — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-18
- Goal metric: q2dm3 = 7/7 spawns in largest component; `just mapcache 'q2dm*'` → err=0.

## Resume Instructions
Phase order: tooling (T1, T2) → diagnose (T3) → fix (T4, then T5 only if needed) →
batch UX (T6) → regen+verify (T7). T6 is independent and can land anytime. Bump
`mapcache::VERSION` whenever a task changes generated nav geometry.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: navinspect live-build fallback | `tools/bin/navinspect.rs` | pending | |
| 2 | T2: compgaps flat-gap walkability | `tools/bin/compgaps.rs` | pending | |
| 3 | T3: diagnose q2dm3 connectors | `context/map_errors.notes.log.md` | pending | |
| 4 | T4: anchor lift top nodes | `world/build.rs`, `world/navgraph.rs`, `world/mapcache.rs` | pending | VERSION bump |
| 5 | T5: spawn-aware bridge (if needed) | `world/navgraph.rs`, `world/build.rs` | pending | only if T4 insufficient |
| 6 | T6: resilient cache batch | `qbots/main.rs` | pending | |
| 7 | T7: regenerate + verify | — | pending | live false-bridge guard |

## Baseline (2026-06-18, pre-fix)
- q2dm3 @ spacing 24: 27 components pre-bridge, **3/7 spawns** in largest → gate FAIL.
- gridscan spacings 24/16/12: comps 27/43/45 (finer = worse → not a resolution issue).
- Cause: lift floor-probe blindness + `BRIDGE_HDIST` cut 512→256.
