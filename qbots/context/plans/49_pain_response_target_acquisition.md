# Plan 49 — Pain Response: Shot Bots Acquire Attackers Behind Them

> **Status**: in-progress
> **Created**: 2026-07-10
> **Depends on**: Plan 48
> **Goal**: A bot taking damage from outside its view cone turns and engages the attacker instead of ignoring it.
> **Agent**: implementation agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Widen the shared `CombatDriver`'s fresh-target acquisition FOV to the full sphere for a few
seconds after the bot takes damage, so main/zb2 bots shot from behind acquire and engage the
attacker (q3 already does this in its own enemy selection).

**Deliverables**:
1. Pain-aware acquisition in `CombatDriver` (self-tracked health, no caller changes) + unit test.
2. Live q2dm3 soak comparison (post-Plan-48 baseline vs post-fix) via EVT/behavior counters.

**Estimated effort**: Small (2 h)

---

## Context

### Pre-Identified Bug

Live q2dm3 soaks (post-Plan-48) still show bots "running into walls when shot and never
engaging". Root cause verified in code:

- Fresh target selection in the shared driver is FOV-gated at 90°:
  `combat.rs:277-280` → `view.nearest_visible_enemy(cm, 90.0)`. The FOV test is a half-angle
  cone (`forward · dir > cos(fov)`, `perception.rs:344-351`), so 90° = the front hemisphere.
- **Nothing widens acquisition when the bot takes damage.** `main` computes `took_damage` but
  only feeds it to engage/third-party/drown logic; `zb2` doesn't track health at all.
- The attacker IS in the entity list — PVS is positional, not facing-based, and an attacker
  with a shot on us almost always has mutual LOS — so the 90° cone is the *only* gate
  rejecting them.
- `q3` is immune: its own `select_enemy` widens `awareness_fov` when `took_damage`
  (`q3/mod.rs:477`), which is why the symptom clusters on main/zb2.

Result: a main/zb2 bot shot from behind keeps route-running (into a wall, if its route is
bad) while being farmed, and never returns fire — the exact reported symptom.

### Why fix inside `CombatDriver`

The driver sees the `Worldview` every tick, so it can track `last_health` itself and detect
pain without any caller signature changes. Every CombatDriver consumer (main, zb2, future
brains) gets the behavior for free; q3 keeps its own equivalent path.

## Step-by-Step Tasks

### T1: Pain-widened acquisition in `CombatDriver`

**File**: `crates/brain/src/combat.rs`

**What to do**: Add `last_health: Option<i32>` and `pain_frames: u32` to `CombatDriver`.
In `evaluate`, before selection: health dropped (and `> 0`) → `pain_frames = PAIN_AWARENESS_FRAMES`
(30 ≈ 3 s at 10 Hz); else decrement. Fresh selection FOV becomes
`if pain_frames > 0 { 180.0 } else { 90.0 }` (180° half-angle = full sphere). Clear both in
`on_respawn`. Unit test: enemy directly behind + health drop → target acquired; without the
drop → not acquired.

**Commit**: `task(P49-T1): pain-widened target acquisition in CombatDriver`

### T2: Live verification + docs

Post-fix q2dm3 soak vs the post-Plan-48 baseline soak (same command, 305 s,
`competition --count 3 --brains main,q3,zb2 --navmodes astar`): compare `EVT` behavior
counters (`shooting at player` / chase events / per-brain deaths) and confirm shot-from-behind
bots return fire. Append brain_notes section; move plan to `completed/`.

**Commit**: `docs(P49): soak verification + close plan`

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/combat.rs` | Pain tracking + widened acquisition FOV | P0 |
| `context/brain_notes.md` | Soak comparison + findings | P1 |

## Open Questions / Risks

1. **False pain from lava/falling damage** — widening FOV briefly when hurt by the world is
   harmless (there is simply no enemy to acquire); no gating needed.
2. **Single-run soak variance** — behavior counters (does a shot bot fire back at all) are the
   gate here, not K/D; the Plan 47 noise-floor rule applies only to K/D conclusions.

## Verification Checklist

- [ ] T1: unit test proves behind-attacker acquisition on pain and rejects it without pain; workspace clippy/tests green
- [ ] T2: soak shows shot bots engaging (EVT counters recorded in brain_notes); plan moved to `completed/`
