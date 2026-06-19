# Quake III Arena bot AI — distilled (for a qbots Q3-derived brain)

> Source: `vendor/Quake-III-Arena/code/` (id Software GPL release).
> Behavioral reference only — every `trap_*`, `gi.*`, `g_entities[]`, and **AAS** call is
> server-side omniscience qbots does **not** have. Treat the C as *pseudocode for behavior*.
> This file captures the **decision logic** worth porting; the *mechanisms* (AAS routing,
> `trap_AAS_PredictClientMovement`, `BotAI_Trace`) are replaced by our `world` crate
> (nav graph + `CollisionModel` trace + LOS) and our PVS-limited `Worldview`.
>
> Companion file: `context/distilled/eraser.md` (the Eraser bot, already ported as `MainBrain`).
> Q3 and Eraser overlap a lot; the **distinctive** Q3 ideas are flagged **[PORT]** below.

---

## 0. The two layers of Q3 bot code

Q3 splits the bot into two layers; only the **upper** layer is behavior we port:

| Layer | Files | What it is | qbots equivalent |
|-------|-------|-----------|------------------|
| **`game/ai_*.c`** | `ai_main`, `ai_dmnet`, `ai_dmq3`, `ai_cmd`, `ai_chat`, `ai_team`, `ai_vcmd` | The *bot brain*: FSM nodes, enemy selection, aiming, weapon/goal choice, chat. Calls down into botlib. | **what we port** → a `trait Brain` plugin |
| **`botlib/be_*.c`** | `be_aas_*` (routing/sampling/reach), `be_ai_move`, `be_ai_goal`, `be_ai_weight`, `be_ai_char`, `be_ai_weap`, `be_ea` | The *engine* of the bot: **AAS** (Area Awareness System = precompiled nav mesh + routing), movement execution, fuzzy goal/weight evaluation, character files, "elementary actions" (`EA_*` = press buttons). | replaced by `world` (nav/trace/LOS) + `nav_mode::Navigator` + `move_ctrl`/`steer` + our own item model |

**Key consequence:** Q3's bot is *fed* perfect world facts by AAS/`trap_*`. Our brain must
derive the same decisions from `Worldview` (PVS-limited entity deltas) + `world` traces.
Where Q3 calls AAS routing (`trap_BotMoveToGoal`, `trap_AAS_AreaTravelTimeToGoalArea`), we
already have `Navigator` (A*/navmesh/hybrid) — inject it, same as `MainBrain`.

---

## 1. The node-based FSM (`ai_dmnet.c`) **[PORT — this is the spine]**

Q3's AI is a **finite-state machine of function pointers**. `bs->ainode` holds the current
node fn; each AI frame calls it; the node either does its work and returns, or calls
`AIEnter_<X>()` to switch nodes (which sets `bs->ainode` and records a "node switch" for the
50-switch/ frame loop-guard `MAX_NODESWITCHES`). `ai_main.h:134`: `int (*ainode)(bot_state_t*)`.

### The nodes (`ai_dmnet.h`)

Full set: `Intermission`, `Observer`, `Respawn`, `Stand`, `Seek_ActivateEntity`,
`Seek_NBG`, `Seek_LTG`, `Battle_Fight`, `Battle_Chase`, `Battle_Retreat`, `Battle_NBG`.

**Deathmatch-relevant subset (what qbots needs):**

```
                 dead
   any node ───────────────▶ Respawn ──(respawned)──▶ Seek_LTG
                                                          │
   ┌──────────────────────────────────────────────────────┤
   │                                                        │ no enemy: roam toward a
   ▼                                                        │ Long-Term Goal item
 Seek_LTG  ◀──────── enemy dead / lost ──────┐             │
   │  find enemy?                              │             ▼
   │   ├─ wants-retreat ─▶ Battle_Retreat ─────┤   periodic nearby-item check
   │   └─ else          ─▶ Battle_Fight        │   (range ~150u) → Seek_NBG
   │                          │                │             │ (grab item, ≤ nbg_time) ─▶ back
   │                          │ enemy out of   │
   │                          │ sight?         │
   │                          │  ├ wants-chase ─▶ Battle_Chase ──(visible)─▶ Battle_Fight
   │                          │  └ else        ─▶ Seek_LTG     ──(timeout 10s)─▶ Seek_LTG
   │                          │ wants-retreat? ─▶ Battle_Retreat
   │                          ▼
   └──────────────── (item nearby during battle) ─▶ Battle_NBG ─▶ back to battle node
```

