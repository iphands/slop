# Plan 32 — Underwater breath: dive with a plan, surface to breathe

> **Status**: pending (file authored 2026-07-09; SERIES row existed since Plan 23 split)
> **Created**: 2026-07-09
> **Depends on**: Plan 40 (swim movement), Plan 46 (shared traversal executor)
> **Goal**: Bots manage air like humans — track time submerged, surface before drowning, don't loiter or fight pointlessly underwater, and dive for deep items only with the air (and health) to make it back.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Swimming works (Plan 40) but bots have **no breath model at all** — a bot will
sit submerged indefinitely, taking drowning damage every 2s once its 12s of air runs out
(Q2 rules: `vendor/yquake2/src/game/p_client.c` `P_WorldEffects` — air = 12s, then
escalating drown damage; air resets on surfacing). Add a client-side air clock, a
surface-seek override, and air-budget gating for underwater goals.

**Deliverables**:
1. `brain::water` air clock: submerged time tracked from `water_level == 3` (eyes under —
   already computed client-side from `cm` + origin, `water.rs:28`), reset on `< 3`;
   mirrors the server's 12s rule with a safety margin.
2. Surface-seek override in the traversal executor: when air remaining < the estimated
   time-to-surface + margin, abandon the current underwater leg and swim up/to the nearest
   surfaceable point (nav-graph swim nodes know their z; the water surface z is known from
   the water brush).
3. Dive gating: an underwater goal (swim-edge path) is only accepted if the round-trip
   submerged time fits the air budget; drowning-damage state also triggers Plan 30's
   health-seek after surfacing.
4. Recorder visibility + live proof on q2dm1's railgun water route.

**Estimated effort**: Small–Medium (half day).

## Context

- `water.rs` computes `water_level` 0–3 client-side; `is_swimming = level >= 2`; swim
  intent + water-exit exist (`main.rs:546-573`, moving into the Plan 46 executor).
- Q2 server truth (`P_WorldEffects`): `air_finished = level.time + 12` while
  `waterlevel < 3`; under at `waterlevel == 3` past that → drown damage 2/s escalating.
  We can't read `air_finished` off the wire — but we compute `water_level` ourselves, so a
  client-side clock synced to our own submersion observation is accurate to a tick.
- Estimated time-to-surface: vertical distance to the water-surface z at ~300 u/s swim
  speed (measure actual sustained vertical swim speed live and pin the constant with a
  comment; Plan 40 logs have the data — z 238→434 in ~46 frames).
- The q2dm1 railgun tunnel is comfortably inside 12s, so today's routes rarely drown —
  the behavior matters for longer loiters (fights near water, recovery failures) and for
  looking human (bob up for air between dives).

## Step-by-Step Tasks

### T1: Air clock (pure)

**File**: `crates/brain/src/water.rs`

**What to do**: `AirClock { submerged_since: Option<f32> }` with
`tick(level3: bool, now) -> AirState { remaining_secs, drowning }` (12s budget, 2s safety
margin constant). Unit tests: submerge → deplete → surface resets; drowning flag past
zero.

### T2: Surface-seek override

**File**: `crates/brain/src/traverse.rs` (Plan 46 executor)

**What to do**: While swimming with `remaining < time_to_surface(pos) + margin`: override
the swim target to straight-up / nearest surface point (prefer the current swim path's
next above-surface node if closer), full `up` thrust; suppress goal/combat steering until
`water_level < 3`. Emit a recorder marker (reuse `S` + a `drowning` field or a `B`reath
flag — coordinate with the recorder schema doc in `recorder.rs`).

### T3: Dive gating + post-surface heal hook

**Files**: `crates/brain/src/brains/main.rs` (goal selection), `crates/brain/src/items.rs`

**What to do**: When a selected goal's path includes swim edges, estimate submerged
time (sum of submerged edge lengths / measured swim speed); reject/defer the goal if
> air budget (unless the path surfaces midway). If drowning damage was taken, raise the
health-hunger multiplier (Plan 30's picker) for the next ~10s.

### T4: Live proof + notes

**What to do**: q2dm1 `spawn-to-weapon railgun` still reaches (no regression, air never
critical). Then a forced-loiter test: pin a `spawn-to-point` goal *under* water past 12s —
the bot must surface, breathe, and re-dive rather than drown (health stable in logs).
Append `context/brain_notes.md` (dated).

> **Rule B reminder**: commit after *each* task; fmt + clippy(-D warnings) + tests green.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/water.rs` | `AirClock` | P0 |
| `crates/brain/src/traverse.rs` | surface-seek override | P0 |
| `crates/brain/src/brains/main.rs`, `items.rs` | dive gating, heal hook | P1 |
| `crates/brain/src/recorder.rs` | drowning/breath visibility | P2 |

## Open Questions / Risks

1. **Clock drift vs server** (we start counting on *observed* level-3, the server on its
   own). *Mitigation*: 2s safety margin; drowning damage observed via health drop corrects
   us (clamp remaining to 0 on unexplained underwater health loss).
2. **Surface point may be ceiling-blocked** (underwater tunnels). *Mitigation*: prefer
   path-aware surfacing (next above-water node) over raw straight-up; the swim graph knows
   which nodes are submerged (Plan 39).
3. **Override fighting the ride/ladder machines.** *Mitigation*: it lives inside the same
   executor with a defined priority: drowning-surface > ride > ladder > swim-normal.

## Verification Checklist

- [ ] T1: `AirClock` unit tests pass; commit.
- [ ] T2: forced-loiter test surfaces before damage (log: health flat, `S` frames broken by
      surface intervals); commit.
- [ ] T3: over-budget dive goal deferred (unit test with synthetic path); post-drown heal
      hunger active; commit.
- [ ] T4: q2dm1 railgun swim unchanged; notes appended; commit.
- [ ] fmt + clippy(-D warnings) + tests green before each commit.
