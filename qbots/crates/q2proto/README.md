# q2proto — Quake 2 Wire Codec (Protocol 34)

**Byte-for-byte port of Yamagi Quake 2's wire format.** A pure, transport-agnostic
Rust implementation of the client–server protocol.

> **Ground truth:** `vendor/yquake2/src/common/` — every encoder/decoder ported verbatim
> from C. Source lines cited in-code. **This isn't a guess; it's copied from line X.**

---

## Why This Exists

Classic Q2 bots run *inside* the server as `gamex86.dll` plugins with perfect knowledge
of geometry and entities via `gi.trace()`. **qbots is external** — it sees only UDP
packets. `q2proto` is the **byte shuffling layer** that makes an external program speak
Quake 2 correctly.

**No async. No sockets. Just correct byte operations over `bytes::BytesMut`.**

---

## ⚠️ Common Gotchas

Read these before you start — getting them wrong breaks connections:

```
⚠️  Endianness: Everything is LITTLE-ENDIAN. The C source assumes x86; Rust doesn't.
⚠️  Coord vs Float: Positions are i16 * 0.125 (1/8 unit), NOT f32. Use read_coord/write_coord.
⚠️  Angle compression: MSG_ReadAngle reads a SIGNED byte (−128→180°, 0→0°).
⚠️  Delta encoding: Track your last sent Usercmd to delta-encode the next.
⚠️  Delta decoding: Maintain 16-frame history to decode `svc_frame` entities.
⚠️  OOB prefix: 0xFF×4 is also i32 -1. Read as i32 to detect connectionless packets.
⚠️  InfoString limits: Keys/values max 63 chars; total string max 511 bytes (NUL-terminated).
```

---

## What This Is

- **MSG_* primitives** — `Reader` / `Writer` for all `MSG_Read*` / `MSG_Write*` operations
  (little-endian scalars, strings, fixed-point coords, compressed angles/dirs).
- **Delta compression** — `Usercmd` encoding/decoding via `build_clc_move`, ported from
  `movemsg.c` with the 162-entry `bytedirs[]` vertex-normal table.
- **InfoString** — userinfo parsing/building (`key\value\key\value`).
- **Connectionless framing** — OOB packets (`0xff 0xff 0xff 0xff`), challenge/handshake.
- **Server frames** — `svc_*` / `clc_*` opcodes, frame parsing, packet entities.
- **CRC** — block CRC and sequence-bit CRC for netchan reliability.

---

## Quick Start

### Sending: Build a Connect Packet

```rust
use q2proto::{InfoString, write_oob, Writer};

let mut info = InfoString::new();
info.set("name", "mybot");
info.set("rate", "25000");

let mut w = Writer::new();
write_oob(&mut w, &format!(
    "connect 34 28000 12345 \"{}\"\n",
    info.as_str()
));
let connect_packet = w.freeze();

// Send `connect_packet` via UDP to server:27910
// (q2proto doesn't do sockets — you provide the transport)
```

### Receiving: Parse a Server Frame

```rust
use q2proto::{Reader, is_oob, oob_payload};

let server_frame: &[u8] = receive_udp_packet(); // you provide this

if is_oob(server_frame) {
    let payload = oob_payload(server_frame).unwrap();
    // Parse OOB reply: "client_connect", "challenge 123 p=34", etc.
} else {
    // In-band netchan packet: parse svc_* ops, playerstate, entities
}
```

### Building a Movement Command

```rust
use q2proto::{Usercmd, build_clc_move};

let last_cmd = Usercmd {
    msec: 16,
    forwardmove: 200,
    angles: [0, 180 * 65536 / 360, 0], // pitch, yaw, roll as i16
    ..Default::default()
};

// Build a clc_move with 3 usercmds (oldest, mid, newest).
// `build_clc_move` delta-encodes `cmds[2]` (newest) against `last_cmd`
// (the previous frame's newest). Returns the full payload.
let cmds = [last_cmd, last_cmd, last_cmd]; // oldest, mid, newest
let payload = build_clc_move(1000, cmds, 42); // serverframe, sequence

// Send via UDP (with netchan headers — that's client/'s job)
```

See the actual usage in `client/src/conn.rs` for integration with the netchan layer.