### Node responsibilities (DM only)

- **`AINode_Respawn`** (`ai_dmnet.c:1260`): wait dead; press attack/buttons to respawn;
  on respawn → `Seek_LTG`. (qbots: detect death via playerstate; `on_death()` hook exists.)
- **`AINode_Stand`** (`:1201`): stand still for `stand_time` (used after a chat/taunt). Minor.
- **`AINode_Seek_LTG`** (`:1795`) — **the roaming brain**:
  1. dead/intermission/observer guards → switch out.
  2. `bs->enemy = -1`; **`BotFindEnemy()`** → if found: `BotWantsToRetreat()` ?
     `Battle_Retreat` : (empty goal stack →) `Battle_Fight`.
  3. else get the long-term goal item (`BotLongTermGoal`/`BotGetItemLongTermGoal`), and
     **every 0.5 s** check for a nearby goal (`BotNearbyGoal`, range 150) → `Seek_NBG`.
  4. move to goal via `trap_BotMoveToGoal`; set view from the move dir or a random "roam"
     look (`BotRoamGoal`) when waiting/idle.
- **`AINode_Seek_NBG`** (`:1662`): same as LTG but for the transient *nearby* goal with an
  `nbg_time` deadline (≈ `4 + range*0.01` s); when reached or expired → `Seek_LTG`. Enemy
  found behaves like LTG (retreat/fight).
- **`AINode_Battle_Fight`** (`:1986`) — **the combat brain**:
  1. `BotFindEnemy(bs->enemy)` → may upgrade to a *better* (closer) enemy.
  2. enemy dead / invisible-and-not-shooting (20 % chance to lose track) → `Seek_LTG`.
  3. enemy **not visible** → `BotWantsToChase()` ? `Battle_Chase` : `Seek_LTG`.
  4. `BotChooseWeapon` → `BotAttackMove` (circle-strafe) → **`BotAimAtEnemy`** →
     **`BotCheckAttack`** (fires).
  5. after attacking: `BotWantsToRetreat()` → `Battle_Retreat` (unless `BFL_FIGHTSUICIDAL`).
- **`AINode_Battle_Chase`** (`:2151`): goal = `lastenemyorigin`/`lastenemyareanum`; if enemy
  reappears → `Battle_Fight`; if reached last spot or `chase_time` (10 s) elapsed → `Seek_LTG`;
  periodic nearby-item → `Battle_NBG`. Aims at enemy if chased < 2 s ago, else looks along path.
- **`AINode_Battle_Retreat`** (`:2290`): if `BotWantsToChase()` flips true (e.g. picked up a
  better weapon) → `Battle_Chase`; track last-visible; if enemy unseen 4 s → `Seek_LTG`; move
  to an LTG **while retreating** (`BotLongTermGoal(retreat=true)`), grab nearby items
  (`Battle_NBG`), still aim+fire if `attack_skill > 0.3`. If "no way out" → `Battle_SuicidalFight`.
- **`AINode_Battle_NBG`** (`:2479`): grab a transient item mid-fight; when done → back to the
  appropriate `Seek_NBG`/battle node. Still tracks enemy visibility.

**Loop guard:** `BotResetNodeSwitches`/`MAX_NODESWITCHES=50` — if the FSM thrashes >50
switches in a frame it's a bug. **[PORT]** as a per-tick switch counter (cheap safety net).

