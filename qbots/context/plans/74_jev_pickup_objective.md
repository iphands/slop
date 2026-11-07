# Plan 74 — Jev pickup objective: let the model choose which out-of-sight pickup to go for

> **Status**: in-progress
> **Created**: 2026-11-07
> **Depends on**: Plan 72 (the `jev` brain, hub, `GoalPolicy::objective` seam), Plan 73 (the lab,
> the journal, the arena), Plan 30 (`build_map_items`, `ItemMemory`)
> **Goal**: Give `jev` a decision q3 does not already make — which **out-of-sight** pickup
> (weapon / health / armor) to head for while roaming — and measure, against a code-rule control,
> whether the model's choice gets bots onto a real weapon sooner than q3 and than a fixed rule.
> **Agent**: implementation agent (ralph-loop) for T1–T5, T7; **T6 and T8 are live** (endpoint,
> Q2 server) — agent-run only with the user's explicit in-session permission, otherwise
> human-run; T6's corpus labels need the user's skim **before** any scoring.

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.
> Rule A (zero warnings) and Rule B (commit per task) are not optional here.

---

## TL;DR

**What**: When a `jev` bot is roaming (no enemy in view), code lists the nearest **reachable,
not-known-empty** pad of each pickup class from the BSP item table — which q3 throws away — and
the model picks one (or `carry_on`); the bot walks there. A new `jvr` arm picks from the **same**
candidates by a fixed code rule, so the arena can tell "the model chose well" apart from "knowing
where the pads are helps".

**Deliverables**:
1. `MapItem` tells a weapon pad from an ammo pad (today both are `ItemWeapon`).
2. Per-bot pad candidates (A* distance, `ItemMemory` availability, relevance) and an
   objective-only `pads` state line — posture requests stay byte-identical.
3. Objective question v2 (`seek_weapon / seek_health / seek_armor / carry_on`, only offered
   options with a candidate), roaming triggers, the apply/hold/release path.
4. `jvr` brain kind (rule picker, model answer journalled as shadow).
5. A new labelled corpus, lab rounds, holdout once, `calibrate` → frozen threshold **or** a
   recorded failure (W2 stop).
6. Objective outcome windows + a per-life `secs_to_strong` metric for **every** arm; arena rounds
   (2 q3 + 2 jev + 2 jvr) and the written result.

**Estimated effort**: Large (3 days)

---

## Context

### Why this plan exists

Plan 73's arena conclusion (`context/distilled/jev_lab.md`, *Arena conclusion* point 4): the
posture question mostly agrees with q3's own heuristic, so it cannot move the result much; **the
next lever is a question whose answer q3 does not already compute** — "which pickup to go for"
or "when to re-engage". This plan takes the pickup question.

### Pre-Identified Issue — bots fight with the spawn weapon

Every posture decision in the Plan 73 arena journals, by held weapon (2026-11-07):

```bash
python3 -I - <<'EOF'
import json,glob,collections,re
c=collections.Counter(); n=0
for f in sorted(glob.glob('logs/jev/journal-*.jsonl')):
    for l in open(f):
        try: v=json.loads(l)
        except: continue
        m=re.search(r'held:(\S+)', v.get('state') or '')
        c[m.group(1) if m else 'none']+=1; n+=1
print(n, c.most_common())
EOF
```
```text
1510 [('blaster', 727), ('shotgun', 384), ('unknown', 184), ('super_shotgun', 72), ('rocket_launcher', 43), ('machinegun', 42), ('hyperblaster', 32), ('chaingun', 26)]
```

**76 %** of fight decisions (1 153 / 1 510) were taken holding a *weak* weapon by the state's own
definition (blaster / shotgun / machinegun, `brain::jev::state::weak_weapon`) — 48 % with the
spawn blaster. Plan 73 A1-2: "weak weapon" carried 198 of 232 jev disengages. Bots spend most of
their fights under-armed.