See [`src/reader.rs`](src/reader.rs), [`src/writer.rs`](src/writer.rs),
[`src/usercmd.rs`](src/usercmd.rs), and [`src/oob.rs`](src/oob.rs) for the full API.

---

## Core Concepts

### The MSG_* Primitives

The Q2 protocol is a stream of typed fields. `Reader`/`Writer` implement the exact
semantics of the C `MSG_Read*` / `MSG_Write*` functions:

- **Scalars** — `ReadByte`, `ReadShort`, `ReadLong`, `ReadFloat` (little-endian).
- **Strings** — `ReadString` (null-terminated, max 256 bytes).
- **Special formats**:
  - **Coord**: `i16 * 0.125` (1/8 unit fixed-point) — use `read_coord`/`write_coord`.
  - **Angle**: `i8 * 1.40625` (360/256) — use `read_angle`/`write_angle`.
  - **Angle16**: `i16 * 360/65536` — use `read_angle16`/`write_angle16`.
  - **Dir**: 162-entry vertex-normal table (`BYTEDIRS`) — compressed 3D vectors.

### Delta Compression

Usercmds are delta-compressed against the previous frame to save bandwidth. The
`build_clc_move` function ported from `movemsg.c`:

1. Compute a bitmask of changed fields (`CM_ANGLE*`, `CM_FORWARD`, etc.).
2. Write only the changed fields.
3. Use `bytedirs[]` to compress 3D angles into a single byte.
4. **Minimum size**: 3 bytes (bits + msec + lightlevel) for an unchanged command.

### Connectionless Packets (OOB)

Out-of-band packets are prefixed with `0xff 0xff 0xff 0xff` and carry text commands:

- `getchallenge\n` → request a challenge number.
- `connect 34 <qport> <challenge> "<userinfo>"` → initiate connection.
- `status\n` → query server status (map, player list).

See [`src/oob.rs`](src/oob.rs) for the OOB encoder/decoder.

### Server Frames

The server sends `svc_*` opcodes in a reliable stream. `q2proto` parses:

- `svc_serverdata` — protocol version, spawncount, gamedir, client number.
- `svc_configstring` — indexed string table (models, sounds, statusbar).
- `svc_frame` — playerstate + entity deltas, delta-decoded against 16-frame history.
- `svc_print` / `svc_sound` / `svc_stufftext` — events and commands.

See [`src/frame.rs`](src/frame.rs) and [`src/ops.rs`](src/ops.rs).

### What This Crate Does NOT Do

- **No UDP sockets** — you provide the transport (tokio, async-std, etc.).
- **No netchan reliability** — sequence tracking, retransmission is `client/`'s job.
- **No game logic** — entity interpretation, bot AI is `brain/`'s job.

---

## Testing

Unit tests verify round-trips with hand-built bytes:

```bash
cargo test -p q2proto
```

Key test cases:
- `full_delta_round_trips` — all fields change, decode equals encode.
- `unchanged_cmd_is_3_bytes` — delta compression minimum size.
- `clc_move_checksum_is_self_consistent` — CRC over body + sequence.
- `oob_round_trip_via_reader` — prefix detection + payload extraction.
- `infostring_set_get_remove_cycle` — InfoString CRUD operations.

The codec is **pure functions over bytes** — no integration tests needed. Every
`MSG_*` operation is tested against known C outputs.

**Got it wrong?** Check [`context/pitfalls.md`](../../context/pitfalls.md) for known
gotchas and multi-attempt fixes.

## Protocol Reference

- **Version**: 34 (`PROTOCOL_VERSION`)
- **OOB prefix**: `[0xff, 0xff, 0xff, 0xff]` (also `i32 -1`)
- **Delta backup**: 16 frames (`UPDATE_BACKUP`)
- **Opcodes**: [`SvcOp`], [`ClcOp`] (from `common.h`)

## Cargo Features

**None.** This crate has zero dependencies beyond `bytes`.

---

## Sources

| Feature | yquake2 Source |
|---------|----------------|
| MSG_* R/W | `common/msg.c` |
| Usercmd delta | `common/movemsg.c` |
| OOB framing | `client/cl_network.c`, `server/sv_conless.c` |
| Netchan CRC | `common/netchan.c` |
| bytedirs[] | `common/header/shared.h` (162-entry vertex-normal table) |

---

## License

MIT / Apache-2.0 (same as the rest of qbots).