> **qbots mapping.** `MainBrain` already collapses this into a 5-state FSM
> (`Roam/Hunt/Engage/Flee/Pickup`, `fsm.rs`). The Q3 brain keeps the **explicit
> `Fight/Chase/Retreat/NBG` separation** because the *transitions* are driven by the
> aggression scalar (§2) — that's the behavioral payload. `Seek_LTG`≈Roam, `Seek_NBG`≈Pickup,
> `Battle_Fight`≈Engage, `Battle_Chase`≈Hunt, **`Battle_Retreat` is new** (qbots' `Flee` is
> health-only; Q3 retreats whenever *aggression < 50*, a richer trigger).

---

## 2. `BotAggression` — the decision scalar **[PORT — the standout feature]**

`ai_dmq3.c:2199`. A single 0–100 number computed each combat decision from the bot's
**loadout + health + armor + enemy geometry**. Threshold **50** gates retreat & chase:

```
BotAggression(bs):
  if QUAD and (weapon != gauntlet or enemy within 80u):      return 70   // quad → press
  if enemy is >200u higher than me:                          return 0    // bad angle
  if health < 60:                                            return 0
  if health < 80 and armor < 40:                            return 0
  // best available weapon+ammo determines aggression:
  if BFG  & ammo>7:  return 100
  if RAIL & slugs>5: return 95
  if LG   & ammo>50: return 90
  if RL   & rockets>5: return 90
  if PG   & cells>40:  return 85
  if GL   & nades>10:  return 80
  if SG   & shells>10: return 50
  return 0                                                   // only weak guns → flee
```

```
BotWantsToRetreat(bs): return BotAggression(bs) < 50     // ai_dmq3.c:2268
BotWantsToChase(bs):   return BotAggression(bs) > 50     // ai_dmq3.c:2321
```

`BotFeelingBad` (`:2247`, used for retreat in some modes): gauntlet=100, health<40=100,
machinegun=90, health<60=80.

**Why this matters:** it ties *engage/disengage* to a meaningful, legible health+weapon
heuristic instead of a fixed health threshold. A full-health bot with only a machinegun still
plays cagey; a hurt bot with a railgun + slugs still presses. This is the single most
characterful Q3 idea to bring to qbots.

### qbots adaptation (PVS / wire constraints) **— important**

