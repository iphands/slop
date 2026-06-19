# q2dm3 Nav Fragmentation Fix — Tracker

## Overview
- Status: DONE (rescoped) — diagnostics + resilient batch delivered; deep nav fix → Plan 35.
- Start date: 2026-06-18
- Delivered: navinspect live-build, compgaps flat-gap fix, q2dm3 diagnosis, resilient
  `generate-map-cache --allow-failures` (good maps cache, failures named).

## T3 outcome — surgical assumption insufficient → deferred to Plan 35
Diagnosis showed the fragmentation is **multi-causal AND broad**: restoring BRIDGE_HDIST=512
still gives 3/7, and the batch revealed **5/8 stock maps fail** (q2dm2/3/5/6/7), not just
q2dm3 — a code regression (`walkable_stair` floor-check `662580e69` + BRIDGE_HDIST cut). Per
user decision, the nav-graph fix is deferred to **Plan 35**; this plan shipped the unblock.

## Resume Instructions
Phase order: tooling (T1, T2) → diagnose (T3) → fix (T4, then T5 only if needed) →
batch UX (T6) → regen+verify (T7). T6 is independent and can land anytime. Bump
`mapcache::VERSION` whenever a task changes generated nav geometry.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: navinspect live-build fallback | `tools/bin/navinspect.rs` | done | `QBOTS_LIVE=1` |
| 2 | T2: compgaps flat-gap walkability | `tools/bin/compgaps.rs` | done | 1043→269 real |
| 3 | T3: diagnose q2dm3 connectors | `context/map_errors.notes.log.md` | done | multi-causal + broad |
| 4 | T4: anchor lift top nodes | — | deferred | → Plan 35 |
| 5 | T5: spawn-aware bridge | — | deferred | → Plan 35 |
| 6 | T6: resilient cache batch | `qbots/main.rs`, `justfile` | done | `--allow-failures` |
| 7 | T7: regenerate + verify all q2dm* | — | deferred | → Plan 35 |

## Baseline (2026-06-18, pre-fix)
- q2dm3 @ spacing 24: 27 components pre-bridge, **3/7 spawns** in largest → gate FAIL.
- gridscan spacings 24/16/12: comps 27/43/45 (finer = worse → not a resolution issue).
- Cause: lift floor-probe blindness + `BRIDGE_HDIST` cut 512→256.
