# Plan 02 — Wire Codec (`q2proto`)

> **Status**: pending
> **Created**: 2026-06-14
> **Depends on**: Plan 01
> **Goal**: A pure, transport-agnostic, heavily-tested Rust port of the Quake 2 message
> codec — readers/writers, opcode tables, `usercmd_t` delta encode/decode, InfoString, and
> connectionless (OOB) framing — so Plan 03 can speak the wire format byte-for-byte.
> **Agent**: implementation agent (ralph-loop) | sub-agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Port the Q2 binary message API from `vendor/yquake2/src/common/movemsg.c` into
`crates/q2proto` as pure functions over `bytes::BytesMut`. No sockets, no async — just
correct byte shuffling, validated by round-trip property tests.

**Deliverables**:
1. `Reader`/`Writer` primitives (little-endian byte/short/long/float/string/coord/angle/pos/dir).
2. Opcode + flag tables (`svc_*`, `clc_*`, `CM_*`, `PS_*`, entity bits, `SND_*`).
3. `Usercmd` + delta encode/decode (`MSG_WriteDeltaUsercmd` / `ReadDeltaUsercmd`).
4. `InfoString` (`key\value\…`) get/set/remove.
5. Connectionless OOB frame (`0xff×4` prefix) + command tokenizer.
6. Unit + round-trip tests proving byte-for-byte parity with the C reference.

**Estimated effort**: Medium (1 day)

---

## Context

### Why pure + transport-agnostic