Today's behaviour, by code:
- q3's roam goal is the best **visible** item, else a cycled roam waypoint
  (`crates/brain/src/brains/q3/mod.rs:240-260`, `items::best_item_goal` over `view.items()`).
  q3 never heads for a pad it cannot see.
- `Q3Brain::set_map` discards the BSP item table (`q3/mod.rs:873-879`, `items: _`).
- `JevBrain::set_map` keeps it (`brains/jev/mod.rs:66`, `policy.rs:251-260`) — but
  `JevPolicy::objective` returns `None` (`policy.rs:390-393`): the seam exists and is unused.

### Why a code-rule control (`jvr`), not `jev_static`

Plan 72 risk 3: jev-with-BSP-knowledge vs q3 also measures "who read the BSP". If jev beats q3
on time-to-weapon, the cheapest explanation is "knowing where the pads are helps", which a
three-line rule also gets. The honest question for the **model** is whether it beats a fixed rule
**on the same candidates**. So `jvr` = `jev` with the objective chosen by a code rule (the
model's objective answer still asked and journalled, never applied); posture is identical in
both. Rejected: `jev_static` as the control — it reads no pads, so it re-creates the confound.

### Why class labels, not candidate slots

Lab rule R4-2 (`jev_lab.md`): short literal option descriptions; meaningless labels measure flat
(a 24-option choice over slot labels gave top p 0.051 — `context/distilled/jev.md`). So the
labels are the classes (`seek_weapon`, …) and code resolves each to that bot's best pad of the
class. Only classes with a candidate are offered (≤ 4 options with `carry_on`; Round 3 measured
a 4-option objective with `carry_on`: RC 75 %, CW 17 % — all of it the `hold_position`/`carry_on`
merge this plan removes).

### Why objective-only state

Posture's 0.30 threshold was calibrated on schema v2 renders. A new `pads` line in the **posture**
request would void that calibration. Posture and objective are separate requests, so the `pads`
line is rendered into objective requests only, and the posture goldens must stay byte-identical
(W1).

### Key Facts (checked 2026-11-07 unless marked)

| Fact | How confirmed |
|---|---|
| `build_map_items` classifies `weapon_*` **and** `ammo_*` as `EntityClass::ItemWeapon` | `crates/brain/src/perception.rs:744` |
| The wire carries no per-weapon inventory — only the **held** weapon (view model) and its ammo | `perception.rs:33-35`; `Weapon::from_view_model` (`weapons.rs:162`) |
| Weapon strength: `Weapon::power_tier` (blaster 0, MG/CG 25, SG/SSG 50, GL 80, HB/RL 90, rail 95, BFG 100) | `weapons.rs:141-155` |
| `ItemMemory { observe, available }` tracks pads seen empty, with Q2 respawn times (armor 20 s, powerup 60 s, else 30 s) | `items.rs:39-47`, `:76-125`; used by `main` and `xon` |
| `NavGraph::path(start, goal)` + `path_len(&path)` give an A* distance | `crates/world/src/navgraph.rs:822`, `:1729` |
| `MapItem { class, origin, nav_node }` — built in `items.rs:30`, plus test constructors in `items.rs:310`, `xon/goals.rs:414`, `jev/mod.rs:136` | grep |
| The hub keeps **one** newest-wins pending ask per bot (any question), **one** pacing clock per bot, and one stored `AppliedDecision { objective, posture }` per bot; a posture-only answer stores `objective: None`, which supersedes | ~~unconfirmed~~ RESOLVED (T1): `hub.rs:176-197`, `:433-470`, `:613-650`; `jev/source.rs:85-99` |
| DM auto-switches on pickup **only** from the blaster, and only on the first copy; otherwise the bot wields a better weapon only when combat asks for it (`select_best_weapon` → `use <name>`) | ~~unconfirmed~~ RESOLVED (T1): `vendor/yquake2/src/game/player/weapon.c:325-330`; `weapons.rs:363`, `qbots/src/main.rs:1815-1816` |
| Arena server `dmflags 17424` = INSTANT_ITEMS + FORCE_RESPAWN + QUAD_DROP — **no** `DF_WEAPONS_STAY`, so a taken weapon pad is empty for 30 s | RESOLVED (T1): connectionless `status` to `noir.lan:27910`; bits from `shared.h:1152-1166`; respawn `weapon.c:310-316` |
| Pads: q2dm1 11 weapon (7 strong), 18 health, 18 armor, **35 ammo**; q2dm3 8 weapon, 15 health, 8 armor, 19 ammo | ~~unconfirmed~~ RESOLVED (T1): BSP entity lumps from `vendor/baseq2/pak0.pak` |

### Rejected Claims

- *"q3 ignores items."* False — it grabs **visible** items (`SeekNbg`/`BattleNbg`, `roam_goal`).
  The gap is out-of-sight pads only.
- *"The objective question already failed, so the model can't do this."* The Plan 72 T10 /
  Plan 73 T4 failure was ambiguous-case leakage on a corpus whose labels included
  `hold_position` and whose state had **no out-of-sight facts**. Different question, different
  state; it gets its own lab and calibration (T6) and a stop rule (W2).

### Guard rails (carried over from Plan 73)

Labels committed before any answer is scored; tuning half for choosing, holdout **once**;
threshold only from `calibrate` on the holdout; one change per arena round with the prediction
written first; decisions/lives before frags (≥ 100 per comparison, else LOW POWER); the shadow
answer never steers.

---

## Step-by-Step Tasks

### Wave A — offline (T1–T5)

### T1: Confirm the open facts, record them

**File**: `context/plans/74_jev_pickup_objective_tracker.md` (Key Facts → Evidence)

**What to do**: Resolve the three unconfirmed Key Facts, citing source lines: (1) how the hub's
stored decision and `DecisionSource::latest` carry answers — can a posture ask clobber a pending
objective ask, and does a stored decision hold one question's answer or several; (2) yquake2's
DM weapon-pickup auto-switch rule; (3) per-class pad counts on q2dm1 and q2dm3 (a throwaway
`#[test]` or `jev-probe` output is fine — no `tmp/` scripts, nothing committed but the numbers).
If (1) shows clobbering, T4 gains a sub-step (separate pending per question key); write that into
T4 here.

**Verify**: each fact has a source line or a command + output in the tracker.
**Expected observation**: per-question answers are keyed by `QuestionKey` in the decision; DM
auto-switches only from the blaster; q2dm1 has ≥ 4 weapon pads.
**Commit**: `task(T1): confirm hub question keying, DM autoswitch, pad counts`

### T2: Tell weapon pads from ammo pads

**File**: `crates/brain/src/brains/core.rs` (`MapItem`), `crates/brain/src/items.rs`
(`build_map_items`), `crates/brain/src/weapons.rs` (new `Weapon::from_classname`), the three
test constructors listed in Key Facts.

**What to do**: Add `pub weapon: Option<Weapon>` to `MapItem` — `Some(w)` for `weapon_<name>`
classnames, `None` for ammo and everything else. `from_classname` maps yquake2's classnames
(`weapon_shotgun`, `weapon_supershotgun`, `weapon_machinegun`, `weapon_chaingun`,
`weapon_grenadelauncher`, `weapon_rocketlauncher`, `weapon_hyperblaster`, `weapon_railgun`,
`weapon_bfg`); cite `g_items.c`. Classes are unchanged, so every existing consumer behaves
identically.

**Before** (`core.rs:29-36`):
```rust
pub struct MapItem {
    pub class: EntityClass,
    pub origin: Vec3,
    pub nav_node: Option<usize>,
}
```
**After**:
```rust
pub struct MapItem {
    pub class: EntityClass,
    /// `Some` for a `weapon_*` pad (which weapon); `None` for ammo and non-weapons.
    pub weapon: Option<Weapon>,
    pub origin: Vec3,
    pub nav_node: Option<usize>,
}
```

**Verify**: `cargo test -p brain -- from_classname map_item` — every `weapon_*` maps,
`ammo_rockets` → `None`, unknown → `None`; `q3_parity_golden` unchanged; `just all`.
**Commit**: `task(T2): MapItem knows which weapon a pad holds (ammo is not a weapon)`

### T3: Pad candidates + objective-only `pads` state line

**File**: `crates/brain/src/brains/jev/policy.rs`, `crates/brain/src/jev/state.rs`,
`crates/qbots/src/jev/render.rs`, render goldens under `crates/qbots/src/jev/testdata/`

**What to do**:
- `JevPolicy` owns an `ItemMemory`, `observe`d every `pre_tick` (cheap; already used per tick by
  `main`).
- `candidates(view) -> PadCandidates` **at submit time only**: per class (weapon / health /
  armor), the pad with the shortest A* distance (`path` + `path_len` from the bot's nearest node
  to `MapItem::nav_node`) that is `available`, reachable, and **relevant**: weapon pad only if
  its `power_tier` > the held weapon's; health only below 100 hp; armor only below 100 armor.
  Skip pads within 128 u that are visible (q3 already grabs those).
- New `FieldKey::Pads` / `Field::Pads(PadFacts { weapon, health, armor: Option<DistBand> })`,
  rendered `pads  weapon:<band|none> health:<band|none> armor:<band|none>`. Bump
  `STATE_SCHEMA_VERSION` to 3. The renderer includes `Pads` **only** in objective requests.

**Verify**: `cargo test -p brain -- jev_pads` — a scripted map with a rocket pad behind a wall
(not in `view.items()`) yields a `weapon:mid` candidate; a pad seen empty is skipped until its
respawn time; a railgun-holder gets no weapon candidate for a shotgun pad.
`cargo test -p qbots -- jev_render` — **the posture goldens are byte-identical** (no `pads`);
a new objective golden includes the `pads` line; the injection golden still byte-identical.
**Commit**: `task(T3): per-bot pad candidates and an objective-only pads state line`

### T4: Objective question v2, roaming triggers, apply/hold/release

**File**: `crates/brain/src/jev/question.rs`, `crates/brain/src/brains/jev/policy.rs`,
`crates/qbots/src/jev/hub.rs` (+ whatever T1 adds)

**What to do**:
- `objective_question_v2(&PadCandidates)`: labels `seek_weapon` ("go to the nearest better
  weapon"), `seek_health` ("go to the nearest health pickup"), `seek_armor` ("go to the nearest
  armor pickup"), `carry_on` ("no particular objective: keep roaming as usual"); a class with no
  candidate is **not offered**; with no candidates at all, do not ask. Wording is provisional —
  T6's lab decides it.
- Triggers, all with **no enemy in view**: respawn (+2 s), release of the previous objective,
  `refresh_due()`. Subject to the hub's existing pacing.
- Store `label → NavGoal::Position(pad origin)` in the ticket ring under the `TicketId`; resolve
  an answer only against its own ticket (Plan 72 T10's rule). `GoalPolicy::objective` returns the
  applied goal; release on arrival (≤ 64 u horizontally), `on_stall`, death, level change, a
  newer answered ticket, the pad seen empty, or 30 s.
- Gate: margin threshold `jev.gate_margin_multi` (provisional until W2).
- *(added by T1)* **Pacing per question key** in the hub: objective asks get their own pacing
  clock, so a roaming objective ask can never pace out the contact posture ask that follows it
  (today one clock, `hub.rs:433-450`). The pending slot stays newest-wins — a clobbered objective
  ask is simply re-asked at the next trigger.
- *(added by T1)* The brain releases an applied objective only on a newer ticket that **asked**
  the objective; a posture-only ticket's `objective: None` does not release it.

**Verify**: `cargo test -p brain -- jev_objective` — a `StubDecider` naming `seek_weapon` drives
the navigator to the rocket pad; an off-ticket label ⇒ `Rejected(OffTicketLabel)` and pure q3;
an older ticket resolves against **its** map; the goal holds across ticks and releases on each
release condition; no ask while an enemy is in view; `jev_static` unaffected.
`cargo test -p qbots -- jev_objective_render` — offered options match candidates; total request
≤ the token budget.
**Commit**: `task(T4): objective question v2 — pad candidates, roaming asks, held until released`

### T5: `jvr` — the code-rule control arm

**File**: `crates/brain/src/brains/jev/{mod,policy}.rs`, `crates/brain/src/brains/mod.rs`
(`BrainKind::JevRule`, CLI `jvr`), `crates/qbots/src/jev/hub.rs` (arm label `jev_rule`)

**What to do**: `ObjectivePicker::{Model, Rule}`. Rule, from the same candidates: weak held weapon
and a weapon candidate → `seek_weapon`; else health band lo/crit and a health candidate →
`seek_health`; else armor < 50 and an armor candidate → `seek_armor`; else `carry_on`. The model is
still asked; its objective answer is journalled with `shadow: true` and never applied. Posture is
identical to `jev`'s (live).

**Verify**: `cargo test -p brain -- jev_rule` — rule table cases; a model answer that disagrees
is journalled, not applied; `build_brain_with_decider(JevRule, …)` builds.
**Commit**: `task(T5): jvr arm — same pad candidates, objective picked by a code rule`

**W1 gate (end of Wave A)**: `just all` green; `q3_parity_golden` and posture render goldens
unchanged since Plan 72's close.

### Wave B — live (T6–T9)

### T6: Corpus, lab rounds, holdout once, calibrate (live endpoint)

**File**: `context/fixtures/jev/calibration/objective_v2/{obvious/<label>,ambiguous,flat}/*.txt`,
`crates/qbots/src/jev/probe.rs` (new `ProbeQuestion::ObjectiveV2`, corpus dir `objective_v2`),
`crates/qbots/src/jev/lab.rs` (variants for v2), `context/distilled/jev_lab.md`, tracker Rounds

**What to do**: Write ≥ 10 obvious per label, ≥ 20 ambiguous, ≥ 6 flat, in schema v3 objective
renders (with the `pads` line); half of them seeded from the live mix (blaster/shotgun, healthy,
no enemy). v2's options depend on the candidates, so the probe derives each case's offered
options from that case's `pads` line (a class at `none` is not offered) — the lab and the bot
must build the question through the same function. **Commit the corpus and get the user's skim before the first scoring call.** Then lab
rounds on the tuning half (one change per variant, prediction written first), the winner on the
holdout **once**, `calibrate` on the holdout.

**Verify**: `cargo test -p qbots -- jev_probe_objective_v2` (offered options follow the `pads`
line); `cargo run -p qbots -- jev-probe lab --question objective-v2 …` (tuning half, then the
holdout once) and `cargo run -p qbots -- jev-probe calibrate` (default `--dir context/fixtures/jev/calibration`)
outputs recorded in the tracker.
**Expected observation**: obvious `seek_weapon` (blaster, weapon pad near, healthy) is the
easiest — RC ≥ 90 %; leakage concentrates in "hurt + weak weapon" ambiguous states.
**W2 gate (stop rule)**: a holdout threshold exists with CW = 0 and leakA ≤ 20 %. If not, record
the failure in `jev_lab.md`, run T8 with `jvr` vs q3 only (pads knowledge, no model), and close
the model half as a negative result — do **not** loosen the threshold.
**Commit**: corpus `task(T6): objective v2 corpus (labels before scoring)`; then one commit per
lab round; then `task(T6): objective v2 holdout + calibrate — <threshold|failed>`

### T7: Objective outcomes + `secs_to_strong` for every arm

**File**: `crates/brain/src/jev/source.rs` (`Outcome`), `brains/jev/policy.rs`,
`crates/qbots/src/main.rs` (`bot_task`: per-life EVT), `crates/qbots/src/jev/journal.rs`

**What to do**: *(added by T5)* journal `jvr`'s **rule pick** next to the model's answer, so
rule/model agreement (P2) is computable. Objective windows (30 s, from apply): `arrived`, `secs_to_arrive`, `upgraded`
(held weapon tier rose), hp/armor gained, died, frags. A per-life `EVT life` line from `bot_task`
for **every** brain: `secs_to_strong` (spawn → first non-weak held weapon, or none — *held*, not
owned: past the blaster, DM does not auto-switch, so a bot wields a picked-up weapon only once
combat asks for it; T1), life length,
frags. `jev-probe journal` adds an objective section (by arm × answer × agrees-with-rule) and a
per-arm `secs_to_strong` summary (median, share of lives that ever got a strong weapon).

**Verify**: `cargo test -p qbots -- jev_journal` — fixture lines produce the new sections;
`cargo test -p brain -- jev_objective_outcome` — window closes on arrival/death/30 s.
**Commit**: `task(T7): objective outcome windows and per-life secs_to_strong for every arm`

### T8: Arena rounds (live Q2 server)

**File**: `context/fixtures/jev/arena_74.yaml` (2 q3 + 2 jev + 2 jvr, navmode `as`), tracker
Rounds, `context/fixtures/jev/arena/b*_report.txt`

**What to do**: Pre-register each round's predictions in the tracker and commit them **before**
running. Baseline round B1: 3 × 10 min on parked q2dm1. Predictions to pre-register (edit only
before B1 runs):
- P1: `jvr` median `secs_to_strong` ≤ 0.7 × q3's.
- P2: `jev` within ±15 % of `jvr` on `secs_to_strong`; model and rule agree on ≥ 80 % of applied
  objective answers.
- P3: weak-weapon share of posture decisions falls below 60 % for `jev` and `jvr` (76 % in Plan 73).

Further rounds change **one** thing each (wording, a relevance rule, the threshold only via
`calibrate`). Stop when a round teaches nothing new or after 4 rounds.

**Verify**: `jev-probe journal` reports per round committed; ≥ 100 lives per arm per comparison
or flagged LOW POWER.
**Commit**: `docs(74): pre-register round BN` before each round; `task(T8): arena round BN — <finding>` after.

### T9: Write it down, close

**File**: `context/distilled/jev_lab.md` (Plan 74 section), `docs/BRAINS.md` (`jvr` row),
`context/brain_notes.md` (dated entry), `context/pitfalls.md` (if earned), `SERIES.md`

**What to do**: The answer to "does the model choose pickups better than a rule?" in one
paragraph with numbers; what wording/facts worked; follow-ups (re-engage timing, region layer).

**Verify**: re-read each file before the commit message names it.
**Commit**: `task(T9): Plan 74 findings, BRAINS.md jvr, brain notes`; then the close via
`/hands-plan close 74`.

---

## Dependency matrix

| Task | Needs |
|---|---|
| T1 | — |
| T2 | — |
| T3 | T2 |
| T4 | T1, T3 |
| T5 | T4 |
| T6 | T4 (renders); live endpoint permission |
| T7 | T4 |
| T8 | T5, T6 (W2), T7; live server permission |
| T9 | T8 |

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/brains/core.rs` | `MapItem.weapon` | P0 |
| `crates/brain/src/items.rs` | `build_map_items` fills `weapon` | P0 |
| `crates/brain/src/weapons.rs` | `Weapon::from_classname` | P0 |
| `crates/brain/src/brains/jev/policy.rs` | `ItemMemory`, candidates, objective apply/release, rule picker | P0 |
| `crates/brain/src/brains/jev/mod.rs` | `jvr` construction | P1 |
| `crates/brain/src/brains/mod.rs` | `BrainKind::JevRule` (`jvr`) | P1 |
| `crates/brain/src/jev/state.rs` | `FieldKey::Pads`, schema v3 | P0 |
| `crates/brain/src/jev/question.rs` | `objective_question_v2` | P0 |
| `crates/brain/src/jev/source.rs` | objective `Outcome` fields | P1 |
| `crates/qbots/src/jev/render.rs` | `pads` in objective requests only | P0 |
| `crates/qbots/src/jev/hub.rs` | per-question pending (if T1 says so), `jev_rule` arm | P1 |
| `crates/qbots/src/jev/journal.rs` | objective + `secs_to_strong` sections | P1 |
| `crates/qbots/src/jev/lab.rs` | objective v2 variants | P1 |
| `crates/qbots/src/main.rs` | per-life `EVT life` | P1 |
| `context/fixtures/jev/calibration/objective_v2/**` (new) | corpus | P0 |
| `context/fixtures/jev/arena_74.yaml` (new) | arena roster | P1 |
| `context/distilled/jev_lab.md` | findings | P1 |
| `docs/BRAINS.md` | `jvr` row | P2 |

---

## Open Questions / Risks

1. **The rule may be all there is.** "Holding a blaster → go get a weapon" is nearly the whole
   decision. If `jev ≈ jvr`, the honest result is "the model adds nothing over the rule here".
   *Mitigation*: that is an acceptable, pre-registered outcome (P2), not a failure to explain
   away.
2. **Stale pads.** Absence beyond `ITEM_TRUST_RANGE` (500 u) is uninformative, so a far pad may
   already be taken. *Mitigation*: `ItemMemory`, release when the pad is seen empty, the 30 s cap;
   `arrived && !upgraded` in the journal measures the waste.
3. **Unknown inventory.** We see only the held weapon; a bot may walk to a railgun pad while
   holding a shotgun but already owning a railgun. *Mitigation*: relevance compares against the
   held weapon only; T1 confirms the auto-switch rule; the journal's `upgraded` shows how often it
   happens.
4. **Hub slot clobbering** between posture and objective asks — T1 decides; T4 fixes if needed.
5. **Herding.** A deterministic model sends same-state bots the same way. *Mitigation*: candidates
   are per-bot nearest pads, so two bots in different places get different goals; the journal's
   per-pad goal counts show herding if it happens.
6. **Live states may again be less decisive than the corpus** (Plan 73: 24–47 % under margin).
   *Mitigation*: half the corpus is seeded from the live mix; the journal reports the live
   gate-reject rate for the objective separately.
7. **Re-engage timing** — the other half of the Plan 73 follow-up — is **deferred — it overlaps
   posture (asked only with an enemy in view, where the model ≈ q3) and its competitor is q3's
   fixed 4 s give-up; the pickup gap is the larger measured one (76 % weak-weapon fights).**
8. **Effort**: Large; T6 and T8 are open-ended. *Mitigation*: W2 is a hard stop; T8 caps at 4
   rounds.

---

## Verification Checklist

- [x] T1: hub question keying, DM auto-switch rule, q2dm1/q2dm3 pad counts recorded with sources
- [x] T2: `weapon_*` → `Some(Weapon)`, `ammo_*` → `None`; `q3_parity_golden` unchanged
- [x] T3: out-of-sight pad becomes a candidate; empty pad skipped until respawn; posture goldens byte-identical; objective golden has `pads`
- [x] T4: stub `seek_weapon` drives to the pad; off-ticket rejected; held until each release condition; no ask with an enemy in view
- [x] T5: `jvr` applies the rule, journals the model's answer as shadow, never applies it
- [x] W1: `just all` green; parity + posture goldens unchanged
- [ ] T6: corpus committed and skimmed before scoring; holdout once; threshold frozen **or** failure recorded (W2)
- [x] T7: objective windows + per-life `secs_to_strong` for every arm; journal sections tested
- [ ] T8: predictions committed before each round; ≥ 100 lives per arm or LOW POWER; P1–P3 answered
- [ ] T9: `jev_lab.md`, `BRAINS.md`, `brain_notes.md` updated and re-read
- [ ] Rule A held at every commit (`just all` chained before `git commit`)
- [ ] Rule C: plan + tracker `git mv` to `completed/`, `SERIES.md` marked done
