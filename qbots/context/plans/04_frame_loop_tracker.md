# Frame Loop & Movement — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-14
- Plan: `04_frame_loop.md`
- Depends on: Plan 03 (live connection + Plan 02 codec)
- Exit criterion: bot stands on a real map, decodes frames, and walks ≥ 2 min without dropping.

## Resume Instructions
1. Confirm Plan 03 done: bot connects + holds connection.
2. Grab the gold packet/frame capture saved in Plan 03 T8 — it's the test fixture for T1/T3.
3. Decode is line-for-line from `cl_parse.c:739/547/363`; do not improvise the delta loop.
4. Movement first (T4), prediction second (T5) — T5 can ship after Plan 05's real tracer.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: player/entity state decoders | `q2proto/src/{playerstate,entitystate}.rs` | pending | `shared.h:1233/1280`, `common.h:243` |
| 2 | T2: svc_frame + ring | `client/src/frame.rs` | pending | `cl_parse.c:739`; UPDATE_BACKUP=16 |
| 3 | T3: snapshot decode | `client/src/snapshot.rs` | pending | entity types via CS table |
| 4 | T4: movement controller | `client/src/movement.rs` | pending | desired vel → Usercmd |
| 5 | T5: pmove prediction | `client/src/predict.rs` | pending | port `pmove.c`; stub collision |
| 6 | T6: verify walking | — | pending | real map, ≥ 2 min, no drop |
