# Plan 40 — Swim Movement, Water-Exit, and Navmode Ranking

> **Status**: pending
> **Created**: 2026-06-19
> **Depends on**: Plan 39 (water nav graph), Plan 10 (movement harness), Plan 12/13
> (steering/recovery), Plan 26 (`runtester` brain)
> **Goal**: Make bots actually swim the water route — emit vertical movement on swim
> edges, surface, and climb out onto the railgun ledge — then prove + rank the fix by
> running `spawn-to-weapon railgun` across all six navmodes.
> **Agent**: implementation agent (ralph-loop / sub-agent)

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Plan 39 gives the nav graph swim nodes/edges; this plan makes the **brain**
execute them. Today **no brain ever sets `intent.up`** (`grep` confirms: only `jump` ever
touches `upmove`, which is a fixed 270 launch — useless for sustained swimming). Bots also
actively avoid water: recovery's `find_best_direction` `continue`s past any water endpoint
(`recover.rs:158-160`). So even with swim edges, a bot would not descend, swim, or surface.
This plan adds water detection + vertical swim intent + water-exit, water-aware recovery,
and a recorder flag — then runs the ranking sweep that proves the railgun is reachable.

**Deliverables**:
1. Water-state detection in the brain (we own the `CollisionModel`, so compute it
   ourselves — no wire data needed).
2. Swim movement: on a swim edge / while submerged, set `intent.up` toward the 3-D target
   and pitch toward it; sustained vertical (not the one-shot `jump`).
3. Water-**exit**: surface + climb onto the railgun ledge (Q2 water-jump via look-up +
   forward, or held `upmove`).
4. Water-aware recovery: don't treat slow swim/bob as "stuck"; don't strand the bot away
   from water; don't blacklist swim edges.
5. Recorder `S` (swimming) per-frame flag.
6. **Proof + ranking**: `spawn-to-weapon railgun --count 1 --max-secs 300 --navmode <m>`
   for all six navmodes; a results table (reached/elapsed) in the tracker + `mode_perf`.

**Estimated effort**: Medium (1 day)

---

## Context

### What's missing (verified 2026-06-19)

- `MovementIntent.up` exists and `move_ctrl.rs:123` encodes it as `upmove`, but **no brain
  sets it** (`grep -rn "\.up" crates/brain` → only the sentry test + the encoder).
- `intent.jump` sets `upmove = JUMP_VELOCITY (270)` once (`move_ctrl.rs:130`) — a launch,
  not sustained swim thrust.
- Recovery steers **away** from water: `find_best_direction` skips any candidate whose
  endpoint is over water (`recover.rs:158-160`). The `StuckDetector` (4u/1s) will also fire
  while a bot bobs at the surface or swims slowly (water move is 0.5× speed) → false stuck.

### Q2 water physics we exploit (`vendor/yquake2/src/common/pmove.c`)

- `PM_WaterMove` (`:545`): `wishvel = pml.forward*forwardmove + pml.right*sidemove`, then
  `wishvel[2] += upmove`. **`pml.forward` is the full 3-D view vector** (includes pitch).
  So vertical motion comes from *either* (a) `upmove > 0`, *or* (b) pitching up/down while
  pressing `forwardmove`. With no input, `wishvel[2] -= 60` (sink). Speed is halved
  (`:579 wishspeed *= 0.5`).
- **Water-jump / climb-out** (`PM_CheckSpecialMovement`, `:414-426`): when
  `viewangles[PITCH] <= -15` (looking up) **and** `forwardmove > 0` and the forward path is
  blocked, the engine grants a climb-out boost. This is how a player "holds jump to surface"
  / climbs onto the railgun ledge. So **face the ledge + look up + press forward** to exit.

### Detection: we compute waterlevel ourselves

The wire does not carry `waterlevel`. But the brain holds `cm: &CollisionModel` and
`view.self_state().origin`, so replicate pmove's check (`pmove.c:765-790`): sample
`CONTENTS_WATER` at feet (`origin.z + mins.z + 1`), waist (`origin.z`), and eye
(`origin.z + viewheight`) to derive `waterlevel ∈ {0,1,2,3}`. `≥2` = swimming;
`==3` = fully submerged (head under). Surface = transition `3→2` while ascending.

