# q2dm3 Lava Deaths + zb2 Wall-Running Fixes — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-07-10
- Bugs: L1 (lava-blind floor probe), L2 (unguarded combat strafing), Z1 (LOS-only shortcut), Z2 (no-route freeze/blind-run), Z3 (identical-route replan loop)

## Resume Instructions
Read Plan 48 `Pre-Identified Bugs` first — every bug is verified with file:line. Tasks are
independent except T3 depends on T1 (`segment_has_floor` signature/semantics). Follow RULES.md
Rule A/B: zero warnings, commit per task (`task(P48-TN): …`).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: lava-aware segment_has_floor + node sampling, cache v21 | `world/navgraph.rs`, `world/mapcache.rs` | done | `tests/lava_q2dm3.rs` self-locates lava; verified red pre-fix / green post-fix via stash |
| 2 | T2: ground-hazard probe gates combat/dodge/stuck strafing | `brain/hazard.rs`, `brains/main.rs`, `brains/zb2.rs` | pending | |
| 3 | T3: zb2 shortcut skip walkability validation | `brains/zb2.rs` | pending | |
| 4 | T4: zb2 no-route engage + hard-stuck goal rotation | `brains/zb2.rs` | pending | |
| 5 | T5: pitfalls + brain notes; close plan | `context/pitfalls.md`, `context/brain_notes.md` | pending | |
