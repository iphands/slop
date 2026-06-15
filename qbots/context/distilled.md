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

## svc_frame — perception (VERIFIED LIVE)
- `svc_frame` body (`CL_ParseFrame`, cl_parse.c:739): `serverframe`(i32) `deltaframe`(i32)
  `surpressCount`(byte) `areabits`(len-byte + data) `svc_playerinfo` + player_state
  `svc_packetentities` + entity loop.
- Entity-list terminator = `MSG_WriteShort(0)` (sv_entities.c:150) → 2 zero bytes, decoded
  as `bits=0` + `number=0` (not one byte).
- `event` is single-frame: force-cleared to 0 when `U_EVENT` is absent.
- Entity delta field order matters (decoder reads FRAME8 before ORIGIN1, etc.).
- Confirmed live: frames stream at ~10 Hz, delta-resolve across the 16-frame ring, `ents`
  tracks the PVS. Bot perceives its own origin + visible world.

## clc_move — movement (VERIFIED LIVE)
- Format (`CL_SendMove`, cl_input.c:786): `clc_move` op + checksum byte + serverframe ack
  (i32, or -1) + **three** delta usercmds (`nullcmd→a, a→b, b→c`).
- Checksum = `COM_BlockSequenceCRCByte(body_after_checksum, len, outgoing_sequence)`
  (crc.c:157). The server validates it strictly — wrong byte → move dropped/kick. Ported
  correctly: **bot walks, no kick.**
- CRC is CRC-16/CCITT-FALSE (poly 0x1021, init 0xffff); check value of "123456789" = 0x29B1.
- `chktbl[1024]` in source has only **960** initializers — C zero-pads; the trailing 64
  bytes (readable at high sequence) must be explicit zeros.
- `sequence` for the checksum = the netchan `outgoing_sequence` that `transmit` will write
  to `w1` (i.e. the pre-increment value).
- Timing: 3 usercmds × `msec=33` ≈ 99 ms per 100 ms heartbeat ≈ realtime. Observed the bot
  walk at ~100 u/s with `forwardmove=400`; yaw 0 ⇒ forward ≈ −X here.

## BSP / pak loading (VERIFIED LIVE)
- Q2 BSP = IBSP magic + version **38** + 19 lumps (`dheader_t`, `files.h:294`). Lumps:
  `{i32 fileofs, i32 filelen}`.
- `.pak` format (`files.h:30`): header `[b"PACK", dirofs, dirlen]` + dir of 64-byte
  `dpackfile_t` `[name[56], filepos, filelen]`. Stock DM maps (`q2dm1`…`q2dm8`) are in
  **pak1.pak**; single-player maps in pak0. Loader searches `pak0..9` ascending.
- Collision structs: `dplane_t`(20B) `dnode_t`(28B) `dleaf_t`(28B) `dbrush_t`(12B)
  `dbrushside_t`(4B) `dmodel_t`(48B); `leafbrushes` are `u16`. Node children: leafs encoded
  as `-(leaf+1)`.
- `q2proto::Reader` (LE codec) parses all structs; `Reader` has no `read_u16` — read as
  `i16` then `as u16` (bit-correct for unsigned values).
- Verified counts (real maps): q2dm1 = 2408 planes / 2250 leafs / 960 brushes; base1 =
  8558 planes; 007_facility = 5020 planes.

## Collision trace — `gi.trace()` replacement (VERIFIED LIVE)
- `cplane_t` (`shared.h:578`) adds `signbits` = `signx | signy<<1 | signz<<2` (set when
  `normal[j] < 0`), computed at load (`collision.c:1463`). `type < 3` ⇒ axial fast path
  (`d = p[type] - dist`).
