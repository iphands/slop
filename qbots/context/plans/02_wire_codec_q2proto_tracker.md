# Wire Codec (q2proto) — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-14
- Plan: `02_wire_codec_q2proto.md`
- Depends on: Plan 01 (workspace must exist + compile)
- Exit criterion: `cargo test -p q2proto` green; `just all` green; all primitives + usercmd delta + InfoString + OOB round-trip.

## Resume Instructions
1. Re-read the **Key Facts** in the plan (source line numbers cited).
2. `cargo test -p q2proto` — see which round-trips fail; those are the active task.
3. When touching a codec primitive, diff against `vendor/yquake2/src/common/movemsg.c` line-for-line.
4. Keep `q2proto` **pure** (no `tokio`, no sockets) — transport belongs to Plan 03.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Reader/Writer primitives | `q2proto/src/{reader,writer}.rs` | pending | port `movemsg.c`; `bytes` dep |
| 2 | T2: opcodes + flags | `q2proto/src/ops.rs` | pending | `common.h:199–300` |
| 3 | T3: usercmd delta | `q2proto/src/usercmd.rs` | pending | `movemsg.c:644/1181` |
| 4 | T4: InfoString | `q2proto/src/infostring.rs` | pending | `shared/shared.c` Info_* |
| 5 | T5: OOB framing | `q2proto/src/oob.rs` | pending | `0xff×4` + tokenizer |
| 6 | T6: round-trip tests | `q2proto/tests/` | pending | + gold vector (deferred) |
