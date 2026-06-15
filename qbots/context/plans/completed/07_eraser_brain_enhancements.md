# Plan 07 — Eraser-Derived Brain Enhancements (combat / danger / skill)

> **Status**: done
> **Created**: 2026-06-14
> **Depends on**: Plan 06 (brain skeleton — perceive/navigate/fight/FSM)
> **Goal**: Port Eraser v1.01's battle-tested combat aim/lead/jitter, weapon selection, projectile danger
> avoidance, and per-bot skill/personality system into `crates/brain` — with Eraser's **exact constants** — so a
> single qbot tracks, hunts, and frags like an Eraser bot despite PVS-limited perception.
> **Agent**: implementation agent (ralph-loop) | sub-agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> **Authoritative reference: `context/distilled/eraser.md`** — every number/formula below is cited there with
> `bot_*.c:line`. Source: `vendor/Quake2BotArchive/extracted/Eraser101_SRC/Eraser/src/`.

---

## TL;DR

**What**: Take the distilled Eraser findings and port the *portable* parts — the combat math, the dodge system,
the skill config, and the FSM watchdog thresholds — into `crates/brain`. This is the "make our bots fight like
Eraser" tuning pass on top of Plan 06's skeleton. Eraser's plugin-only mechanisms (`gi.trace`, full entity table,
enemy health, `Pmove` oracle) are replaced with our `world/` BSP trace + PVS-limited snapshots.

**Deliverables**:
1. Combat aim: hitscan vs projectile lead, per-weapon lead factors, skill jitter, ±15° pitch clamp.
2. Fire timing + weapon selection: fire intervals, reaction delay, `botPickBest*` priority lists, 0.9 s switch lockout.
3. Projectile danger avoidance: `avoid_ent`-style rocket/grenade dodge (`combat>=4` gated).
4. Per-bot skill/personality: 7-field `bots.cfg`-style config + `AdjustRatingsToSkill` + full rating→behavior map.
5. FSM refinements: 2 s/4 s goal give-up, 4-u/5-s stuck recovery, `SIGHT_FIRE_DELAY` reacquire, RL-retreat, ideal-distance.
6. Eraser's known gaps fixed: explicit non-Quad powerup values, correct BFG lead, working kill/death auto-skill.

**Estimated effort**: Medium–Large (1.5–2 days)

---

## Context

### Why this plan
Plan 06 builds the brain skeleton (perception → navigate → fight → FSM). Eraser gives us **decades-proven
constants and formulas** that are pure arithmetic over state we have or reconstruct — porting them is the
single biggest quality lever for "bots that actually hit and dodge." This plan does NOT re-architect Plan 06;
it fills in the combat/danger/skill modules with Eraser's calibrated numbers and adds Eraser-specific behaviors
(danger dodge, RL-retreat, ideal-distance) that the skeleton left as stubs.

### Port the ideas, rebuild the mechanisms (AGENTS.md §Domain)
Eraser is a gamecode plugin: `gi.trace()`, full `g_edicts[]`, free LOS, exact enemy origin/velocity/health, a
same-frame `Pmove` oracle. qbots has: PVS-limited `svc_packetentities` deltas (origin only; **velocity derived**,
**health not transmitted**), and our own BSP trace. So:
- **Aim math** ports verbatim — substitute our derived velocity + BSP LOS.
- **Enemy-health-dependent behaviors** (RL-retreat vs healthy human, blaster-retreat, `bot_BetterTarget` health
  compare) → degrade to a **hit-derived healthiness estimate** (track blood/hitsound/muzzle telemetry per entity).
- **`Pmove` oracle** → drop; we send `usercmd` and read the authoritative result next frame.

### Key facts (from `distilled/eraser.md`)
- **Lead factors**: blaster/HB `dist/1000`, RL `dist/650` (ignores upward V), GL `dist/550` + piecewise pitch lob
  (+15° ≥384u; `15*(2*dist/384−1)` <384u), BFG `dist/550` is a **bug** (use `dist/400`).
- **Jitter**: `tf = min(dist/2, 256) * (5−acc)/5*2`, z-scale MG 0.1 / else 0.2; humans jittered `*(1−vmag/600)`
  (more accurate vs moving), acc5 = perfect (zero jitter).
