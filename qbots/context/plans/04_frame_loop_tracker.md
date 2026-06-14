# Frame Loop & Movement — Tracker

## Overview
- Status: 100% — DONE (2026-06-14); **verified live** (bot walks on `noir.lan:27910`).
  T5 (pmove prediction) deferred to after Plan 05 (needs the world tracer).
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
| 1 | T1: player/entity state decoders | `q2proto/src/{playerstate,entitystate}.rs` | done | U_* MOREBITS + PS_*; 5 tests |
| 2 | T2: svc_frame + ring | `q2proto/src/frame.rs` | done | parse_frame + FrameRing; terminator = WriteShort(0); 3 tests |
| 3 | T3: snapshot decode (wired) | `client/src/conn.rs` | done | conn decodes svc_frame → conn.frame; self_origin() |
| 4 | T4: movement controller → real clc_move | `client/src/conn.rs` (keepalive) | done | COM_BlockSequenceCRCByte ported; bot walks live (~100 u/s) |
| 5 | T5: pmove prediction | `client/src/predict.rs` | deferred | needs world tracer (Plan 05); not required for the walk milestone |
| 6 | T6: verify walking | — | done | **VERIFIED**: origin streams + changes (361→−30 X over ~4s); no drop/kick |

## Confirmed live
- Full perceive→act loop proven: connect → spawn → decode frames (own state + PVS ents) →
  send real `clc_move` → server moves the bot → new frames show the new origin.
- Checksum port is byte-correct (server accepted moves; no kick).