### Backend coverage (sets ranking expectations)

Plan 39 adds water to the **A* graph only**. So the ranking sweep is expected to show:
`astar` (and the A*-driven portions of `hybrid-fallback` / `hybrid-segment` /
`hybrid-hier` local) **reach** the railgun; pure `navmesh` and navmesh-corridor selection
(`hybrid-race`, `hybrid-hier` global) **do not**. The sweep *documents* this split rather
than hiding it; navmesh water is a separate follow-up.

---

## Step-by-Step Tasks

> Commit after **every** task (Rule B). `cargo fmt` + `cargo clippy -- -D warnings` +
> `cargo test` green before each commit (Rule A).

### T1: Water-state helper

**File**: `crates/brain/src/water.rs` (new), wired into the brain tick.

**What to do**: `pub fn water_level(cm: &CollisionModel, origin: Vec3) -> u8` returning
0–3 per the pmove feet/waist/eye sampling above (use `CONTENTS_WATER`, not `MASK_WATER` —
lava/slime are never swum). Add a small `WaterState { level, submerged, surfaced_this_tick }`
if helpful. Unit-test against synthetic contents.

### T2: Swim movement in the nav-follow path

**Files**: `crates/brain/src/brains/runtester.rs` (primary — the `spawn-to-weapon` driver),
then `crates/brain/src/brains/main.rs` (so live bots swim too).

**What to do**: After computing `pursue_pos` (the 3-D look-ahead), when
`nav.current_edge_is_swim()` **or** `water_level >= 2`:
- Set `intent.up` = `clamp((target.z - pos.z) / SWIM_VERT_SCALE, -1.0, 1.0)` for sustained
  vertical thrust (do **not** use `mv.jump()` in water).
- Set `pitch` toward the 3-D target (so `pml.forward` carries the vertical component too) —
  but keep aim/move separation: in scenario mode aim follows movement, so pitch toward the
  target is fine; in `main`, gate this so combat aim still wins when firing.
- Keep `forwardmove` toward the XY target as today.
- Skip the "slow on narrow ledges" `speed_scale` damping while swimming (water is open
  volume, not a thin ledge).

### T3: Water-exit / surfacing (the railgun-ledge climb-out)

**Files**: `crates/brain/src/brains/runtester.rs`, `main.rs`.

**What to do**: When the **current edge is a swim→dry exit** edge (next node dry and at/above
the surface) or `water_level` is dropping toward a dry node:
- Face the exit node, press `forward`, and set `pitch <= -15` (look up) to trigger the Q2
  water-jump climb-out (`pmove.c:414`). Hold `intent.up = 1.0` as well so a pure
  hold-to-surface also works.
- Add a short hysteresis so the bot doesn't oscillate at the lip (e.g. keep "exiting" mode
  for N ticks once started until `water_level == 0`).

### T4: Water-aware recovery

**File**: `crates/brain/src/recover.rs` + its callers in the brains.

**What to do**:
- Gate the `StuckDetector` while `water_level >= 2`: swimming/bobbing is not a wedge — use a
  relaxed threshold or suspend stuck escalation in water (a true stuck in water should rely
  on swim re-path, not blind reverse).
- Do **not** `nav.blacklist_waypoint_if_blocked` on swim edges (the swim trace is legit).
- `find_best_direction`'s water `continue` (`recover.rs:158-160`) must not run when the bot
  is *supposed* to be in water — either pass a "in_water" flag that disables the water-skip,
  or skip `find_best_direction` entirely while swimming.

### T5: Recorder `S` (swimming) flag

**File**: `crates/brain/src/recorder.rs`.

**What to do**: Add a `swimming` bool to `FrameRecord`, emit `S` in the `flags()` run (next
to `B/W/H/A/R`), and document it in the module header schema. The scenario tick sets it from
the T1 water state. Lets the SUMMARY/per-frame log show where the bot swam.

### T6: Build + unit tests