- **Fire intervals (s)**: Blaster .6, RL .8, GL .9, RG 1.5, SG/SSG 1, HB/CG/MG 0, BFG 2.8.
- **Reaction delay**: `0.8 * (5−combat*0.5)/5` → 0.40 s (combat5) … 0.72 s (combat1).
- **Weapon priority lists**: Best (fav_weapon→BFG→CG→HB→RL→RG→MG→SSG→GL→SG→blaster), Close (CG→SSG→HB→MG→…), Far (CG→RG→MG→…).
- **Danger dodge**: grenades tag bots ≤256 u; rockets tag `combat>=4` bots ≤300 u heading-toward; perpendicular dodge-jump or strafe-away.
- **Give-up**: 2 s if >128 u away, hard 4 s. **Stuck**: 4-u deadband, 1 s cadence, jump then (we use retrail, not suicide) @ 5 s.
- **Skill remap**: `acc/cmb += (sk−1)*2.5`, `aggr -= (sk−1)*2.0`, clamp [1,5].
- **`camper` is dead code** in Eraser v1.01; **`bot_auto_skill` is declared but not wired** — we implement both fresh.

---

## Step-by-Step Tasks

> **RULES.md Rule A/B**: zero warnings, clippy clean, fmt applied, tests green — **commit**
> `task(TN): <desc>` at each boundary. Cite Eraser line refs in `///` docs.

### T1: Combat aim — hitscan vs projectile lead + jitter

**Files**: `crates/brain/src/combat.rs`, `crates/brain/src/aim.rs`

