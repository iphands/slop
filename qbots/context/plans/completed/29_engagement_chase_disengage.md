# Plan 29 — Engagement decisions: chase for the kill, disengage, third-party breaks

> **Status**: pending (file authored 2026-07-09; SERIES row existed since Plan 23 split)
> **Created**: 2026-07-09
> **Depends on**: Plan 28 (matchup reads), Plan 27 (persona)
> **Goal**: `main` bots pick and finish fights like humans — commit to a chase when they're winning (pursue through doorways via the nav graph, not just stare at the last-seen point), break off when the fight turns, and disengage a 1v1 when a third party opens fire.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: The FSM already remembers a lost enemy (`fsm.rs:17` `Hunt { last_enemy_pos }`;
q3 has `BattleChase` with a 10s deadline). But `main`'s Hunt just walks at the last-seen
point and gives up — it doesn't *pursue* (extrapolate through the doorway, use the nav
graph), doesn't weigh *whether* to chase (winning? healthy? armed?), and nothing detects a
third-party attacker. This plan makes engagement decisions explicit and persona-driven.

**Deliverables**:
1. Chase commitment: on LOS loss while winning, path (A*) to the enemy's last position
   **extrapolated along their velocity**, with a persona/matchup-gated time budget.
2. Chase-or-not gate: health, held weapon (matchup from Plan 28), and persona aggression
   decide chase vs re-arm/heal (ties into Plan 30 goals).
3. Third-party break: taking damage while engaged with someone who isn't the shooter →
   break the 1v1 (reposition/flee), matching the SERIES "make them choose" intent.
4. Live verification: chase sequences visible in logs; K/D and kills-per-5-min vs baseline.

**Estimated effort**: Medium (1 day).

## Context

### What exists (surveyed 2026-07-09)
- `main` FSM (`crates/brain/src/fsm.rs`): `Roam → Hunt{last_enemy_pos} → Engage → Flee`;
  Engage→Hunt stores last position (:133); 2-frame sight grace in `combat.rs:18-24`;
  stale-target fire forbidden after grace (Plan 11 honesty).
- q3: `BattleChase` (10s deadline, `q3/mod.rs:117,352`), `BattleRetreat`
  (aggression-gated). q3 is the reference for "chase texture"; leave q3 itself untouched
  (it's the control brain).
- Plan 45 shipped `main`'s weapon-rush + kite/flee gates — the disengage half exists in
  embryo; the *chase* half doesn't (Plan 45 tuned defense, and found kills come from
  pressure).
- Wire facts: enemy health is NOT visible. "Winning" must be inferred client-side: we
  recently hit them (aim landed / their pain sound `svc_sound`), we have the matchup edge
  (Plan 28), we're healthy. Own damage IS visible (health stat drops); the *direction* of
  incoming fire is not explicit — infer the shooter by visible muzzle-flash entities
  (`svc_muzzleflash`), or "damage while my target hasn't fired/isn't facing me".

### Design sketch
Extend `Hunt` into a real pursuit: goal = `last_enemy_pos + enemy_vel * k` snapped to the
nav graph; budget = `chase_secs(persona, matchup, my_health)`; abort → best of (item goal,
roam). Add `ThirdParty { attacker_hint }` handling as a Flee-variant trigger, not a new
FSM state, to keep the state machine small.

## Step-by-Step Tasks

### T1: "Winning/losing" estimator (pure)

**File**: `crates/brain/src/combat.rs` (or new `engage.rs`)

**What to do**: Track per-target: our recent landed-fire heuristic (target visible + our
shots aligned within jitter → count as pressure; use pain sounds when available), our
health trend over the engagement, matchup score (Plan 28). Output
`EngageRead { pressure: f32, losing: bool }`. Pure + unit-tested with synthetic histories.

### T2: Real pursuit in `Hunt`

**Files**: `crates/brain/src/fsm.rs`, `crates/brain/src/brains/main.rs`

**What to do**: On Engage→Hunt, seed the pursuit goal with velocity extrapolation and path
to it via the navigator (today's Hunt target is raw last-seen). Chase budget from persona ×
`EngageRead` (winning + aggressive → up to ~8s like q3's 10; losing/low-health → 0, go to
Plan 30's heal/re-arm goal instead). Re-acquire on LOS (existing Engage transition). On
budget expiry, drop cleanly to goal selection (no lingering).

### T3: Third-party break

**Files**: `crates/brain/src/brains/main.rs`, `crates/brain/src/perception.rs` (hints)

**What to do**: Detect "damaged by someone else": health dropped while (a) current target
lost LOS or hasn't been firing toward us, or (b) a different visible player's muzzle-flash
aligned with us. On detection during Engage: break off — pick a flee/reposition goal that
leaves BOTH attackers' lines (reuse flee movement), persona-scaled (a berserker persona may
instead switch targets to the nearer threat). Log the event distinctly (`third_party`).

### T4: Live verification + notes

**What to do**: 2× 5-min `competition --count 4 --brains q3,main` + a 3-brawler free-for-all
(`--count 6`). Expect: kills ≥ baseline (chases convert), deaths ≤ baseline (third-party
breaks). Grep logs for `Hunt` sequences ending in re-acquire + kill (the "chased for the
kill" proof). Append `context/brain_notes.md` (dated).

> **Rule B reminder**: commit after *each* task; fmt + clippy(-D warnings) + tests green.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/fsm.rs` | pursuit-capable Hunt | P0 |
| `crates/brain/src/brains/main.rs` | chase gate, third-party break, goal handoffs | P0 |
| `crates/brain/src/combat.rs` / `engage.rs` | `EngageRead` estimator | P0 |
| `crates/brain/src/perception.rs` | muzzle-flash / pain-sound hints | P1 |

## Open Questions / Risks

1. **Chasing into ambushes / feeding** — Plan 45 showed aggression without information cuts
   both ways. *Mitigation*: chase only when `EngageRead` says winning AND matchup ≥ even;
   measure deaths in T4 and stop-loss.
2. **Muzzle-flash attribution is fuzzy.** *Mitigation*: treat as hint; the (a) clause
   (damage while target quiet) needs no attribution at all.
3. **FSM churn** (Hunt↔Engage flapping at doorways). *Mitigation*: keep the existing sight
   grace; add a short (0.5s) re-transition damper.
4. **Enemy pain sounds may not always be transmitted** (PVS/attenuation). *Mitigation*:
   they're a bonus signal only; pressure works without them.

## Verification Checklist

- [ ] T1: `EngageRead` unit tests (winning/losing scenarios) pass; commit.
- [ ] T2: live log shows Hunt → extrapolated pursuit → re-acquire → kill at least once per
      5-min run; no post-budget lingering; commit.
- [ ] T3: third-party events logged and followed by a break (no 3-way standing trades);
      unit test for the detector; commit.
- [ ] T4: kills ≥ baseline AND deaths ≤ baseline across 2 runs, or findings documented;
      `brain_notes.md` appended; commit.
- [ ] fmt + clippy(-D warnings) + tests green before each commit.
