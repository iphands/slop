# Plan 50 — Lava-Proof Base Edges + In-Lava Escape Override

> **Status**: in-progress
> **Created**: 2026-07-10
> **Depends on**: Plan 48, Plan 49
> **Goal**: Bots neither route across lava trenches nor burn to death standing in a pool — entry is prevented at the graph, and any bot that still lands in lava sprints out.
> **Agent**: implementation agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Close the two lava gaps the Plan 48 post-fix soak exposed: flat walk edges are
hull-traced but never floor-validated (routes cross narrow lava trenches), and no behavior
exists for "I am standing in lava" (bots burn 15+ s to death, made worse by Plan 48's hazard
gate zeroing all movement inside a pool).

**Deliverables**:
1. Build-time floor-continuity (lava-aware) validation on flat walk edges + deadly-floor check in `walkable_stair` (cache v22).
2. `hazard::escape_from_lava` — survival override wired into main/q3/zb2 as the final movement word.
3. Post-fix soak vs the Plan 48 baseline soak (same command/duration) with burn-frame + death metrics.

**Estimated effort**: Small–Medium (half day)

---

## Context

### Soak evidence (2026-07-10, q2dm3, 305 s, 3×main + 3×q3 + 3×zb2, post-Plan-48 code)

- Damage histogram shows the Q2 lava-burn signature: `damage=3` ×90, `damage=6` ×111,
  `damage=9` ×159 (3 × waterlevel per 0.1 s server frame) — **360 burn frames ≈ 36 s of
  lava contact** in 5 minutes.
- Sustained sequences: one bot burned 100→0 at 6/frame over ~15 s (log 0012.3–0015.7,
  two bots burning concurrently). Bots ENTER and then NEVER LEAVE.
- Scoreboard: main 1.06, q3 0.54, zb2 0.26 (8 kills / 31 deaths).

### Pre-Identified Bugs

**BUG E1 — flat walk edges are never floor-validated** (`world/src/navgraph.rs:300-302`).
A flat/gentle edge (|dz| ≤ STEP) is accepted on a hull trace alone. The hull flies at body
height, so a lava trench narrower than `CONNECT_RADIUS` (72 u) between two safe rim nodes
does not block it — the edge exists, A* uses it, and every brain walks the route into the
trench. `segment_has_floor` (lava-aware since Plan 48) guards only smoothing/look-aheads;
`pursue_target_safe`'s fallback steers at the far NODE, which still crosses the trench.
`walkable_stair`'s floor probes are also `MASK_SOLID`-only (same lava-bed blindness).

**BUG E2 — no in-lava escape; Plan 48's hazard gate freezes burning bots**
(`brain/src/hazard.rs`, all brains). Standing in a pool, `dir_is_hazardous` is true in every
direction → `safe_combat_dir` returns `None` (stand and fight) and `safe_strafe_dir` returns
0 — correct at the RIM, lethal INSIDE. `find_best_direction` rejects all `MASK_WATER`
endpoints → `UseHeading` unavailable. Lava is not `CONTENTS_WATER`, so no swim machinery
runs either. Result: a bot in lava has no code path that gets it out.

## Step-by-Step Tasks

### T1: Floor-validate flat walk edges + deadly-aware stair probes (cache v22)

**File**: `crates/world/src/navgraph.rs`, `crates/world/src/mapcache.rs`

**What to do**: In the flat-edge branch (`dz.abs() <= STEP`, hull trace passes), also require
`segment_has_floor(cm, a, b)`. In `walkable_stair`, reject any floor-existence probe whose
hit is lava/slime-covered (`floor_is_deadly`). Bump cache VERSION 21 → 22. Extend
`world/tests/lava_q2dm3.rs`: generate the q2dm3 graph and assert NO edge's straight segment
fails `segment_has_floor` (spot-check: no edge midpoint hangs over lava).

**Commit**: `task(P50-T1): floor-validate flat walk edges; lava-aware stairs (cache v22)`

### T2: `escape_from_lava` survival override

**File**: `crates/brain/src/hazard.rs`, `brains/main.rs`, `brains/q3/mod.rs`, `brains/zb2.rs`

**What to do**: `pub fn escape_from_lava(cm, pos) -> Option<Vec3>` — returns the world
direction to the nearest safe standable floor when the bot's feet/origin are in
lava/slime: fan 16 yaws × marching samples (32..192 u), floor search in a ±64 u band,
floor must be non-deadly with headroom; pick the closest. Each brain applies it as the
LAST movement override (after combat/dodge): face the escape dir, full forward, `jump()`
every tick (clears pool rims). Survival outranks aim. Pak-gated test: from a q2dm3 lava
surface point, the function returns a direction whose march reaches safe floor.

**Commit**: `task(P50-T2): in-lava escape override for all brains`

### T3: Post-fix soak + docs

Same soak command (305 s, `--count 3 --brains main,q3,zb2`); compare burn frames
(`damage=3/6/9` counts), sustained-burn sequences, per-group deaths vs the baseline.
Record in brain_notes; close plan (and Plan 49's T2 rides along — pain-response engagement
counters from the same log).

**Commit**: `docs(P50): soak comparison; close plan`

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/world/src/navgraph.rs` | Flat-edge floor validation, stair deadly check | P0 |
| `crates/brain/src/hazard.rs` | `escape_from_lava` | P0 |
| `crates/brain/src/brains/{main.rs,q3/mod.rs,zb2.rs}` | Survival override wiring | P0 |
| `crates/world/src/mapcache.rs` | VERSION 22 | P0 |

## Open Questions / Risks

1. **Edge validation could disconnect intended crossings** — q2dm3 has no legitimate
   walk-through-lava route (items in lava are jump-in/jump-out human plays we don't model).
   Connectivity gate: the existing `check_spawn_connectivity` + ride/spawn tests must stay
   green on all 8 maps' caches.
2. **Build-time cost** — `segment_has_floor` per flat edge ≈ a few point traces per 16 u;
   rayon-parallel edge pass already exists. Accept a one-time regen.
3. **Escape override vs traversal legs** — gate the override on NOT `gates.any()`? No: if a
   ride/swim state misfires while in lava, survival still wins. Apply unconditionally.

## Verification Checklist

- [ ] T1: q2dm3 cache regenerates (v22); no-edge-over-lava assertion green; all 8 maps' connectivity tests still pass
- [ ] T2: pak-gated escape test green; workspace clippy/tests green
- [ ] T3: post-fix soak shows burn frames and sustained burns cut vs baseline (recorded in brain_notes); plans 49+50 closed