**What to do**: Unit-test the swim-intent logic with `StubNav` (swim edge set →
`intent.up != 0` toward target; exit edge → look-up pitch + forward). Keep the existing
runtester/main tests green. `cargo test`, `clippy -D warnings`, `fmt`.

### T7: Live proof + navmode ranking sweep

**What to do**: With a server running q2dm1 (discover/confirm the map first per
`qbots/CLAUDE.md`), run for **each** navmode:

```bash
cargo run --release --bin qbots -- spawn-to-weapon railgun --count 1 --max-secs 300 --navmode <mode>
```

for `mode ∈ {astar, navmesh, hybrid-fallback, hybrid-race, hybrid-hier, hybrid-segment}`.
Collect each run's `# SUMMARY reached=… elapsed=… mean_speed=… …` line and fill the ranking
table in the tracker. Expected: `astar` (+ A*-backed hybrids) `reached=1`; pure `navmesh`
`reached=0` (documented limitation). Re-run any fl/borderline mode 2–3× for stability.

### T8: Knowledge capture + close-out

**Files**: `context/brain_notes.md` (mandatory dated section — brain-notes discipline),
`context/distilled.md`, `context/pitfalls.md`, `context/plans/mode_perf.md` (if present).

**What to do**: Append a dated `brain_notes.md` section (swim model, exit trick, results).
Record the swim-movement + water-jump-out approach in distilled and the "no brain set
`intent.up`; recovery avoided water" pitfall. Add the ranking table to `mode_perf`. Update
`SERIES.md`; move Plan 39 **and** 40 (+ trackers) to `completed/` once green (Rule C).

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/water.rs` (new) | waterlevel detection from cm + origin | P0 |
| `crates/brain/src/brains/runtester.rs` | swim intent + exit (the scenario driver) | P0 |
| `crates/brain/src/brains/main.rs` | swim intent + exit for live bots | P1 |
| `crates/brain/src/recover.rs` | water-aware stuck/wall-avoid gating | P0 |
| `crates/brain/src/recorder.rs` | `S` swimming flag | P2 |
| `context/brain_notes.md`, `distilled.md`, `pitfalls.md`, `mode_perf.md` | capture + ranking | P1 |

---

## Open Questions / Risks

1. **Pure-navmesh modes won't reach** (no navmesh water — Plan 39 scope). *Mitigation*:
   expected; the ranking documents the split; follow-up plan for navmesh water if wanted.
2. **Water-jump climb-out is timing-sensitive** (needs look-up + forward + blocked path).
   *Mitigation*: also hold `upmove=1.0` so a plain hold-to-surface works; add exit
   hysteresis; tune on q2dm1 with the recorder `S`/`H` flags.
3. **`main` aim vs. swim-pitch conflict** (combat wants aim pitch; swimming wants target
   pitch). *Mitigation*: combat off in the scenario (runtester); in `main`, let combat aim
   win when firing, else use swim pitch.
4. **No live server in CI.** *Mitigation*: T6 unit-tests the intent logic deterministically;
   T7 is a manual live acceptance step (like the Plan 26/37 live sweeps).
5. **Breath/air not handled** (long swims could drown). *Mitigation*: the railgun swim is
   short; full air/breath monitoring is the existing pending **Plan 32** (folds on top).

---

## Verification Checklist

- [ ] T1: `water_level` returns 0–3 correctly (synthetic contents unit test).
- [ ] T2: on a swim edge, `intent.up` is nonzero toward the target; no `jump()` in water.
- [ ] T3: exit edge → look-up pitch + forward + up; bot climbs onto a dry ledge in a test.
- [ ] T4: `StuckDetector` does not escalate while swimming; swim edges never blacklisted.
- [ ] T5: recorder emits `S` while submerged; schema doc updated.
- [ ] T6: `cargo test` green; `clippy -D warnings` clean; `fmt` applied.
- [ ] T7: **live `spawn-to-weapon railgun` `reached=1` on `astar`** (and A*-backed hybrids);
      full six-navmode ranking table recorded with elapsed times.
- [ ] T8: `brain_notes.md` dated section added; distilled + pitfalls + mode_perf updated;
      Plans 39 & 40 moved to `completed/`; SERIES updated.
