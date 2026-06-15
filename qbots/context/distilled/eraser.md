# Eraser Bot v1.01 — Distilled (for qbots)

> Source of truth: `vendor/Quake2BotArchive/extracted/Eraser101_SRC/Eraser/src/`
> (extracted from `bin/Eraser101_SRC_b.zip` — the **final, author-released source**, v1.01).
> Read before any brain/combat/nav work. Cite as `bot_*.c:line`. Companion: 3ZB2 notes live in `../distilled.md` / `vendor/3zb2-zigflag/`.

## The one caveat that frames everything

**Eraser is a gamecode plugin (`gamex86.dll`).** It runs *inside the server* with **free, omniscient access**:
`gi.trace()`, the full `g_edicts[]` entity table, `gi.inPVS()`, exact origins/velocities/health of every
player, physics (`gi.Pmove`), and direct velocity writes. **qbots is none of that** — it's an external UDP
client that sees only what the server sends within the PVS, lagged and partial, and must rebuild a world
(BSP trace + nav graph) itself.

So the rule for every section below:
- **Algorithms/decisions/numbers port** (they're arithmetic over state we have or reconstruct).
- **Every `gi.trace`/`gi.inPVS`/`visible()`/`M_walkmove`/direct-`velocity=` → replace** with our `world/` BSP trace,
  nav-graph A*, or a `usercmd_t` population.
- **Enemy health, full player table, projectiles behind you → impossible** to observe over the wire; degrade gracefully.

> **⚠️ The route engine core is WITHHELD.** `p_trail.c` (the route-table: `CalcRoutes`, `OptimizeRouteCache`,
> `PathToEnt`, `ClosestNodeToEnt`, `CheckMoveForNodes`, `matching_trail`, `.rt2` save/load) is bound by an NDA
> (`bot_nav.c:38-53`) and shipped only as compiled `NavLib.lib`. Everything about those internals below is
> **inferred from call sites + struct fields + cvars**, marked [INF]. Reimplement from first principles (A* on
> our BSP nav graph), don't try to match NavLib byte-for-byte.

---

## 0. How a bot "thinks" (frames, not physics ticks)

- Bots are `edict_t` with `bot_client=true`, driven through Id's **monster think pipeline**
  (`walkmonster_start_go → monsterinfo.run → bot_run`, `bot_ai.c:405`). There is **no FSM enum** —
  "states" are implicit, composed from the triad `{enemy, movetarget, goalentity, target_ent}`.
- **Distributed thinking**: `p_client.c:1884-1908` round-robins bots so only one thinks per slot;
  `bot_frametime` is a **constant `0.1`** (`g_spawn.c:847`), not real frame delta. Per-frame move budget
  `dist = BOT_RUN_SPEED * bot_frametime = 300 * 0.1 = 30` units (`bot_ai.c:476`).
- Eraser builds a **synthetic `usercmd_t` and feeds it to `gi.Pmove()`** via `BotMoveThink` (`bot_nav.c:229`) —
  it reuses real player physics as an *oracle*, inspecting the result the same frame.
- **qbots**: we already send a real `usercmd_t` per `clc_move` (Plan 04). We replicate the *decision* layer
  that produces it; we **do not** run `Pmove` locally (the server does). So "run pmove then inspect" is
  impossible — we predict or accept ~1 frame latency.

### Per-bot state (the ~40 fields on `edict_t`, `g_local.h:1270-1352`)
`enemy`, `movetarget` (desired item), `goalentity` (current nav node), `target_ent` (guard/follow),
`last_enemy_sight`, `sight_enemy_time`, `search_time`, `last_seek_enemy`, `giveup_lastgoal`,
`last_goal`/`last_goal_time`, `movetogoal_time`, `bored_suicide_time`, `node_target`, `jump_velocity`,
`checkstuck_origin`/`checkstuck_time`, `avoid_ent`/`avoid_dir`/`avoid_dir_time`, `closest_trail`/`time`,
`strafe_dir`/`strafe_changedir_time`, `crouch_attack_time`, `bot_plat_pausetime`, `last_ladder_touch`,
`fire_interval`/`last_fire`, `bot_fire` (fn ptr to current weapon's `bot*` func), `bot_stats*`, `botdata*`,
`skill_level`. Plus route-node fields when the edict is a node: `paths[24]`, `trail_index`, `node_type`, `routes*`.

### Skill ratings (`bot_stats_t`, `g_local.h:938-947`)
```c
float accuracy;      // 1-5  aiming accuracy
float aggr;          // 1-5  higher = attack players instead of collecting items
float combat;        // 1-5  strafing/jumping/crouching while firing
gitem_t *fav_weapon; //       seek from further away; attack more often when held
int quad_freak;      // 0/1  won't seek quad from far away if 0
int camper;          // 0/1  ⚠️ DEAD CODE in v1.01 — see §9
int avg_ping;        //       scoreboard cosmetics only
```

### Load-bearing constants (`bot_procs.h`)
`BOT_RUN_SPEED=300`, `BOT_STRAFE_SPEED=200`, `BOT_IDEAL_DIST_FROM_ENEMY=160`, `BOT_GUARDING_RANGE=600`,
`STEPSIZE=24`, `SIGHT_FIRE_DELAY=0.8`, `BOT_CHANGEWEAPON_DELAY=0.9`, `MAX_BOTS=25`. WANT tiers:
`WANT_KINDA=1`, `WANT_YEH_OK=2`, `WANT_SHITYEAH=3`. Node types `NODE_{NORMAL,PLAT,LANDING,BUTTON,TELEPORT,GRAPPLE}`.

---

## 1. The think loop / behavior FSM (`bot_run`, `bot_ai.c:405-896`)

Per-frame order of operations:

1. Die/swim guards; enemy sanity (null non-client enemy).
2. **Enemy-scan (fast roam)** — always `bot_roam(self, true)` (no route recompute): the *target-acquisition* tick.
3. Taunt abort (skill≥3 + enemy in PVS → break into run).
4. Compute move distance + crouch (ducking `dist*=0.5`, `viewheight=-2`; standing `viewheight=22`).
5. Grapple branch; enemy/team/world filters; `movetarget` validity (item still `SOLID_TRIGGER`?).
6. Dead-enemy handling → maybe taunt, else clear + force full `bot_roam(false)`.
7. **Re-path on enemy acquired** (`PathToEnt(enemy)`); full roam if no real goal; no-goal fallback
   (`botRoamFindBestDirection` + `bot_move`).
8. **Main movement**: set `ideal_yaw` by priority → `bot_ChangeYaw` → `bot_MoveAI` → `bot_SuicideIfStuck` →
   `bot_Attack` (if enemy).
9. Platform pause (rising plat → 0.3 s pause).
10. **Give-up watchdog** (2 s/4 s — §3).
11. Idle re-roam if wandering a non-item movetarget >5 s.

**Implicit modes** (no enum; qbots should make this an explicit `enum BrainState`):
`Roam` (enemy=null, movetarget=null → best-direction + move) · `Pickup` (item movetarget) ·
`Hunt` (enemy set, goalentity=path-node toward enemy) · `Engage` (enemy visible → `bot_Attack`) ·
`Guard` (CTF defend/follow, holds within `BOT_GUARDING_RANGE=600`) ·
`Flee` (no explicit state — emerges from RL-retreat, ideal-distance backup, danger-dodge).

---

## 2. Enemy search & target selection

### Primary scan — `bot_roam(no_paths=true)` (`bot_ai.c:130-185`)
Iterate `players[]`. A candidate qualifies iff ALL: notarget off, ≠ self, ≠ current enemy, alive, solid,
enemy team, (`bot_client` OR `light_level>5` — gates dark/cloaked humans), not invincible, AND one of:
**we have Quad/invuln** · CTF and they carry our flag · we have no enemy · **their health < current enemy's** ·
they hold blaster (easy prey). Then require `(carrying_flag OR dist<closest) AND visible() AND CanSee()`.

- **Switch-for-better-prey logic**: prefer weaker, flag-carrier, or easy-weapon holder; optionally abort
  current movetarget (`aggr/5*0.2 > random()`).

### Pain-driven retarget — `bot_BetterTarget` (`bot_ai.c:2443-2490`)
On damage: CTF flag-carrier always wins; if current enemy within 512 switch iff `other.health < enemy.health`;
else switch iff `other.health>0`. Also triggers team-help if hurt by stronger foe while on blaster/shotgun.

### FOV/visibility gates
- **`CanSee`** (`bot_ai.c:2570`): `dist<256` → true; else visible iff `|forward−dir| < 1 + (combat/5)`.
  So **higher `combat` skill → wider effective FOV** (threshold 1.2→2.0 ≈ 0°→~120° cone).
- **`CanReach`** (`bot_ai.c:2590`): traces down at midpoint (12-unit steps when `bot_calc_nodes`, 32 fast path)
  to confirm a floor exists (no walking into the void). Water↔water auto-reachable.
- **`CanStand`/`CanJump`**: up-traces (32 / 1 unit) — can I uncrouch / is headroom clear.

**Plugin-only**: full `players[]` iteration, `visible()`/`gi.inPVS` free, **enemy health visible**.
**qbots**: our "player table" is *already* the PVS subset the server sent — the scan is filtered for free;
replace `visible()` with our BSP trace; **enemy health is NOT transmitted by Q2** → replace health comparisons
with a *hit-derived healthiness estimate* (track blood/hitsound/muzzle telemetry per entity). Flag-carrier and
"any live enemy" branches port verbatim.

---

## 3. Give-up / persistence & stuck recovery

### Give-up watchdog (`bot_ai.c:848-887`)
Track `giveup_lastgoal` + `last_reached_trail`. Abandon current goal if
`(last_reached_trail < now−2 AND dist>128) OR (last_reached_trail < now−4)` — i.e. **2 s if >128 u away,
hard cap 4 s**. Relaxed while actively chasing a freshly-sighted enemy (`last_enemy_sight < now−0.5`).
On give-up: blacklist via `ignore_time` (movetarget +3 s, enemy +1 s, goalentity +0.5 s) → `bot_roam(false)`.

> The `BOT_SEARCH_LONG/MEDIUM/SHORT = 4/2/1` tiers (`bot_procs.h:18-20`) are referenced **only in commented-out
> code** (`bot_wpns.c:296-302`) — **not active in v1.01**. The active give-up is the 2 s/4 s watchdog above.

### Sighting / reacquire delay
`last_enemy_sight` refreshed only while `visible(enemy) && inPVS` (0.2 s grace). On losing sight,
`sight_enemy_time=now` → bot must wait `SIGHT_FIRE_DELAY` again before firing (prevents snap-refire on reacquire).

### RL-retreat — the only explicit "flee" (`bot_wpns.c:308-322`)
If enemy is a **human with RL, healthy (>25)** (or we're stuck on blaster/shotgun), and not carrying flag →
**abandon the attack**: clear goalentity (ignore 1 s), enemy (ignore 2 s). Bots flee RL-wielding humans they
can't out-damage.

### Stuck detection (`bot_SuicideIfStuck`, `bot_ai.c:2659-2689`)
Only runs if `checkstuck_time < now−1` and not paused. `move = origin − checkstuck_origin`:
- **`|move| < 4` (4-unit deadband)** → stuck: if `>5 s` cumulative → **suicide** (respawn is recovery);
  else **`botRandomJump`**.
- Else record new `checkstuck_origin` + reset timer.
1 s cadence, 4-unit deadband, jump-then-suicide-after-5 s.

### Inline micro-recovery inside `bot_move` (`bot_ai.c:2268-2378`)
If `|move| < dist*0.1` (moved <10%): try `M_walkmove` straight; if wall (fraction<0.3, near-vertical normal)
→ jump (`velocity.z=310`) if head clear else abort goal; **strafe** ±110° (flip every 3 of 6 s, `slide_time=now+0.5`);
last resort `M_walkmove(yaw ± 180*random, dist*0.5)`.

### "Didn't get closer" goal-abort (`bot_ai.c:2420-2436`)
Heading to goalentity but `|goalvec| >= |oldgoalvec|` for >0.3 s → `ignore_time=+1 s`, null goalentity.

**qbots**: detection ports verbatim (track own origin history). Recovery actuator swaps: `velocity=`/`M_walkmove`
→ `usercmd{forwardmove,sidemove,upmove,BUTTON_JUMP}`. **Suicide-by-respawn is unavailable to a network client**
→ drop it, lean harder on jump/retrail/strafe recovery.

---

## 4. Movement → `usercmd_t` (THE directly-portable part)

`bot_move` (`bot_ai.c:1946-2441`) **builds the exact struct qbots sends over the wire** — this is the single
most portable function in Eraser:

- `ucmd.msec = 100` always.
- `forwardmove`: `sv_maxvelocity` running, `*0.5` walk, `*0.6` swim; set to a *velocity-scale* not a constant.
- `upmove`: drowning (if air trace finds air/ladder) · `NODE_LANDING` ≥64 above · airborne · ladder.
- `angles[PITCH/YAW/ROLL] = ANGLE2SHORT(vectoangles(dir))`.
- Within 32 u of goalentity → snap velocity straight to it ×10 (we set `forwardmove` toward it instead).
- Airborne: clamp `velocity[2] <= 310`; if overshooting goal in Z, kill X/Y to drop onto it.
- **Post-move corrections** (the real steering): ledge-drop detection (jump to goal if +32 Z), **lava/slime
  lookahead** (`drop_dist`=400 default; 200+drop if goal below; 22 if above — trace down, abort+restore if
  open/lava), stair-stick snap, wall-scrape jump assist (`velocity[2]+=13`), stuck-recovery (above),
  goal-reach (`|dz|<16 && horiz<12` → advance to next hop).

**qbots**: port the *decision*; drop `BotMoveThink`/local-Pmove (server does it) and the lava/ledge post-aborts
(server-enforced physics — but we may still want a *prediction* of these to avoid bad goals). Strafe/crouch/
jump/ideal-distance all become `usercmd` population. `M_walkmove` recovery calls → additional `sidemove`
contributions.

### Ideal-distance maintenance (`BOT_IDEAL_DIST_FROM_ENEMY=160`)
When `goalentity==enemy`: `dist<160` → hold; `dist<80` (half) → reverse `ideal_yaw` away + `bot_move` (back up).
Same pattern for guarding `target_ent`.

---

## 5. Combat — aim, lead, jitter, fire timing (PER WEAPON)

> The combat subsystem (`bot_wpns.c`) is the richest vein for qbots. Below: the exact, line-cited mechanics.

### Fire gate (`bot_Attack`, `bot_wpns.c:134-324`) — fires iff BOTH:
1. **Cooldown**: `time since last_fire > fire_interval` (per-weapon; CG/MG/HB = **0** = every frame).
2. **Reaction delay**: aware of enemy longer than `SIGHT_FIRE_DELAY * (5 − combat*0.5)/5`
   = `0.8 × {combat1:0.9, 3:0.7, 5:0.5}` → **0.40 s (combat 5) … 0.72 s (combat 1)**.

Plus visibility gate (`last_enemy_sight > now−0.2` OR `visible && inPVS`) and friendly-fire check (team-trace;
abort if teammate in line).

### Aim-point construction (every weapon)
`start = origin + forward*8`, `start[2] += viewheight−8` (~22 standing). Build `target`. **Apply skill jitter**
(§below). `forward = normalize(target−start)` → angles. **Clamp pitch ±15°** (`:368` etc.) — bots never aim
steeply up/down (a noted GL/RL-lob limit). Call real `fire_*`/`monster_fire_*`.

### Lead prediction — EXACT factors
| Weapon | Projectile speed | Lead time | Notes |
|--------|-----------------|-----------|-------|
| Blaster | 1000 | `dist/1000` | Leads even at `skill<=1` (unique). `:344-348` |
| Machinegun/Shotgun/SSG/Chaingun | hitscan | `0` (or `−0.2` trail at `skill<=1`) | No real ToT; rely on spread+jitter. `:408,492,564,648` |
| Railgun | hitscan | `−0.2` always (trails) | skill-gate commented out; flat 32-u base jitter. `:766-786` |
| Hyperblaster | 1000 | `dist/1000` | `:1089-1093` |
| Rocket | 650 | `dist/650`; **ignores upward velocity** (won't lead a jumper skyward) | `:863-898` |
| Grenade | 600 (fired) | `dist/550` (**over-leads** ~9% to compensate for arc) | `:1005,1042` |
| BFG | 400 (fired) | `dist/550` (**bug**: mismatches 400 speed, over-leads ~38%) | `:1165-1172` |

- **RL ignores upward velocity** (`if vel[2]>0: vel[2]=0`) so it doesn't lead jumpers into the sky.
- **RL ground-aim** (`combat>3`, `:872-896`): trace 64 u down from target; if floor + clear eye→floor trace →
  **aim at the floor for splash** (classic RL-vs-dodger).
- **GL lob** (`:1042-1048`): no ballistic solve — takes direct-aim forward, **pitches up** piecewise:
  `dist≥384` → +15°; `dist<384` → `15*(2*dist/384 − 1)` (ranges −15° downward at dist=0 to +15° at 384).
- **BFG ground-aim** (`dist>200 && grounded`): `target[2] −= 4*combat` (high-combat aims lower for splash).
- **BFG costs 60 cells/shot**, `damage_radius 1000`.

### Skill jitter block (identical skeleton, e.g. MG `bot_wpns.c:423-430`)
```c
if (accuracy < 5) {
    tf = (dist < 256) ? dist/2 : 256;                     // base: ramps 0→128 over 0→256u, caps 256
    tf *= ((5.0 - accuracy)/5.0) * 2;                     // acc1=1.6, acc2=1.2, acc3=0.8, acc4=0.4, acc5=0
    if (enemy is human && !bot) tf *= (1 - vmag(vel)/600); // ⚠️ fast humans jittered LESS
    VectorAdd(target, crandom()*tf, crandom()*tf, crandom()*tf*zscale);
}
```
- **acc=5 → zero jitter (perfect aim)**; acc=1 → 1.6× base. At 512 u: acc4≈102 u, acc3≈205 u, acc1≈410 u std-dev-ish.
- **Human-vs-bot asymmetry** (note sign): MG/SG/SSG/CG/RL/GL/BFG use `*(1−vmag/600)` (**more accurate vs
  moving humans**, a quirk); Blaster inverts `0.5+vmag/600`; Hyperblaster uses `vmag/600` (jitter only when moving).
- **Z-scale**: MG `0.1`; everything else `0.2`. Bots miss mostly horizontally, rarely high/low.

### Per-weapon special behavior (`bot_wpns.c`)
- **RL self-damage avoidance** (`:928-962`): temporarily moves enemy to predicted pos, traces a 130-u box
  forward from muzzle; if blocked + bot healthy → **walk backwards** (or `botPickBestCloseWeapon`), abort shot.
- **RL peak-of-jump abort** (`:842-846`): `combat>3` and airborne upward → don't fire (wait for apex).
- **SSG fan optimization**: two `fire_shotgun` at yaw +5°/−10° (15° spread), `count/4`, doubled per-pellet dmg.
- **CG wind-up**: `machinegun_shots` ramps 0→3 over ~0.3-0.5 s, then one bullet of `3*shots` dmg (CPU opt).
- **Chaingun uses half spread** (`*0.5`).

**Plugin-only**: `enemy->s.origin`/`velocity` exact & free; `infront()`, ground-aim traces, RL self-trace use `gi.trace`;
enemy health for RL-retreat. **qbots**: entity origin from `svc_packetentities` deltas; **velocity only sometimes
sent → derive by differencing origins across frames** (low-pass filter — noisy at 10-20 Hz). Lead math ports
verbatim with our derived velocity; add a small `tf` floor to compensate for snapshot quantization (systematic
lead error, not random). RL self-damage trace → our BSP trace from muzzle along aim ~130 u; we just withhold
`BUTTON_ATTACK` for a frame. Pitch-clamp and GL lob port directly.

---

## 6. Weapon selection (`botPickBestWeapon` / Far / Close, `bot_wpns.c:1225-2113`)

**Hardcoded priority lists** keyed on "has weapon AND has ammo" — **no scoring function, no range/enemy-state
weighting in the pick itself**; range awareness is injected by *callers* (each `bot*` wrapper calls Far/Close
after firing based on `dist`). Triggered on ammo/weapon pickup, on running dry, and from wrappers at range.

| Pick | Priority (top→bottom) | When called |
|------|----------------------|-------------|
| **Best** (general) | **fav_weapon** → BFG → Chaingun → Hyperblaster → RL → Railgun → MG → SSG → GL → Shotgun → Blaster | pickup, dry |
| **Close** (no splash) | CG → SSG → HB → MG → BFG → RG → GL → SG → RL → Blaster | RL self-range, GL `dist<radius` |
| **Far** (snipe) | CG → **Railgun** → MG → HB → BFG → RL → SG → SSG → GL → Blaster | SG/SSG/RL/GL `dist>700`; BFG `>1000` |

- **fav_weapon overrides everything** — a bot whose fav is shotgun grabs it over BFG.
- Every switch: set `newweapon`, `bot_fire` fn ptr, `last_fire = time + BOT_CHANGEWEAPON_DELAY` (**0.9 s**
  switch lockout), set `fire_interval` (halved under Haste), `ShowGun`.
- BFG is **#2** default (aggressive). Quad-only-BFG block is commented out.

**qbots**: lists port verbatim as a per-weapon preference vector; inputs ("have weapon X", "have ammo Y")
come from our client `stat`/inventory. **The 0.9 s switch lockout is server-enforced** (`WEAPON_SWITCHING`) —
qbots must withhold `BUTTON_ATTACK` for 0.9 s after sending a `weapon` impulse. fav_weapon is pure config.

---

## 7. Danger avoidance (rockets/grenades) — `avoid_ent`

> **Great discovery for qbots** (→ Plan 07). Eraser actively dodges projectiles.

### Population (`g_weapon.c`) — projectile entities tag nearby bots
- **Grenades** (`:467-474`, every bounce frame): for each bot within **256 u** → `bot.avoid_ent = grenade`.
- **Rockets** (`:632-650`, rocket think 0.3 s): for each bot with **`combat>=4`** (skilled dodgers only) within
  **300 u** (axial) AND heading roughly toward bot (`entdist − path_along < 75`) → `bot.avoid_ent = rocket`.

### Consumption (`bot_move`, `bot_ai.c:2032-2062`) + `botJumpAvoidEnt` (`bot_nav.c:357-450`)
If grounded/in-water and `avoid_ent` set: if `avoid_dir_time>now` use cached `avoid_dir`; else call
`botJumpAvoidEnt`:
- Bail (2=ignore) if: teammate (friendly-fire-off), danger >300 u, `movetarget` within 256 (don't dodge mid-pickup),
  not `visible`.
- Adopt `avoid_ent->owner` as enemy if we have none.
- Perpendicular dodge `trail_vec = (dir[1],dir[0],0)` from self→danger; pick side already-on (random <0.5 if 200-300 u);
  add ±fwd/back noise.
- Grounded: trace 200 u along dodge + 512 down; if landing safe (no lava/slime) → **dodge-jump**
  (`velocity = trail_vec*BOT_RUN_SPEED, velocity.z=300`); else **strafe-away** (`avoid_dir=trail_vec, +0.3 s`).
- In water: cache `avoid_dir` 1 s (can't jump).

**Plugin-only** ⚠️: populator iterates `players[]` and reads projectile `s.origin`/`velocity` — server-side
projectiles the external client may **never see** (behind you / out of PVS). **qbots's danger set is a strict
subset**: only projectiles the server sends as `svc_packetentities` (in our PVS). When we *do* see one
(classify via configstring modelindex, track velocity), run the same `botJumpAvoidEnt` geometry on our BSP trace.
**Shooter is not sent on the wire** → infer from recent `svc_sound`/`svc_temp_entity` muzzle events near the
projectile origin. Gate on our own `combat>=4` tier.

---

## 8. Items & pickups (`bot_items.c`)

### Rating: `dist_divide` weighting (`RoamFindBestItem`, `bot_items.c:100-326`)
Iterate category linked-list; score each item; **best = min `effective_dist = path_dist / dist_divide`**.
Higher `dist_divide` = more desirable (looks "closer").

- **Skip** if not `SOLID_TRIGGER` (respawning), `entdist > 2000` (except flags/weapons), or on `ignore_time`.
- **Direct-grab shortcut**: `dist<384` AND `visible_box` AND `CanReach` → set movetarget, return. Route early-accept `<128`.

| Category | `dist_divide` rules |
|----------|--------------------|
| **Ammo** | `(2*hasWeaponForAmmo + (dist<256?1:0)) × 2*canPickup × 3*(onBlaster?1:0)` — only aggressively pursued when stuck on blaster; 0 if no weapon uses it |
| **Bonus/armor** | `botCanPickupArmor()` score (§below) |
| **Bonus/Quad** | `4 + 4*(skill−1)` if skill>1, **`×2 if quad_freak`` |
| **Bonus/other powerups** | ⚠️ **default 1 (WANT_KINDA)** — Eraser under-rates invuln/mega-as-bonus/silencer (a gap qbots should fix) |
| **Bonus/CTF flag** | `6`, `+100` if armed, **`+9999` to cap** if carrying both |
| **Weapons** | RL/CG/Rail = `4`; BFG = `3 +3 if players>4`; fav_weapon `+=3`; on-blaster `+=4` |
| **Health** | mega/stim (`count==100`) = `4`; skip if `health>90`; skip route-seek if `health>50` |

### Armor (`botCanPickupArmor`, `:334-405`)
Returns 0=skip, else desirability (used as `dist_divide`). **Shard=1**. **No armor=4** ("any armour is FUCKING GOOD").
Has-armor: salvage math mirroring real `Pickup_Armor`; `((newcount−cur)/50)*3 + 1`; return 0 if already maxed for type
(don't waste). Values: jacket 25/50@30%, combat 50/100@60%, body 100/200@80%.

### Health (caller amplifies, `bot_ai.c:244-265`)
`health<10` → instant-take; `health<50` → **÷3** (treat 3× closer, strong flee-to-health); 50-90 grab-if-near; >90 ignore.

### Hover-for-respawn (`bot_ai.c:646-655`)
When current movetarget item is taken, **keeps goal + waits** iff `WANT_SHITYEAH` AND `nextthink <= now+4`
(respawn within 4 s) — camps a high-value item's respawn. Keyed off WANT tier, **not** `camper` flag.

### Skill modulation via `aggr` (`bot_ai.c:227`)
Item search entered iff `onBlaster` OR `(0.3*aggr/5) < random()` → **higher aggr → searches items LESS** (fights instead).
Chase-abort: `(aggr/5)*0.2 > random()` (`:164`).

**Plugin-only**: full item linked-lists + exact respawn state. **qbots**: **cannot enumerate items** → build a per-bot
"observed item world": cache item origins from PVS `svc_packetentities` deltas + BSP-known spawns from configstrings;
track **respawn timers ourselves** (seed delay on first observe / on pickup event). The **rating logic is pure
inventory+distance math — ports verbatim** once we have the observed-item input. Fix Eraser's gaps: explicit values
for invuln/mega/silencer; author real camping (Eraser's `camper` is dead code — §9).

### WANT-tier bucketing (`botSetWant`, `:81-89`)
`dist_divide<=1`→KINDA, `<=3`→YEH_OK, else→SHITYEAH.

---

## 9. Navigation / route-node graph (`bot_nav.c`, `p_trail.h`, [INF] NavLib)

### Graph model
- **Node = an `edict_t`** in global `trail[TRAIL_LENGTH=750]` (`trail_head` high-water; `>=500` = "learned enough").
- **`paths[24]`** = neighbor node *indices* (degree cap; overflow overwrites slot 0).
- **`routes_t`** (`g_local.h:1113`): `route_path[750]` (**next-hop** toward each target) + `route_dist[750]` (**cost**)
  = classic **all-pairs next-hop routing table**, `O(N²)` shorts, computed by `CalcRoutes()` [INF].
- **Spatial hash** `trail_portals[25][25][196]`: 24×24 X/Y grid over `MAX_MAP_AXIS=5000` (~208 u cells),
  each node in **≤4 cells** (blur boundaries); `num_trail_portals` occupancy; for `ClosestNodeToEnt`/`matching_trail` [INF].

### Node types (when dropped)
- **NODE_NORMAL** — dropped by `CheckMoveForNodes` [INF] as a human moves (`p_client.c:2044`, `bot_calc_nodes=1`).
- **NODE_PLAT** — plat bottom (`g_func.c:362`); a NODE_NORMAL at plat top (`:372`) is manually spliced into
  `paths[]`; one-shot via `frags` guard.
- **NODE_LANDING** — jump destination "not visible from any node except the jumping node" [INF]; source jump node
  stores launch `velocity`/`goalentity`=landing. Movement reads `goalentity->velocity` as the launch vector.
- **NODE_BUTTON** — `button_touch` one-shot (`g_func.c:859`).
- **NODE_TELEPORT** — teleporter dest (`g_misc.c:1814`), dedup via `matching_trail`; zero-cost edge [INF].
- **NODE_GRAPPLE** — jump node launched by firing the grapple (`bot_ai.c:1441-1457`).

### Edge/visibility predicates (both `gi.trace`-based, plugin-only)
`visible()` (point LOS), `visible_box()` (**`gi.inPVS` short-circuit** then box trace — canonical node-node & bot-item test),
`CanReach()` (floor-exists trace, 12/32 u steps). **qbots**: `inPVS` we get for free (server only sends PVS entities) —
skip the explicit check; `visible_box`→our BSP trace; `CanReach`→BSP trace down for floor.

### Path following (`PathToEnt`/`ClosestNodeToEnt` [INF], usage visible)
- `PathToEnt(self, target, ...)` returns dist; sets globals `PathToEnt_Node` (**next visible hop from self**)
  + `PathToEnt_TargetNode`. Returns −1 if no route. The **single** pathing primitive (~40 call sites).
- Node-advance protocol (`bot_ai.c:1738`): `if (PathToEnt(self,goal)>-1) goalentity = PathToEnt_Node;`
  on `bot_ReachedTrail` (`|dz|<16 && horiz<12`) → `PathToEnt` again for next hop.
- Give-up: `last_reached_trail < now−2 & >128u`, OR `< now−4` → blacklist + re-roam (§3).

### Movement think → see §4.

### `botRoamFindBestDirection` (`bot_nav.c:96-176`) — fallback heading
When `ClosestNodeToEnt == −1`: fan-out trace in **7 directions at 45°** (skips 4,5), `TRACE_DIST=256`, lifted by
`STEPSIZE=24`; score `= fraction*256`; **halve score if down-trace `fraction>0.4`** (avoid long falls); skip all liquid;
pick best yaw; early-out if any scores full 256.

### `botRandomJump` (`:178`): grounded + `last_jump<now−0.5` + `CanJump` → `botRoamFindBestDirection` then
jump `velocity=forward*300*(0.4..1.0), .z=300`.

### Plat/button/teleporter routing
- **Plat** (`bot_MoveAI:961-1050`): riding up → `bot_plat_pausetime=now+0.3` + walk to center; plat not at
  `STATE_BOTTOM` → pause +0.5 s + walk backward. Boarded-reach: only "reached" when `groundentity==plat && STATE_TOP`.
- **Button** (`botButtonThink`, `bot_ai.c:2691`): scans players within 900 u; if routable (<1200) claims bot
  (`plyr->activator=button, goalentity=hop`); abort after 5 s.
- **Teleport**: NODE_TELEPORT = zero-cost-edge hint [INF].
- **Grapple shortcut** (`:1796-1831`): if next-next node aligns + trace hits geometry 0.4-1.0 → fire grapple to cut corner.

**qbots**: `trail_portals` 24×24 hash ports directly (cell=map_axis/24). `route_path`/`route_dist` all-pairs table is
`O(N²)` wasteful → **replace with on-the-fly A\*** on our BSP nav graph (Plan 05). Special node types port as node/edge
metadata; we detect plat/button/teleport state from `svc_packetentities` deltas + `svc_sound` (not `moveinfo.state`).
Node-drop learning by observing humans is impossible over UDP → **pre-author from BSP** (preferred) + the §13
danger/popularity overlay (the realistic analog).

### ⚠️ `camper` is DEAD CODE in v1.01
`camper` is **set/randomized** (`bot_misc.c:409`) and **read from bots.cfg** (`:620`) but **NEVER read** by any AI/item/nav
code. The "find a dark corner with fav_weapon" behavior is **not implemented**. Real loitering comes only from
hover-for-respawn (§8) + fav_weapon weighting. **Do NOT port a `camper` FSM from Eraser — author one fresh if wanted.**

---

## 10. Dynamic route learning (the withheld engine + what we infer)

[INF] `CheckMoveForNodes` (`p_client.c:2042`, **real players only — `!bot_client`**, gated by `bot_calc_nodes=1`)
reads per-player drop state (`last_max_z`, `last_groundentity`, `jump_ent`, `duck_ent`, `last_trail_dropped`) and
decides spacing + node type. So **learning = observing human/bot movement server-side** (matches v0.4 changelog).

- **`CalcRoutes(node_index)`** [INF]: single-source row (next-hop + cost) into `trail[node]->routes`, called
  **incrementally** on every node/edge change (plat `g_func.c:390`, teleport `g_misc.c:1885`). Almost certainly
  **Dijkstra/BFS over `paths[]` adjacency** (not Floyd-Warshall — doesn't fit lazy per-node recompute).
- **`OptimizeRouteCache()`** [INF]: budgeted background optimizer (`last_optimize`, `optimize_marker` cursor,
  `bot_optimize=1200` budget), called **once per frame** (dedicated: `g_main.c:1132`; listen: `p_client.c:1950`) —
  scans a slice so no frame stalls.
- **Auto-disable**: `CheckNodeCalculation` (`g_spawn.c:518`) — at `nodes_done || trail_head>=500` → force
  `bot_calc_nodes=0`; plus a 300 s (5 min) scheduler (`g_main.c:647`).
- **`.rt2` format: NOT recoverable** (no `fread`/`fwrite` of routes, no literal in any released `.c`/`.h`).
  `loaded_trail_flag` is read at `bot_spawn.c:145` but written only inside `p_trail.c`. We don't need it — BSP is our truth.

**qbots**: topology comes from our BSP (static). The portable *ideas*: (1) spatial-hash dedup, (2) incremental
single-source recompute + budgeted background optimizer (only needed if we add a dynamic edge-weight overlay),
(3) special node taxonomy, (4) **observed-traffic/danger augmentation** — §13, the genuinely new capability.

---

## 11. Skill & personality config (`bots.cfg`, `bot_misc.c`)

### `bots.cfg` format (`ReadBotConfig`, `bot_misc.c:649-808`; `ReadBotData`, `:547-623`)
Path `./<game>/bots.cfg`; missing = fatal. `#`=comment. `[b`=bot, `[t`=team, `[v`=view-weapon. A bot record:
```
"<name>"  "<skin/male/razor.pcx>"  <accuracy> <aggr> <combat> <fav_weapon_num> <quad_freak> <camper> <avg_ping>
```
- accuracy/aggr/combat: **int → float, range 1-5**.
- fav_weapon num: `0`=BFG,`2`=SG,`3`=SSG,`4`=MG,`5`=CG,`6`=GL,`7`=RL,`8`=Rail,`9`=HB (`1`→RL). Default template = railgun.
- quad_freak/camper: bool; avg_ping: display-only.
- A hardcoded **"Eraser" bot** (acc5/aggr0/combat5/rail/quad_freak1) is always prepended; the name "Eraser" is reserved.
- Result: linked list `botinfo_list`, count `total_bots`; each `bot_info_t` has `ingame_count` (rotate personalities, avoid double-spawn).

### Skill scaling — `AdjustRatingsToSkill` (`bot_misc.c:1065-1085`)
Called **once at spawn** (`bot_spawn.c:298`). `skill_level` = engine `skill` cvar (0-3). Offset from 1-5 template, clamp [1,5]:
```
accuracy = tmpl.accuracy + (skill_level-1)*2.5      // +5 at skill3
combat   = tmpl.combat   + (skill_level-1)*2.5      // +5 at skill3
aggr     = tmpl.aggr     - (skill_level-1)*2.0      // -4 at skill3  (NOTE: subtract — high skill = patient, stocks up first)
```
At skill3: a template-5 bot maxes accuracy/combat, drops aggr to 1 → accurate, combat-capable, **patient**.

### ⚠️ `bot_auto_skill` is DECLARED but NOT WIRED in v1.01
Registered (`g_save.c:195`) but **never tested in any `if`**; `skill_level` is **set once at spawn** and never
incremented/decremented. The kill-raises/death-lowers skill the changelog implies is **stubbed/unreleased**.
**qbots can implement it** (we observe our own kills/deaths via `svc_print`) — don't assume Eraser's works.

### How ratings drive behavior (cross-ref)
| Rating | Effect | Where |
|--------|--------|-------|
| **accuracy** | jitter magnitude `(5−acc)/5*2` (acc5=perfect) | every `bot*` wrapper |
| **combat** | reaction `0.8*(5−combat*0.5)/5`; FOV width `1+combat/5`; strafe (combat1=none); jump cadence; crouch-at-range (combat>4); RL/BFG ground-aim (combat>3); rocket-dodge gate (combat>=4) | `bot_Attack`, `CanSee`, `bot_wpns.c` |
| **aggr** | item-search freq (higher=searches less); chase-abort prob | `bot_ai.c:164,227` |
| **fav_weapon** | top priority in `botPickBestWeapon`; `+=3` item rating | `bot_wpns.c`, `bot_items.c` |
| **quad_freak** | `×2` Quad item rating | `bot_items.c` |

**qbots**: `bots.cfg` → per-bot TOML/serde struct. Adopt the same 7-field personality with 1-5 ranges (combat AI
is calibrated to them). Port `AdjustRatingsToSkill` verbatim. Port jitter/reaction/FOV/strafe formulas verbatim.
`skill->value`'s lead-enable gate → make it per-bot/per-server config in qbots (we don't set the Q2 cvar).

---

## 12. Spawn / fleet supervisor policy (`bot_spawn.c`, `g_main.c`)

### `spawn_bot(name)` (`:214-348`)
`GetBotData` (random unused personality if name=NULL, skip in-use, skip "Eraser") → `G_SpawnBot` (free slot via
`bot_GetLastFreeClient`, bail if `!deathmatch` or **no route table**) → wire client → build userinfo (`name`,`skin`,`hand=2`)
→ **`ClientConnect(bot, userinfo, false)` — bot enters through the SAME path as a human** (server treats it as real) →
`SelectSpawnPoint`, place, fov 90, `MZ_LOGIN` multicast → copy stats + `AdjustRatingsToSkill` → model/skin/netname/
`ShowGun`/`KillBox`/`walkmonster_start`. 30% chance: greeting thinker 1.5-2.5 s.

### Fleet supervisor (`G_RunFrame`, `g_main.c`) — a complete, battle-tested spec qbots should port
- **Spawn to target `bot_num`**: `(spawn_bots>0 || bot_count<bot_num) && last_bot_spawn<now−0.5` → `spawn_bot(NULL)`
  if `num_players < maxclients − bot_free_clients`. **Throttled 1 bot / 0.5 s**.
- **`bot_name <name>` cvar** → spawn that specific bot, clear cvar.
- **Kick when crowded**: `bot_count>0 && num_players > maxclients − bot_free_clients` → disconnect **lowest-scoring** bot.
- **`bot_free_clients`**: reserve slots for humans.
- **Map-change respawn**: names saved to `respawn_bots[64][256]`; re-spawn after `level.time>8` (8 s grace for humans),
  max 2/frame, 0.3 s apart.
- **`respawn_bot`** (`:104`): teleport-effect, `PutClientInServer`, **clear enemy/goal/movetarget, reset weapon to blaster
  (`bot_fire=botBlaster`, `fire_interval=0.6`), clear stuck/avoid/flagpath timers** — a good checklist for our respawn state machine.
- **`botRemovePlayer`**: `health=0` (others stop targeting), remove from `players[]`, decrement counters, clear `enemy`
  pointers on bots that hunted it.

**qbots**: mechanism-level spawn is irrelevant (we're already UDP clients, Plan 03/07). **Port the supervisor *policies***:
spawn-to-target, 0.5 s throttle, lowest-score-kick-when-crowded, `bot_free_clients` reservation, map-change respawn list,
8 s reconnect grace. These map directly onto the `qbots` binary's supervisor task (Plan 09).

---

## 13. ★ GREAT DISCOVERIES for qbots (→ implementation plans)

These are the highest-value, most-portable wins distilled from the above. **Plan 07** ports Eraser's known-good
combat/nav/danger/skill algorithms with the exact numbers; **Plan 08** builds something Eraser literally *cannot*
(dynamic risk-weighted routing for an external client — the realistic analog of its withheld dynamic-learning engine).

### ★ A — Combat aim/lead/jitter (→ Plan 07)
The **exact per-weapon lead factors** (`dist/speed` with the documented quirks: RL ignores upward-V, GL over-leads
to `dist/550` + piecewise pitch lob, BFG misuses `dist/550`), the **jitter formula** (`(5−acc)/5*2` scale, 256-u base,
human-vs-bot asymmetry, z-scale 0.1/0.2), **fire intervals**, **reaction delay** (`0.8*(5−combat*0.5)/5`), and
**±15° pitch clamp** — all pure arithmetic over state we have. The biggest single quality lever for "make our bots
hit like Eraser."

### ★ B — Projectile danger avoidance (→ Plan 07)
The `avoid_ent` system: detect rockets/grenades in our PVS (subset of Eraser's), perpendicular dodge-jump or
strafe-away, `combat>=4` gate, `botJumpAvoidEnt` geometry on our BSP trace. Eraser bots *dodge* — most bots don't.

### ★ C — Per-bot skill/personality (→ Plan 07)
`bots.cfg` 7-field personality + `AdjustRatingsToSkill` formula + the full rating→behavior mapping (accuracy→jitter,
combat→reaction/FOV/strafe/dodge, aggr→item-search-vs-fight, fav_weapon, quad_freak). Calibrated to 1-5 ranges.

### ★ D — Observed danger/popularity heatmap nav overlay (→ Plan 08) — NOVEL
Eraser's route graph is **static topology** (learned once). qbots can do better: **augment our static BSP nav graph
with runtime-observed edge weights** — (1) **danger weight** (increment on observed death near a node via `svc_print`
obituaries, decay over time — the external-client analog of Eraser's rocket/grenade avoidance applied to *routing*),
(2) **popularity weight** (count observed player traffic in PVS → prefer well-traveled routes). This is a dynamic
risk-aware pathing layer Eraser structurally cannot build (it has no "observation" — it owns the world). A genuinely
new capability, not a port.

### ★ E — Eraser FSM shape + give-up/stuck watchdogs (→ fold into Plan 06)
The 2 s/4 s goal give-up, 4-unit/5-s stuck→suicide (we use jump/retrail instead), SIGHT_FIRE_DELAY reacquire-delay,
RL-retreat, ideal-distance (160/80). These refine Plan 06's FSM with battle-tested thresholds.

### F — Eraser's gaps qbots should NOT inherit
- **Under-rated non-Quad powerups** (`dist_divide=1` default) — author explicit values for invuln/mega/silencer.
- **`camper` dead code** — author camping fresh if wanted (camp node near fav-weapon/quad, good cover/LOS, dwell).
- **`bot_auto_skill` stubbed** — implement kill/death skill adjustment ourselves.
- **BFG lead bug** (`dist/550` vs 400 speed) — use correct `dist/400`.
- **Suicide-by-respawn stuck recovery** — unavailable over UDP; use jump/retrail.

---

## 14. Plugin-only vs portable — summary table

| Eraser mechanism | Plugin API | qbots replacement |
|------------------|-----------|-------------------|
| Enemy LOS + PVS | `visible()`, `gi.inPVS()` | BSP trace + (PVS already enforced by what server sends) |
| Enemy exact origin/velocity | `enemy->s.origin`/`velocity` | entity delta from `svc_packetentities`; **derive velocity** by frame-differencing (low-pass) |
| Enemy health | `enemy->health` | **NOT available** — hit-derived estimate; drop health-conditional behaviors |
| RL self-damage / ground-aim trace | `gi.trace` | our BSP trace from muzzle |
| Physics oracle (`Pmove` same-frame) | `gi.Pmove` | drop — server runs physics; we send `usercmd` and read next frame |
| Direct velocity writes / `M_walkmove` | `self->velocity=`, `M_walkmove` | `usercmd{forwardmove,sidemove,upmove,BUTTON_JUMP}` |
| Full item enumeration | `*_head` linked lists | per-bot observed-item cache (PVS deltas + BSP spawns + self-tracked respawn timers) |
| Item respawn state | `->solid`, `->nextthink` | self-tracked timers (seed on observe/pickup) |
| `route_path`/`route_dist` all-pairs | NavLib | on-the-fly **A\*** on our BSP nav graph |
| Spatial hash `trail_portals` | NavLib | port directly (cell = map_axis/24) |
| Danger populator (rockets/grenades) | `g_weapon.c` iterates `players[]` + projectile ents | only PVS-sent projectiles; infer shooter from `svc_sound`/`svc_temp_entity` |
| Inventory | `pers.inventory[]` | client `stat`/inventory from server |
| Switch delay 0.9 s | `last_fire=time+0.9` | withhold `BUTTON_ATTACK` 0.9 s after weapon impulse |
| Self-suicide when stuck | `T_Damage(self,...)` | **drop** — lean on jump/retrail |
| Spawn lifecycle | `G_Spawn`/`ClientConnect`/`KillBox` | irrelevant (we're UDP clients) — **port supervisor *policies*** |
| `bots.cfg` / `AdjustRatingsToSkill` | file I/O + arithmetic | TOML/serde + verbatim formula |

---

## 15. Quick-reference constants (exact)

| Constant | Value | Source |
|----------|-------|--------|
| `bot_frametime` / move budget | `0.1` s / `30` u (`300*0.1`) | `bot_ai.c:476` |
| `BOT_RUN_SPEED` / `BOT_STRAFE_SPEED` | 300 / 200 | `bot_procs.h:6-7` |
| `BOT_IDEAL_DIST_FROM_ENEMY` / hold / back-up | 160 / — / 80 | `bot_ai.c:1706-1720` |
| `BOT_GUARDING_RANGE` | 600 | `bot_procs.h:14` |
| `SIGHT_FIRE_DELAY` | 0.8 s (scaled `*(5−combat*0.5)/5`) | `bot_procs.h:22`, `bot_wpns.c:167` |
| `BOT_CHANGEWEAPON_DELAY` | 0.9 s | `bot_procs.h:91` |
| Fire intervals (s) | Blaster .6 · RL .8 · GL .9 · RG 1.5 · SG/SSG 1 · HB/CG/MG 0 · BFG 2.8 | `bot_procs.h:80-89` |
| Give-up | 2 s if >128u away, hard 4 s | `bot_ai.c:857-858` |
| Stuck | 4-u deadband, 1 s cadence, jump then suicide @ 5 s | `bot_ai.c:2667-2673` |
| `bot_ReachedTrail` | `|dz|<16 && horiz<12` | `bot_ai.c:1867-1870` |
| `botRoamFindBestDirection` | 7 dirs @45°, `TRACE_DIST=256`, lift `STEPSIZE=24`, halve if down-trace>0.4 | `bot_nav.c:96-176` |
| `botRandomJump` | horiz `300*(0.4..1.0)`, z=300, 0.5 s gate | `bot_nav.c:178-222` |
| Item direct-grab | `dist<384` + LOS + reach; route early-accept `<128`; far-skip `>2000` | `bot_items.c:121,244,290` |
| Item value `dist_divide` | RL/CG/Rail=4 · BFG=3(+3 if >4 plyrs) · Quad=4(+4*(sk−1),*2 quad_freak) · mega=4 · fav+=3 · onBlaster+=4 · shard=1 · no-armor=4 | `bot_items.c` |
| Health | <10 instant · <50 ÷3 · 50-90 grab-if-near · >90 ignore | `bot_ai.c:244-265` |
| `trail_portals` | 25×25×196, 24×24 grid over 5000u (~208u cells), ≤4 cells/node | `p_trail.h:30-40` |
| `TRAIL_LENGTH` / learn-cap / disable | 750 / `trail_head>=500` / 300 s | `g_local.h:783`, `g_spawn.c:520`, `g_main.c:647` |
| Skill remap | acc/cmb `+(sk−1)*2.5`, aggr `−(sk−1)*2.0`, clamp[1,5] | `bot_misc.c:1068-1084` |
| Jitter | base `min(dist/2,256)`, scale `(5−acc)/5*2`, z-scale MG 0.1 / else 0.2 | `bot_wpns.c:423-430` |
| Lead | blaster/HB `dist/1000` · RL `dist/650` (ignores up-V) · GL `dist/550`+lob · BFG `dist/550`(bug) | `bot_wpns.c` |
| Danger dodge | grenade 256u · rocket 300u+`combat>=4`+heading-toward | `g_weapon.c:467,632` |
| Spawn throttle / kick / grace | 0.5 s/bot · lowest-score-kick · 8 s map-change grace | `g_main.c:810,938,666` |

---

## Key files (absolute)
- `…/src/bot_ai.c` — brain FSM, enemy search, give-up, stuck, movement decision (`bot_run`, `bot_roam`, `bot_MoveAI`,
  `bot_move`, `bot_ReachedTrail`, `bot_SuicideIfStuck`, `CanSee`/`CanReach`, `botButtonThink`).
- `…/src/bot_wpns.c` — combat: fire gate, per-weapon aim/lead/jitter, weapon-select (`bot_Attack`, `bot_FireWeapon`,
  all `bot*` wrappers, `botPickBest*`).
- `…/src/bot_nav.c` — `BotMoveThink` (pmove driver), `botRoamFindBestDirection`, `botRandomJump`, `botJumpAvoidEnt`.
- `…/src/bot_items.c` — `RoamFindBestItem`, `botCanPickupArmor/Ammo`, item rating.
- `…/src/bot_spawn.c` — `spawn_bot`/`respawn_bot`/`botDisconnect`/`botRemovePlayer`.
- `…/src/bot_misc.c` — `ReadBotConfig`/`ReadBotData` (bots.cfg), `AdjustRatingsToSkill`, `GenerateBotData`,
  `FindVisibleItemsFromNode`, `GetWeaponForNumber`.
- `…/src/g_local.h:938-946` (`bot_stats_t`), `:1113-1117` (`routes_t`), `:1270-1352` (bot+node fields).
- `…/src/p_trail.h` — NavLib decls + `NODE_*` enum + portal constants.
- `…/src/g_weapon.c:467,632` — danger-avoidance populators. `g_main.c` — fleet supervisor. `g_func.c`/`g_misc.c` — special-node drops.
- **NOT recoverable**: `p_trail.c` (`CalcRoutes`/`OptimizeRouteCache`/`PathToEnt`/`ClosestNodeToEnt`/
  `CheckMoveForNodes`/`matching_trail`/`.rt2` I/O) — unreleased per NDA.
