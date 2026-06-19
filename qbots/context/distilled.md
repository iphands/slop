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
  (`world` `path_weighted` unit test). **Live-server confirmation pending** — the
  server was reachable (UDP) but the running 8-bot fleet used pre-heatmap code, so
  the new overlay wasn't live-exercised; the per-tick `snapshot()` debug log
  (`total_danger`/`max_danger`/`hot_nodes`) is wired to confirm it next time the
  new binary runs against a game.

## Fleet (`qbots` binary — Plan 09, VERIFIED LIVE 8 bots)
- **`qbots status`** is the verification lens: connectionless OOB
  `\xff\xff\xff\xffstatus\n` → server replies `\xff\xff\xff\xffprint\n` +
  `SV_StatusString()` (infostring line, then `<frags> <ping> "<name>"` per
  client). Parser in `qbots/src/status.rs` (unit-tested). Note: this server's
  serverinfo exposes `maxclients` (25) but not `map` — the field is `None`
  though players are clearly mid-game; don't treat a missing `map` as "no map".
- **8-bot fleet verified live** (qb0-qb7, 2026-06-15): all connected over a ~2 min
  run, frags accumulating (4 bots scored in a 40 s window), no kicks, 10/25
  maxclients. Stagger `connect_stagger_ms=250` is enough — no connectionless
  flood / IP ban. qport scheme `qport_base + i` (28000+i) keeps bots distinct.
- **Reconnect**: per-bot exponential backoff 1 s → 15 s cap, `max_reconnects`
  (0=unlimited). Nav graph cached once per map in `NavCache` (built under a lock,
  shared as `Arc`); a build failure degrades a bot to nav-less, not a crash.
- **CPU**: one tokio task per bot; shared read-only `Arc<NavGraph>` is the
  multiplier. Per-tick overlay alloc is `node_count` floats/bot (~few KB at 10 Hz)
  — fine at this scale; reuse a buffer if pushing past ~32 bots.

## Movement-quality root causes (diagnosed 2026-06-15, pre-Plan 10)
Bots move poorly in 3D — bumping walls, facing wrong way while advancing, rotating
in one spot, not chasing players. **NOT** a wire/`delta_angles` bug (that's fixed —
`move_ctrl.rs` subtracts `delta_angles`; combat aim also routes through `build_cmd`
→ `angle_short`, so both axes are corrected). The bugs are in the **steering layer**:

1. **No LOS anywhere.** `view.nearest_enemy(90°)` (`perception.rs:266`) filters by FOV
   cone only — **no BSP trace**. `CombatDriver::select_target_entity` and `FSM::transition`
   both call it, and nav-to-enemy uses the target's raw origin. → bots chase/shoot/walk
   at enemies *through walls*. Fix → Plan 11.
2. **Waypoint orbiting.** `next_waypoint_direction` aims at the raw current node
   (`nav.rs:177`); advance gate is 3D-Euclidean `dist<64` (`nav.rs:195`) — no Z-aware
   threshold, no look-ahead, no arrive. A node the bot can't close to 64u (Z lip, off-edge
   drift) → it circles the node and `atan2(dir)` spins every tick = "rotating in one spot".
   Fix → Plan 12.
3. **No turn-rate limit.** `mv.look_at` snaps yaw each tick (`main.rs:573/631/652`);
   Q2 imposes **no** server-side turn cap (client sends absolute `cmd.angles`,
   `PM_SetAngles` `pmove.c:1255`), so snaps *work* but look inhuman and feed the orbit
   jitter. Eraser uses `M_ChangeYaw` (`yaw_speed` clamp). Fix → Plan 12.
4. **Blind stuck recovery.** `move_forward(-1.0)+jump+replan` (`main.rs:681-687`) reverses
   into whatever's behind; no strafe, no fan-out (Eraser `botRoamFindBestDirection`
   7-dir fan unported). Two divergent stuck detectors: `nav.rs` `<16u`/30t and
   `main.rs` `<1u`/50t. Fix → Plan 13.
5. **Aim-yaw == move-yaw.** One `mv.yaw` couples facing to movement; engaging means
   walking straight at the enemy (view-relative `forwardmove`), can't circle-strafe.
   Fix → Plan 12.
6. **Grid-zigzag paths.** Spacing-64 uniform grid (`main.rs:1007`), no funnel/string-pull
   smoothing → stair-step paths, corner clip, slow elapsed time. Fix → Plan 14 (deferred).

**Measurement gap**: no per-frame telemetry exists. Plan 10 adds `spawn-to-spawn` /
`spawn-to-weapon` harnesses that record pos/yaw/pitch/vel/speed/waypoint/wall-bumps/
wrong-turns/hindered/facing-vs-move-delta/elapsed → `./logs/<scenario>/<ts>.<bot>.log`.
Elapsed time is the headline ability metric. Constraints: no cheating (no clip/overspeed/
wallhack) — bots must achieve speed *through better control*, not physics violation.

**Physics oracle** (`vendor/yquake2/src/common/pmove.c`): `pm_maxspeed=300`,
`pm_accelerate=10`, `pm_friction=6`, `pm_stopspeed=100`, jump `+=270`, air-accel `0`
default; `wishvel=forward*fmove+right*smove`, `wishspeed` clamped to `pm_maxspeed`
(no sqrt(2) diagonal clamp). `forwardmove/sidemove` are ±400-scale but capped by maxspeed.
`STEPSIZE`: Q2 `18` vs Eraser `24` (verify which the live server uses before tuning steps).