The codec must be exactly right — a single byte wrong and the server drops us (AGENTS.md
§Constraints #2). Keeping it pure (no `tokio`, no sockets) lets us unit-test it
deterministically against hand-built byte vectors and captured packets, and lets Plan 03
layer async transport on top without entangling concerns.

### Key Facts (confirmed against `vendor/yquake2/src/`)

- **Reference source**: `common/movemsg.c` — contains **all** `MSG_Read*`/`MSG_Write*`
  primitives **and** `MSG_WriteDeltaUsercmd` (line **644**) / `MSG_ReadDeltaUsercmd`
  (line **1181**). (Earlier guess of `shared.c` was wrong — it's `movemsg.c`.)
- **InfoString**: `Info_SetValueForKey` etc. live in `common/shared/shared.c`
  (declared in `common/header/shared.h`).
- **Opcodes/flags**: `common/header/common.h:199` (`enum svc_ops_e`), `:231`
  (`enum clc_ops_e`), `:243` (`PS_*`), `:265` (`CM_*` usercmd delta bits), `:277`
  (`SND_*`), `:288` (entity bits).
- **`PROTOCOL_VERSION == 34`** (`common/header/common.h:185`).
- **Connectionless prefix**: 4 × `0xff` (`server/sv_conless.c:385` "four leading 0xff").
- **`usercmd_t`**: `common/header/shared.h:676` — `msec`, `buttons`, `angles[3]`,
  `forwardmove`, `sidemove`, `upmove`, `impulse`, `lightlevel` (all little-endian;
  `angles`/moves are `i16`).
- **Delta rule**: `CM_ANGLE1..3 | CM_FORWARD | CM_SIDE | CM_UP | CM_BUTTONS | CM_IMPULSE`
  bitmask selects which fields differ from the previous cmd; `msec` + `lightlevel` are
  **always** sent. `bytedirs[]` (NUMVERTEXNORMALS) compresses aim directions — port the
  table verbatim from `movemsg.c`.

### Why not use an existing crate

There is no maintained, protocol-34-accurate Rust Q2 client codec we'd trust against a
real server. Hand-rolling from the authoritative C is the safe path and the codec is small.

---

## Step-by-Step Tasks

> **RULES.md Rule A/B**: zero warnings, clippy clean, fmt applied, tests green — **commit**
> `task(TN): <desc>` at each boundary.

### T1: Reader/Writer primitives

**Files**: `crates/q2proto/src/reader.rs`, `crates/q2proto/src/writer.rs`, `crates/q2proto/Cargo.toml`

**What to do**: Add `bytes = "1"` as a dep. Implement `MsgReader` and `MsgWriter` over
`BytesMut` porting the primitives in `movemsg.c` **1:1**: `read_byte`/`write_byte`,
`read_char`/`write_char` (i8), `read_short`/`write_short` (i16 LE), `read_long`/`write_long`
(i32 LE), `read_float`/`write_float` (f32 LE), `read_string`/`write_string` (NUL-terminated),
`read_coord`/`write_coord`, `read_angle`/`write_angle`, `read_pos`/`write_pos` (3 coords),
`read_dir`/`write_dir` (via `bytedirs`). Track read/write position; reader returns errors
on overrun (no panics — see AGENTS.md §Code Quality: no `unwrap` without handling).

**Commit**: `task(T1): port MSG_Read*/Write* primitives to q2proto`

### T2: Opcode + flag tables

**Files**: `crates/q2proto/src/ops.rs`

**What to do**: Port the enums and bitmasks from `common/header/common.h:199–300`:
`SvcOp` (`svc_*`), `ClcOp` (`clc_*`), and the `PS_*`, `CM_*`, `SND_*`, entity-bits
constants as `pub const`. Derive `TryFrom<u8>` for the op enums; unknown op → `Err`.

**Commit**: `task(T2): port svc/clc opcodes and CM_/PS_/SND_ flags`

### T3: `Usercmd` + delta encode/decode

**Files**: `crates/q2proto/src/usercmd.rs`

**What to do**: Port `usercmd_t` as `pub struct Usercmd` (`common/header/shared.h:676`).
Port `MSG_WriteDeltaUsercmd` (`movemsg.c:644`) → `write_delta(&mut Writer, from, cmd)` and
`MSG_ReadDeltaUsercmd` (`movemsg.c:1181`) → `read_delta(&mut Reader, from) -> Usercmd`.
Emit the `CM_*` bitmask exactly as C does (only set a bit when the field changed; always
write `msec` + `lightlevel`).

**Commit**: `task(T3): port usercmd_t delta encode/decode`

### T4: InfoString

**Files**: `crates/q2proto/src/infostring.rs`

**What to do**: Port `Info_*` from `common/shared/shared.c`: an `InfoString` over the
`key\value\key\value` format with `get`, `set`, `remove`, and `MAX_INFO_STRING`-aware
length checks. Used to build `userinfo` (name/skin/rate/msg/hand/fov) in Plan 03.

**Commit**: `task(T4): port InfoString key\\value codec`

### T5: Connectionless (OOB) framing

**Files**: `crates/q2proto/src/oob.rs`

**What to do**: Helpers for connectionless packets: `write_oob` prepends `0xff 0xff 0xff 0xff`
then an ASCII command line (`getchallenge\n`, `connect …\n`); `read_oob` detects the prefix
and returns the command + whitespace-split args (matching how `sv_conless.c` tokenizes via
`Cmd_Argv`). Also define the OOB verbs we send/receive: `getchallenge`, `challenge`,
`connect`, `client_connect`, `print`, `info`.

**Commit**: `task(T5): add connectionless 0xff-prefix framing + tokenizer`

### T6: Tests — round-trips and parity

**Files**: `crates/q2proto/tests/`, `crates/q2proto/src/lib.rs` (`#![...]`, re-exports)

**What to do**:
- Reader/writer round-trip for every primitive (incl. overflow → `Err`).
- `Usercmd` delta: random `from`/`cmd`, `write_delta` then `read_delta`, assert equal;
  assert unchanged fields cost zero bytes (bit not set).
- InfoString set/get/remove round-trip with embedded backslash edge cases.
- OOB: a `connect 34 <qport> <challenge> "<userinfo>"\n` round-trips through tokenize.
- **Optional gold test**: if a real captured packet is obtained (Plan 03 T8), freeze a
  byte vector and assert our encoder reproduces it exactly. Mark `#[ignore]` until then.

**Commit**: `task(T6): add codec round-trip and parity tests`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/q2proto/src/{reader,writer}.rs` | MSG_* primitives | P0 |
| `crates/q2proto/src/ops.rs` | svc/clc + flag tables | P0 |
| `crates/q2proto/src/usercmd.rs` | usercmd delta | P0 |
| `crates/q2proto/src/infostring.rs` | InfoString | P0 |
| `crates/q2proto/src/oob.rs` | OOB framing | P0 |
| `crates/q2proto/tests/*.rs` | round-trip + parity | P0 |

---

## Open Questions / Risks

1. **`coord`/`angle` exact encoding.** Q2 writes coords as a float and angles as a byte
   (`*256/360`). *Mitigation*: port verbatim from `movemsg.c`; add a focused round-trip
   test rather than guessing. (Not used until Plan 04 frames, but define correctly now.)
2. **`bytedirs[]` accuracy.** A wrong entry in the 162-vector table silently corrupts aim.
   *Mitigation*: copy the table verbatim, not by retyping; add a test that all entries are
   unit length.
3. **No captured packet yet.** Gold parity tests need a real exchange (Plan 03 T8).
   *Mitigation*: structure tests so the gold vector is added later without rewriting them.

---

## Verification Checklist

- [ ] T1: `MsgReader`/`MsgWriter` round-trip every primitive; overrun returns `Err` not panic.
- [ ] T2: `SvcOp`/`ClcOp` `TryFrom<u8>` covers `common.h:199/231`; unknown → `Err`.
- [ ] T3: `write_delta`/`read_delta` round-trip; byte length matches C for given field changes.
- [ ] T4: InfoString round-trip incl. backslash edge cases + length cap.
- [ ] T5: OOB `0xff×4` framing round-trips; `connect …` tokenizes into 4 argv as the server expects.
- [ ] T6: `cargo test -p q2proto` green; `just all` green.