- `DIST_EPSILON = 0.03125` (`collision.c:127`) — nudge for the plane-cross split + brush clip.
- Trace sweeps via `CM_RecursiveHullCheck` (split the ray at each node's plane into near/far
  segments, `frac = (t1 ∓ offset ± EPS)/(t1-t2)`), then `CM_TraceToLeaf` → `CM_ClipBoxToBrush`
  (track enter/leave frac across the brush's planes; `enterfrac < leavefrac` ⇒ hit).
- Node children: leafs encoded `-(leaf+1)`. `BoxOnPlaneSide` (corners method) for the
  position-test leaf gather.
- Brush dedup across adjacent leafs uses a per-trace `HashSet` (the C `checkcount` trick
  would need interior mutability; dedup is an optimization, not correctness).
- **VERIFIED on q2dm1**: bounds [-256,-464,-256]..[2240,1808,1920], center (992,672,832)
  `is_solid=false`; 8 horizontal rays from center hit walls at 288–800 units in every
  direction — the tracer is byte-correct against real geometry.

## PVS — cluster visibility (VERIFIED LIVE)
- Vis lump = `dvis_t` header `[numclusters:i32][bitofs[numclusters][2]:i32]` + RLE
  bitvectors. `bitofs[c][0]` (PVS offset) is at byte `4 + c*8`.
- `CM_DecompressVis`: nonzero byte = 8 literal bits; `0x00 <count>` = skip `count` zero
  bytes. Output is `(numclusters+7)/8` bytes (use `.div_ceil(8)`).
- `cluster == -1` (solid/void) ⇒ nothing visible; missing vis ⇒ everything visible.
- The server only sends entities in the viewer's PVS — so this both explains what we see
  and is a cheap LOS pre-filter. True LOS still needs `trace` (T2) — PVS over-approximates.
- **VERIFIED on q2dm1**: 925 clusters; center (cluster 553) sees 336 of them.

## Danger/popularity heatmap nav overlay (Plan 08)
A per-bot runtime cost overlay on the **static** BSP nav graph (the realistic
external-client analog of Eraser's withheld dynamic-route engine — novel; Eraser
owns the world so has no "observation" to learn from at runtime). Topology is
read-only; only per-node edge weights breathe.

- **Cost model**: A\* edge cost `cur→nb = (base_len + overlay[cur]).max(EPS)`,
  `EPS=1`. Overlay = `W_d·danger − W_p·popularity`. `src`-gated (you pay/credit
  a node's weight to *leave* it). Reachability unchanged → `None` iff unweighted.
- **Signals** (all PVS-honest — we fear/credit only places we've located):
  - self death/damage (health-detected) → our own node, highest confidence. A
    death bumps danger `DANGER_BUMP_DEATH=1.0` and forces a replan.
  - enemy presence → popularity EMA toward 1 at each visible enemy's nearest
    node; cools via uniform `decay`.
  - obituary `svc_print` → victim's last-known node, **only if observed within
    `PLAYER_NODE_TTL` (~2 s)**; self/unobserved/stale victims are no-ops.
- **Decay**: danger `*= exp(-dt/TAU_DANGER)`, `TAU_DANGER=45 s`; popularity `*=
  exp(-dt/TAU_POP)`, `TAU_POP=90 s` (slower — busy-lane vs just-died-here).
  Danger capped `DANGER_MAX=8`.
- **Per-skill weights** (`BotSkill::heatmap_weights`): `W_danger = 30 + skill*12`
  (0→30, 10→150; skilled = risk-averse); `W_pop` = 8/20/40 for
  Conservative/Balanced/Aggressive (aggressive seeks hot lanes).
- **Desperate fallback**: if a weighted path is `>5×` the straight-line distance
  (region all-hot), re-query unweighted (drop `W_d`). Below `256 u` straight-line
  the guard is skipped (ratio meaningless for tiny goals).
- **PVS-honesty**: obituary victim resolution matches known player names at word
  boundaries (`find_name`); names come from `CS_PLAYERSKINS=1312` (derived:
  `CS_MODELS(32)+256·5`; validated vs `MAX_CONFIGSTRINGS=2080`). Self-death via
  health, not the obituary (exact origin, no double-count).
- **Composes with Plan 07 T3** tactical dodge by construction: strategic (this)
  picks path/goal; tactical overrides a frame to dodge an imminent rocket.
- **Verification**: `crates/brain/tests/heatmap_pipeline.rs` deterministically
  proves repeated deaths at a chokepoint flip the route to a detour, decay
  (~TAU_DANGER) restores the direct route, and high-skill detours after one
  death where low-skill does not. Gravitation proven at the pathfinding level
  (`world` `path_weighted` unit test). **Live-server confirmation deferred** —
  `noir.lan:27910` was down at 2026-06-15; the per-tick `snapshot()` debug log
  (`total_danger`/`max_danger`/`hot_nodes`) is wired to confirm it live once the
  server is back.
