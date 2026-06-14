# Distilled — confirmed protocol & implementation facts

Compact learnings verified against yquake2 source **and** a live server (`noir.lan:27910`).
Read before new work. Append new findings; keep it dense.

## Q2 connect handshake (protocol 34) — VERIFIED LIVE
Reaches "test connected" / "test entered the game" on a real yquake2 server:
1. OOB `getchallenge\n` → server `challenge <N> p=34`
2. OOB `connect <34> <qport> <N> "<userinfo>"` → server `client_connect`
3. netchan up; queue reliable `clc_stringcmd "new"` → server `svc_serverdata`
4. on `svc_serverdata`: queue reliable `clc_stringcmd "begin <servercount>"` → spawn

- **userinfo rides in the `connect` OOB** (argv[4]); no separate `clc_userinfo` at handshake.
- **No `clc_move` needed to stay connected.** An empty netchan transmit (header + qport
  only) refreshes the server's `last_received`; the server keeps a client that never sends
  usercmds. Real movement (`clc_move` + `COM_BlockSequenceCRCByte` checksum) is for
  *moving the player*, not connectivity. → Plan 04.
- **Precache can be skipped.** Sending `begin` right after `serverdata` (no download loop,
  no map assets) → spawn succeeds. External bot needs no `.bsp`/`.pak` to join.

## Netchan framing (`common/netchan.c`)
- Header: `w1 = outgoing_seq | (reliable<<31)`, `w2 = incoming_seq | (incoming_reliable<<31)`,
  then `qport` (short) on **client→server only**; server→client has **no** qport.
- `outgoing_sequence` starts at **1** (`Netchan_Setup`), not 0.
- Reliable ack: when server's `w2` reliable bit == our `reliable_sequence`, the in-flight
  reliable is acked (`reliable_length = 0`).
- Stale/dup: drop if `sequence <= incoming_sequence`.

## Wire codec gotchas (`common/movemsg.c`)
- **coord = `WriteShort × 8`** (fixed-point 1/8 unit), NOT a float. `read = i16 * 0.125`.
- **angle = signed byte** (`ReadChar`); 180° ↔ byte 128 ↔ reads back as −180° (≡180). Don't
  assert round-trip equality at 180.
- `bytedirs` (162 entries) copied verbatim from `movemsg.c` — index 0 doubles as the
  "null direction" fallback.
- `MAX_CONFIGSTRINGS = 2080` for yquake2 (MAX_CLIENTS=256), **not** classic Q2's 1024.

## Tooling
- A connect `--addr` may be a **hostname** (`noir.lan`): resolve with
  `tokio::net::lookup_host`, not `SocketAddr::from_str` (which rejects hostnames). Pass
  the owned `String` into `lookup_host` to avoid a borrow-across-`await` error.