Q3 reads `bs->inventory[...]` (full server-side inventory). **qbots only sees the
playerstate** — `SelfState { health, armor, frags, ammo:[i32;32], weapon, flags }`
(`perception.rs`). On the Q2 wire the `stats[]` array carries `STAT_HEALTH`, `STAT_ARMOR`,
and `STAT_AMMO` (*current weapon's* ammo). We do **not** get a free per-weapon ammo
inventory for all weapons like Q3 does.

→ The qbots `bot_aggression()` must use **what's observable**:
  - `health`, `armor` — direct.
  - **held weapon** (`weapon` / `gunindex` → configstring model name → `Weapon` enum,
    already done in `weapons.rs`) and **its** ammo (`STAT_AMMO`).
  - We *infer* weapon strength from the **currently held** weapon, not the best in a full
    inventory. (Q2 auto-switches to best on pickup anyway, so "held weapon" is a decent
    proxy for "best owned".) Track picked-up weapons over the life via obituary/pickup
    prints if we want a fuller inventory later (optional, Plan 38).
  - Quad: Q2 `STAT_*`/configstring "quad" timer if visible; otherwise skip the quad branch.
  - Enemy-height delta: from the enemy entity origin in `Worldview` (PVS-limited; fine).

Map Q2 weapons to the Q3 tiers by raw power: **BFG≈Q3 BFG**, **Railgun≈RAIL**,
**Hyperblaster≈PG/LG**, **RocketLauncher≈RL**, **GrenadeLauncher≈GL**,
**SuperShotgun/Shotgun≈SG**, **Machinegun/Chaingun≈MG (weak)**, **Blaster/Gauntlet≈0**.
See `weapons.rs` for the existing enum; add a `power_tier()` there.

---

## 3. Characteristics = the personality system (`chars.h`, `be_ai_char.c`) **[PORT]**

Every Q3 bot loads a **character file** (`bots/*.c`) of ~48 named **characteristics**, mostly
floats in `[0,1]`, read via `trap_Characteristic_BFloat(char, INDEX, min, max)`. These are the
*only* per-bot personality knobs — all behavior above multiplies against them.

### The DM-relevant characteristics (index in `chars.h`)

| # | Name | Range | Drives |
|---|------|-------|--------|
| 2 | `ATTACK_SKILL` | [0,1] | combat-movement quality: <0.2 stand still, <0.4 walk to/from only, ≥0.4 circle-strafe (`BotAttackMove`) |
| 6 | `REACTIONTIME` | [0,5]s | delay before aiming/firing at a just-sighted enemy (`enemysight_time` gate in `BotAimAtEnemy`/`BotCheckAttack`) |
| 7 | `AIM_ACCURACY` | [0,1] | base aim error magnitude (1=perfect). Plus per-weapon overrides #8–15 |
| 16 | `AIM_SKILL` | [0,1] | enables prediction: >0.4 linear lead, >0.8 exact lead, >0.95 "don't aim too early", radial ground-aim >0.6 |
| 36 | `CROUCHER` | [0,1] | chance to crouch in combat |
| 37 | `JUMPER` | [0,1] | chance to jump in combat (dodge) |
| 48 | `WALKER` | [0,1] | tendency to walk (slow, quiet) vs run |
| 41 | `AGGRESSION` | [0,1] | **note:** in stock Q3, `BotAggression()` (§2) is *not* actually scaled by this char (it's loadout-based); the char feeds goal/weight fuzzy logic. We **[PORT]** it as a bias on our aggression threshold (e.g. threshold = 50 − (aggression−0.5)·40). |
| 42 | `SELFPRESERVATION` | [0,1] | avoid firing rockets/own-radial near walls (`BotCheckAttack` radial self-damage check) |
| 43 | `VENGEFULNESS` | [0,1] | tendency to hunt the bot that last killed you (`revenge_enemy`) |
| 44 | `CAMPER` | [0,1] | tendency to camp a spot (qbots `BotSkill.camper` already exists) |
| 45 | `EASY_FRAGGER` | [0,1] | <0.5 → won't shoot chatting players; raises target greed |
| 46 | `ALERTNESS` | [0,1] | **enemy detection range**: skip enemies past `√(900² ... )`; `squaredist > Square(900 + alertness*4000)` (`BotFindEnemy:3012`). Also widens awareness FOV. |
| 47 | `FIRETHROTTLE` | [0,1] | **burst-fire duty cycle**: fraction of time the bot is "allowed" to shoot (`BotCheckAttack:3591`) — humanizes sustained fire |

Per-weapon aim accuracy (#8–15) and per-weapon aim skill (#17–20) let a character be a
crack shot with the rail but spray with the MG.

> **qbots mapping.** `BotSkill`/`Ratings` (Eraser) already covers accuracy/combat/aggression
> on a 1–5 axis with a 0–10 master skill and auto-skill drift. The Q3 brain adds a **richer,
> Q3-shaped `Q3Character`** (the named [0,1] floats above) so a Q3 bot's *texture* (alertness,
> firethrottle, jumper/croucher, vengefulness, camper, per-weapon accuracy) is independent of
> the Eraser skill remap. Keep `BotSkill` for the shared combat/aim modules; add `Q3Character`
> for the Q3-specific knobs. A `skill 0–10 → Q3Character` mapping gives presets (§7).

---

## 4. Enemy selection (`BotFindEnemy`, `ai_dmq3.c:2929`) **[PORT, adapted]**

Per frame, scan all clients and pick the **best** enemy:

1. Skip self, dead, invisible-and-not-shooting; if `EASY_FRAGGER<0.5` skip chatting players.
2. Skip if just-teleported near (3 s grace) to avoid spawn camping the teleport exit.
3. **Closest wins**: if a current enemy exists, skip any candidate *farther* than it
   (unless that candidate carries a flag).
4. **Range gate by ALERTNESS:** skip if `dist² > (900 + alertness·4000)²`.
5. Skip same-team.
6. **Awareness FOV:** if my health just dropped **or** the enemy is shooting → treat FOV as
   **360°** (full awareness — you notice who's hurting you). Otherwise FOV narrows with
   distance: `f = 90 + 90 − (90 − …)` (≈ wide when close, ~90° far).
7. **Visibility:** `BotEntityVisible(eye, viewangles, fov, ent)` — trace + FOV check
   (`:2825`). qbots: `los::has_los_player(cm, eye, target)` + FOV.
8. **Avoidance:** if the enemy is far (>100u), not shooting, I'm undamaged, **and I'm not in
   *their* FOV** → if `BotWantsToRetreat()` I skip them (sneak past). Else commit:
   `bs->enemy = i`, set `enemysight_time` (drives the reaction-time delay).

`enemysight_time` set to `now` on a fresh sight, `now−2` when upgrading from an existing
enemy (so the reaction delay doesn't re-trigger mid-fight).

**qbots adaptation:** clients-only loop becomes "iterate `Worldview.enemies()`" (PVS-limited,
which is *more* realistic — we literally can't see enemies outside PVS). Health-drop detection:
compare `health` to last frame (we have it). "Enemy shooting": Q2 wire — infer from
`EntityState` event/`muzzleflash` temp-entities or the enemy's `EF_*`/frame; conservative
fallback = treat unknown as not-shooting. Same-team via `frags`/skin or DM=no teams.

---

## 5. Aiming (`BotAimAtEnemy`, `ai_dmq3.c:3261`) **[PORT — distinct from Eraser]**

The richest single function. Order of operations:

1. Resolve **per-weapon** `aim_skill` + `aim_accuracy` from the held weapon's characteristics.
2. **Reaction gate:** if `aim_skill > 0.95`, don't even start aiming until the enemy has been
   in sight longer than `0.5·REACTIONTIME` (and not just-teleported). (Lower-skill bots aim
   immediately but inaccurately — feels human either way.)
3. **Invisible enemy** → mostly aim badly (`accuracy *= 0.4` 90 % of the time).
4. **Velocity memory:** enemy origin+velocity sampled every **0.5 s** (`enemyposition_time`).
   If `aim_skill<0.9` and the enemy *changed direction* since last sample
   (`dot(oldvel,newvel)<0`) → `accuracy *= 0.7` (you get faked out by a direction change).
5. **Lead prediction** (only for projectile weapons, `wi.speed>0`, and not when the enemy is
   far + micro-strafing):
   - `aim_skill>0.8` & weapon ready → **exact**: `trap_AAS_PredictClientMovement` simulates the
     enemy forward `dist·10/speed` frames. → qbots: replace with our own ballistic lead
     (constant-velocity extrapolation + optional gravity for grenades) using `world` traces.
   - `aim_skill>0.4` → **linear**: `bestorigin = enemy + (dist/speed)·enemyvel_horizontal`.
6. **Radial ground-aim** (`aim_skill>0.6`, rockets/BFG/grenades, enemy not higher than +16):
   aim at the floor in front of the enemy (splash). Trace down to find the floor, verify the
   spot is LOS-clear and ≥100u away, then aim there.
7. **Aim error injection** (the "miss" model):
   - `bestorigin += 20·crandom·(1−accuracy)` on x,y, `10·` on z (worldspace jitter).
   - hitscan distance falloff: MG/SG/LG/RAIL → `accuracy *= 0.6 + min(dist,150)/150·0.4`
     (more accurate up close).
   - `if accuracy < 0.8`: perturb the aim *direction* by `0.3·crandom·(1−accuracy)` per axis.
   - finally add **weapon-spread** comp: `pitch/yaw += 6·vspread/hspread·crandom·(1−accuracy)`.
8. `bot_challenge` cvar: super-accurate bots snap-aim (skip the gradual turn). Optional.

**Not-visible** branch (enemy behind cover, last-known origin): predict around corners for
splash weapons (`trap_BotPredictVisiblePosition`); accuracy forced to 1 for that guess.

> **qbots mapping.** `aim.rs`/`combat.rs` already do Eraser-style lead + jitter scaled by the
> `accuracy` rating. The Q3 additions worth porting into a **`brains/q3/aim.rs`** (so we don't
> disturb `MainBrain`): **per-weapon accuracy/skill**, **reaction-time sight gate**,
> **direction-change accuracy penalty**, **hitscan distance falloff**, **weapon-spread comp**,
> and **radial ground-aim** for splash weapons. Replace AAS exact-predict with our own
> constant-velocity lead (we already lead in `aim.rs`).

---

## 6. Combat movement & firing

### `BotAttackMove` — circle-strafe (`ai_dmq3.c:2631`) **[PORT]**

```
if ATTACK_SKILL < 0.2: don't move (sitting duck)
movetype = WALK; maybe JUMP (random<JUMPER) or CROUCH (random<CROUCHER), 1s cooldowns
if gauntlet: attack_dist=0  else attack_dist=IDEAL_ATTACKDIST, range=±40
if ATTACK_SKILL <= 0.4:                 // dumb: only close/open the gap
   if dist > ideal+range: move toward; if dist < ideal-range: move away
else:                                    // circle-strafe
   strafe_time += dt
   strafechange = 0.4 + (1-skill)*0.2   // skilled bots change strafe dir more crisply
   if strafe_time > strafechange and random>0.935: flip BFL_STRAFERIGHT, reset
   sideward = cross(forward_horiz, up); if STRAFERIGHT negate
   if random>0.9: add backward   else: close/open to ideal dist (add forward/backward)
   move in `sideward`; on fail, flip strafe dir and retry
```

So: **strafe perpendicular to the enemy, randomly flip direction, blend in forward/backward to
hold an ideal range band, occasionally jump/crouch.** qbots' `MainBrain` already does a
circle-strafe (`steer.rs` strafe_tick + IDEAL_DIST/BACKUP_DIST). The Q3 version differs in the
**random strafe-flip cadence** and **jump/crouch-by-characteristic** dodging — port those into
the Q3 brain's movement.

### `BotCheckAttack` — the trigger (`ai_dmq3.c:3555`) **[PORT — fire-throttle is distinct]**

Fires only if **all** hold:
1. `enemysight_time` older than `REACTIONTIME` (and not just-teleported). **Reaction gate.**
2. not mid-weapon-change (`weaponchange_time`, 0.1 s).
3. **FIRETHROTTLE duty cycle:** maintain alternating "shoot window" / "wait window" whose
   lengths come from `FIRETHROTTLE` (`random()>throttle` → wait `throttle` s; else shoot
   `1−throttle` s). Stops bots from holding the trigger forever — looks human, conserves ammo.
4. gauntlet only if enemy within 60u.
5. aim is within FOV (120° if target <100u else 50°) of current view — i.e. **must be roughly
   looking at the aim target** (you can't fire while still turning).
6. LOS trace to `aimtarget` not blocked by world.
7. **Self-preservation / teammate guard:** trace the shot; if it'd hit a teammate, abort; if
   it's a **radial** weapon and the impact is close enough to splash *me* (`SELFPRESERVATION`),
   abort (don't rocket your own feet near a wall).

> **qbots mapping.** `combat.rs` produces `should_fire` from LOS+FOV already. The Q3-distinct
> bits to add: the **fire-throttle duty cycle** and the **reaction-time sight gate** (track
> `enemy_first_seen_tick` in the Q3 brain), plus the **radial self-damage abort** for splash
> weapons near walls (`world` trace from muzzle → if impact within blast radius of self,
> don't fire) gated by a `SELFPRESERVATION` knob.

---

## 7. Putting it together — the Q3 brain for qbots

A `Q3Brain` (`trait Brain` plugin, `BrainKind::Quake3`, CLI `--brain q3`) that injects the
existing `Navigator` and reuses `world`/`steer`/`recover`, but replaces the *decision layer*:

- **State:** an explicit node enum `{ SeekLtg, SeekNbg, BattleFight, BattleChase,
  BattleRetreat, BattleNbg, Respawn }` + per-node timers (`chase_time`, `nbg_time`,
  `enemysight_time`, `firethrottle_*`, `enemyposition_time`) + a `Q3Character`.
- **Per tick:** run the current node fn; nodes switch via the aggression-gated transitions
  (§1); `BotFindEnemy`-style selection (§4) feeds `BattleFight`; aim/fire via the Q3 aim/fire
  model (§5/§6); roam/NBG goals drive the injected `Navigator` (like `MainBrain`).
- **Personality presets** (skill 0–10 + a named Q3 character): map a master skill to the
  `Q3Character` floats (à la `AdjustRatingsToSkill`), and ship a small roster of named
  characters (e.g. *Grunt* = low skill/high firethrottle spray; *Major* = high
  aim_skill/low firethrottle precision; *Sarge* = high aggression/jumper; *Camper* = high
  camper/alertness, low aggression). Stock Q3 character files in
  `vendor/Quake-III-Arena/.../bots/` are the reference for plausible value sets.

### Core additions needed (small)

The `trait Brain` seam (`BrainContext`/`BrainOutput`/`BrainMap`) is already rich enough for
the *decision* work — `Worldview` carries health/armor/weapon/ammo/frags, `cm` gives
trace/LOS, `nav` is injected, `dt`/`ticks` drive timers. Expected additions are minor:
- `weapons.rs`: a `power_tier()` (BFG>RAIL>RL/LG>PG>GL>SG>MG>0) for `bot_aggression()`.
- `MovementIntent`: confirm **crouch** support (it has `up`; crouch = `up<0` / a `crouch()`
  helper) for the croucher characteristic. Add if missing.
- Optionally surface "enemy is shooting" / "I took damage this frame" on `Worldview`
  (health-delta is derivable in-brain; muzzleflash/temp-entity is a perception add — optional).
- No changes to `Navigator`, the driver, or `MainBrain`.

### What we deliberately drop (CTF/teamplay/mission-pack)

`ai_team.c`, `ai_vcmd.c`, most of `BotLongTermGoal` (LTG_TEAMHELP/ACCOMPANY/DEFEND/GETFLAG/…),
obelisk/harvester/1FCTF, chat (`ai_chat.c`, `be_ai_chat.c`), grapple, kamikaze. DM only.

---

## 8. Quick reference — file → behavior map

| Q3 file:line | Behavior | qbots port target |
|---|---|---|
| `ai_dmnet.c` (FSM nodes) | node FSM + transitions | `brains/q3.rs` node enum + per-node fns |
| `ai_dmq3.c:2199 BotAggression` | engage/disengage scalar | `q3char::bot_aggression(view, char)` |
| `ai_dmq3.c:2268/2321` WantsToRetreat/Chase | aggression<50 / >50 | `q3char` thresholds |
| `ai_dmq3.c:2929 BotFindEnemy` | enemy pick (alertness, closest, awareness FOV) | `brains/q3` enemy select over `Worldview.enemies()` |
| `ai_dmq3.c:3261 BotAimAtEnemy` | per-weapon accuracy, lead, error model | `brains/q3/aim.rs` |
| `ai_dmq3.c:3555 BotCheckAttack` | reaction gate, fire-throttle, self-preservation | `brains/q3/aim.rs` fire decision |
| `ai_dmq3.c:2631 BotAttackMove` | circle-strafe, jump/crouch dodge | `brains/q3` movement |
| `chars.h` / `be_ai_char.c` | 48 named characteristics | `q3char::Q3Character` (named [0,1] floats) |
| `be_ai_goal.c` / `be_ai_weight.c` | fuzzy item weights (LTG/NBG) | reuse `items.rs`; optional fuzzy upgrade |
| `be_aas_*` , `be_ai_move.c` | AAS routing + movement exec | **already have** `Navigator` + `move_ctrl`/`steer` |

> See `context/plans/36_*`, `37_*`, `38_*` for the implementation breakdown.