## LOS (line-of-sight) trace — Plan 11
Use a **zero-size** (`mins=maxs=[0;3]`) `CollisionModel::trace` from eye to target; `fraction ≥ 1.0`
and `!startsolid` = clear. Eye = `origin + [0,0,22]` (Q2 standing viewheight). Check both chest
(`+12z`) and feet (`-20z`) so partially-covered enemies still register as visible. Gate combat
target acquisition AND nav-goal override on this check. Keep a 2-frame grace after LOS loss so
thin-pillar flicker doesn't cause target thrashing. The server already PVS-filters entities, so
the LOS pass runs only on ≤8 visible candidates — cheap at 10 Hz.

## Water navigation — swim nodes/edges + swim movement (Plans 39–40)
The A* `NavGraph` + brain now traverse water; before, all water positions were discarded so
water-only routes (q2dm1 railgun, reachable ONLY by swimming) were isolated → A* NO PATH.
Fix: sample submerged+surface swim nodes, connect them with 3-D `EdgeKind::Swim` edges (no
STEP gate, `|dz|<=48`, cost ×2, reduced `±12` hull, `CONTENTS_WATER` only), add dry↔water
entry/exit (the exit edge fuses the railgun room), protect swim edges from the prune, bump
mapcache to v13. Brain: recompute `water_level` (it's not on the wire) like
`PM_CategorizePosition`; on a swim edge set sustained `intent.up` (NOT one-shot `jump`) +
pitch toward the 3-D target; climb out with the Q2 water-jump (look up `<=-15` + forward,
`pmove.c:414`); suspend recovery while swimming. Live q2dm1: `astar` reaches the railgun
(~11–27 s). **Full detail in `context/distilled/pathing/water_nav.md`.**

## Hybrid navigation modes — Plan 20
Both nav backends (waypoint-graph A* `astar`, polygon-mesh `navmesh`) implement one
`Navigator` trait (`brain::nav_mode`), so a hybrid is a thin `Navigator` that owns **both**
sub-drivers and delegates per trait call. They share `brain::hybrid::Sub` (astar+navmesh+graph)
and `goal_to_pos`/`goal_key` (navmesh ignores `NavGoal::Waypoint`, so resolve it to the node's
world pos first). Dispatch for both call sites (`bot_task`, movement scenarios) goes through one
`build_navigator` factory in `qbots/main.rs` that builds the mesh lazily (`get_or_build_navmesh`
is cached; skip for pure `astar`). Four modes, by selection strategy:
- **`hybrid-fallback`** (reactive): A* drives; a `force_replan` (raised by the loop's
  `StuckDetector::Hard`) while A* is active = "A* wedged" → switch to navmesh for the rest of
  the goal; a changed goal re-arms A*.
- **`hybrid-race`** (selective): on a changed goal, plan both, score
  `len + 64·jumps + 256·recent_stuck[backend]` (graph: `planned_cost`+`planned_jump_count`;
  navmesh: `planned_len`), run the lower; stuck recovery replans the active backend (no per-tick
  switch — that's fallback's job). `recent_stuck` halves each new goal so old wedges fade.
- **`hybrid-hier`** (cooperative): navmesh plans the corridor; each tick project the bot onto
  `navmesh.path()` and feed A* a sliding sub-goal `point_ahead(LOCAL_HORIZON≈300)`; all
  steering/jump trait calls delegate to A*. No corridor → A* straight to goal.
- **`hybrid-segment`** (ownership): navmesh owns open routing; when the bot is within ~96u of a
  graph node with a goal-ward `EdgeKind::Jump` (cos > 0.26), A* takes the segment to execute the
  launch (navmesh's `current_edge_is_jump` is always false), then control returns to navmesh.

Gotcha: `NavigationDriver::current_edge_is_jump` keys on `(prev_waypoint → current_waypoint)`,
and `prev_waypoint` is `None` until the first waypoint advance — so a jump on the **first** edge
of a fresh plan isn't reported until the bot advances off the start node. Hybrids that rely on it
(segment) must not assume the jump flag fires immediately after `set_goal`.

## Live mode competition — `qbots competition`
Spawn N bots **per** nav `--mode` in one process to see which backend frags best:
`qbots competition [--count N] [--modes astar,navmesh,hybrid-race] [--qport-base B]`.
One shared `NavCache` across all modes (graph + navmesh built **once**, not per-mode — verified:
1 `nav graph ready` / 1 `navmesh built` for a 6-mode run), one distinct skin per mode (so fleets
are tellable apart), bots named `<mode_tag>_<i>` (`astar,navmesh,fallback,race,hier,segment`).
Prints a per-mode K/D scoreboard every 30 s (live) and on Ctrl-C (FINAL), grouping
`FleetStats::snapshot()` by the name prefix. Reuses the fleet supervisor with a **per-bot**
`NavMode` (moved out of `FleetShared`). Per-mode qport blocks `base + mi*count + i` are disjoint.
Total clamped by `[fleet].max_bots` (server maxclients headroom). In-process (no 6× nav rebuild,
no shell-outs); a panicking bot task is isolated by tokio.