**What to do**:
- **Aimpoint**: `start = origin + forward*8; start.z += viewheight−8` (~22 standing). Build `target` per weapon.
- **Projectile lead** (RL/GL/HB/BFG/blaster): `target = enemy_origin + enemy_velocity * (dist / speed)` where
  `dist` = our BSP-traced LOS distance to enemy. Per-weapon: HB/blaster `dist/1000`, RL `dist/650` **with
  `vel.z=0 if vel.z>0`** (don't lead jumpers skyward), GL `dist/550` then **piecewise pitch lob**
  (`+15°` if dist≥384 else `15*(2*dist/384−1)` degrees), BFG **`dist/400`** (fix Eraser's bug).
- **Hitscan** (MG/CG/SG/SSG/RG): aim at current origin; RG trails `−0.2*vel` always.
- **Skill jitter** (acc<5): `tf = min(dist/2, 256) * (5−acc)/5.0 * 2`; humans `* (1−vmag/600)`; add
  `crandom()*tf` to x/y and `crandom()*tf*zscale` to z (MG 0.1, else 0.2).
- **Pitch clamp ±15°** (`bot_wpns.c:368`) — bots never aim steeply up/down.
- **Velocity derivation**: low-pass-filter enemy origin deltas across the last N `svc_frame`s (snapshot rate
  ~10-20 Hz is noisy). Add a small `tf` floor (~10-20 u) to compensate for systematic snapshot quantization.

**Commit**: `task(T1): port Eraser combat aim, per-weapon lead, and skill jitter`

### T2: Fire timing + weapon selection

**Files**: `crates/brain/src/weapons.rs`, `crates/brain/src/combat.rs`

**What to do**:
- **Fire gate**: fire iff cooldown (`time − last_fire > fire_interval`) AND reaction (`sight_enemy_time <
  time − 0.8*(5−combat*0.5)/5`) AND LOS (our BSP trace) AND within 0.2 s of `last_enemy_sight` (or freshly traced).
- **Fire intervals** table (Blaster .6, RL .8, GL .9, RG 1.5, SG/SSG 1, HB/CG/MG 0, BFG 2.8 s).
- **Weapon-select priority lists** (`botPickBestWeapon`/Close/Far) as a per-weapon preference vector keyed on
  "have weapon AND have ammo" (from our client inventory). `fav_weapon` overrides to top.
- **0.9 s switch lockout**: after sending a `weapon` impulse, withhold `BUTTON_ATTACK` for `BOT_CHANGEWEAPON_DELAY=0.9 s`.
- **Range-driven reselect**: SG/SSG/RL/GL `dist>700` → Far; GL `dist<160` (radius) → Close; RL self-range → Close.

**Commit**: `task(T2): port fire timing, weapon priority lists, and switch lockout`

### T3: Projectile danger avoidance

**Files**: `crates/brain/src/danger.rs`, `crates/brain/src/move_ctrl.rs`

**What to do**:
- Track visible projectiles (rockets/grenades) from `svc_packetentities` (classify via configstring modelindex;
  track origin/velocity by delta).
- **Rocket dodge** (`combat>=4` only, `g_weapon.c:632`): if a rocket is ≤300 u axial AND heading toward us
  (`entdist − path_along < 75`) → set `avoid_ent`.
- **Grenade dodge** (any skill, `g_weapon.c:467`): grenade ≤256 u → set `avoid_ent`.
- **`botJumpAvoidEnt`** (`bot_nav.c:357`) on our BSP: perpendicular dodge vector `(dir.y, dir.x, 0)`; pick side
  already on (random if 200-300 u); grounded + safe landing (trace 200 + 512 down, no lava) → **dodge-jump**
  (`usercmd: forwardmove=300*dir, BUTTON_JUMP`); else **strafe-away** (cache `avoid_dir` 0.3 s). Skip if
  `movetarget` within 256 u (don't dodge mid-pickup).
- **Shooter inference**: owner not sent on wire → infer from recent `svc_sound`/`svc_temp_entity` muzzle near origin;
  adopt as enemy if we have none.

**Commit**: `task(T3): add rocket/grenade danger avoidance`

### T4: Per-bot skill / personality config

**Files**: `crates/brain/src/skill.rs`, `crates/qbots/src/config.rs`

**What to do**:
- 7-field personality (Eraser `bots.cfg`): `accuracy`, `aggr`, `combat` (1-5), `fav_weapon`, `quad_freak` (bool),
  `camper` (bool), `avg_ping` (display). Serialize via `serde` TOML (roster, mirrors Plan 09 T1).
- **`AdjustRatingsToSkill`** (`bot_misc.c:1065`): `acc/cmb += (skill−1)*2.5`, `aggr -= (skill−1)*2.0`, clamp [1,5].
  Called once at spawn; `skill_level` from config (we don't read the Q2 cvar).
- **Rating→behavior map** (wire into T1-T3 + FSM): accuracy→jitter; combat→reaction/FOV(`1+combat/5`)/strafe/
  jump-cadence/crouch-at-range(`>4`)/ground-aim(`>3`)/rocket-dodge(`>=4`); aggr→item-search-freq (`(0.3*aggr/5)<rand`)
  + chase-abort (`(aggr/5)*0.2>rand`); fav_weapon→select-top + item `+=3`; quad_freak→Quad item rating `×2`.
- **Working `bot_auto_skill`** (Eraser's is stubbed): on our own kill → `skill_level = min(3, skill+0.2)`;
  on our death → `skill_level = max(0, skill−0.2)`; re-run `AdjustRatingsToSkill`. Observe via `svc_print` obituaries.

**Commit**: `task(T4): per-bot skill/personality config with skill remap and auto-skill`

### T5: FSM give-up / stuck / engagement refinements

**Files**: `crates/brain/src/fsm.rs`, `crates/brain/src/nav.rs`

**What to do** (refine Plan 06's FSM with Eraser thresholds):
- **Goal give-up watchdog**: `giveup_lastgoal` + `last_reached_trail`; abandon if `(now−last_reached > 2 s AND
  dist>128) OR (now−last_reached > 4 s)`. Blacklist (movetarget +3 s, enemy +1 s, goalentity +0.5 s via per-bot
  `HashMap<EntId, Instant>`). Relaxed while chasing freshly-sighted enemy.
- **Stuck recovery**: 4-u deadband over 1 s → `botRandomJump` (best-direction + jump); we **do not suicide**
  (unavailable over UDP) — escalate to re-path + `botRoamFindBestDirection` (7-dir 45° fan-out, `TRACE_DIST=256`,
  lift `STEPSIZE=24`, halve score if down-trace>0.4).
- **`SIGHT_FIRE_DELAY` reacquire**: on losing enemy LOS, stamp `sight_enemy_time=now` → re-arm reaction delay.
- **RL-retreat**: if enemy inferred-healthy (hit-derived) + inferred-RL + we're on blaster/shotgun → drop enemy
  (blacklist 2 s). (Enemy health/weapon are inferred, not exact — make this best-effort.)
- **Ideal-distance**: `BOT_IDEAL_DIST_FROM_ENEMY=160`; `<160` hold, `<80` back up (reverse `ideal_yaw`).

**Commit**: `task(T5): refine FSM with Eraser give-up/stuck/engagement thresholds`

### T6: Fix Eraser's gaps

**Files**: `crates/brain/src/{weapons,items}.rs`

**What to do**:
- **Non-Quad powerups**: author explicit `dist_divide` values (invuln=5, mega-as-bonus=4, silencer/breather/adrenaline=2,
  power-shield=3) instead of Eraser's default `1`.
- **BFG lead**: `dist/400` (not Eraser's `dist/550` bug).
- **Camping** (Eraser's `camper` is dead code — author fresh): if `camper` flag set, pick a camp node (nav node
  nearest a fav-weapon/quad spawn with good cover + LOS), dwell there when no pressing enemy/item; rotate camps.

**Commit**: `task(T6): fix Eraser gaps — powerup values, BFG lead, fresh camping`

### T7: Verify — a bot that frags like Eraser

**What to do**: One bot vs another qbots instance (or a populated server). Assert: hitscan and projectile aim
connect at range; bots dodge visible rockets/grenades (`combat>=4`); skill 1 misses more than skill 5; auto-skill
drifts on kills/deaths; give-up fires (no infinite chase); stuck recovery un-wedges. Tune; log FSM transitions.
Record tuning in `context/distilled.md`.

**Commit**: `task(T7): verify Eraser-grade combat, dodge, and skill`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/{combat,aim}.rs` | lead/jitter/pitch-clamp | P0 |
| `crates/brain/src/weapons.rs` | fire timing + select | P0 |
| `crates/brain/src/danger.rs` | projectile dodge | P0 |
| `crates/brain/src/skill.rs` | personality + remap + auto-skill | P0 |
| `crates/brain/src/{fsm,nav}.rs` | give-up/stuck/engagement | P1 |
| `crates/brain/src/items.rs` | powerup values + camping | P2 |
| `crates/qbots/src/config.rs` | roster skill fields | P1 |

---

## Open Questions / Risks

1. **Velocity derivation noise.** Q2 sends player velocity inconsistently; frame-differencing at 10-20 Hz is
   noisy → systematic lead error. *Mitigation*: T1 low-pass-filters + adds a small `tf` floor; tune against a
   stationary-vs-strafing target.
2. **Enemy health/weapon not transmitted.** RL-retreat and `bot_BetterTarget` can't use exact health. *Mitigation*:
   T5 builds a hit-derived estimate; keep those behaviors best-effort/gated.
3. **Projectile visibility subset.** We only dodge projectiles in our PVS (Eraser dodges all). *Mitigation*:
   accept the limitation; document it; PVS-in already filters most threats behind us.
4. **Tuning surface explosion.** accuracy/combat/aggr/fav + per-weapon intervals = many knobs. *Mitigation*:
   ship Eraser's defaults (they're calibrated); tune only via T7 capture.

---

## Verification Checklist

- [ ] T1: hitscan aims at origin; RL leads `dist/650` ignoring upward V; GL lobs; BFG uses `dist/400`; acc5 has zero jitter, acc1 misses more.
- [ ] T2: fire respects interval + reaction delay; 0.9 s switch lockout enforced; Far/Close reselect at 700 u.
- [ ] T3: `combat>=4` bot dodge-jumps a visible rocket; grenade within 256 u triggers strafe-away.
- [ ] T4: `bots.cfg`-style config parses; skill3 raises acc/cmb +5, lowers aggr; auto-skill drifts on kill/death.
- [ ] T5: bot abandons unreachable goal in ≤4 s; stuck → jump/retrail (never suicide); reacquire respects delay.
- [ ] T6: non-Quad powerups valued >1; BFG leads correctly; camper dwells at a camp node.
- [ ] T7: single bot frags over a multi-minute run; skill tiers visibly differ; dodges connect.

---

> **⚠️ CRITICAL REMINDERS ⚠️**
> - **COMMIT AT EVERY TASK COMPLETION** — Format: `task(TN): <description>`. DO NOT WAIT!
> - **FIX ALL WARNINGS BEFORE EACH COMMIT** — `cargo clippy -- -D warnings` must pass.
> - **RUN ALL TESTS BEFORE EACH COMMIT** — `cargo test` must pass.
> - **MOVE COMPLETED PLANS TO `completed/` IMMEDIATELY** — When 100% done, `git mv` to `completed/`.
> - **NEVER batch multiple tasks into one commit** — One task per commit, always.
> - **Reread RULES.md AFTER EACH TASK** — Re-read RULES.md at the end of every task to stay on track.
