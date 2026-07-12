# Xonotic "havocbot" — distilled for the qbots port

> **Sources** (read in full, 2026-07-11): `vendor/xonotic/data/xonotic-data.pk3dir/qcsrc/server/bot/default/`
> `{aim,bot,navigation,waypoints,scripting}.qc/.qh`, `havocbot/{havocbot,roles}.qc/.qh`, `cvars.qh`,
> cvar defaults from `xonotic-server.cfg`, item eval funcs from `qcsrc/server/items/items.qc`.
> The data repo is a **sparse clone (qcsrc only)** inside the umbrella `vendor/xonotic` checkout —
> re-fetch with `git clone --depth 1 --filter=blob:none --sparse <xonotic-data.pk3dir url> && git sparse-checkout set qcsrc`.
>
> All paths below are relative to `qcsrc/server/bot/default/` unless noted.
> `[PORT]` = ports cleanly to our external client. `[ADAPT]` = needs adaptation (noted how). `[DROP]` = not portable / not worth it.
>
> Feeds Plans 58–62. Sibling distillations: `quake3.md`, `eraser.md`, `brains/3zb2_brain.md`.

---

## §0 Code-layer map

| File | What lives there |
|------|------------------|
| `bot.qc` | server-frame housekeeping, per-bot think throttle, ping simulation, skill loading, strategy token |
| `havocbot/havocbot.qc` | the brain: `havocbot_ai`, `havocbot_movetogoal` (steering), enemy/weapon choice, dodge, bunnyhop, keyboard emu |
| `havocbot/roles.qc` | DM role = goal-rating functions (items / enemy players / wander waypoints) |
| `navigation.qc` | goal stack, rating session, Dijkstra `navigation_markroutes`, `tracewalk`, touch-pop, unstuck queue, danger map update |
| `waypoints.qc` | waypoint linking, travel-time cost model, special waypoint types, `.waypoints`/`.cache`/`.hardwired` file formats, autowaypointing |
| `aim.qc` | the aim dynamical system: error offset, filter cascade, mouse-think, turn-rate, fire cone, trajectory lead |
| `cvars.qh` | every `bot_ai_*` knob |

## §1 Architecture — three nested cadences

1. **Server frame** — `bot_serverframe()` (`bot.qc:687`): loads waypoints once (`bot.qc:777-782`), updates the **danger map** every `bot_ai_dangerdetectioninterval` (0.25 s), 64 waypoints per pass (`bot.qc:813-820`), rotates the **strategy token**.
2. **Bot think** — `bot_think()` (`bot.qc:62`), throttled: interval = `max(0.01, bot_ai_thinkinterval * min(14/(skill+bot_aiskill+14), 1))` (`bot.qc:75`) — 0.05 s default, shrinking with skill. Simulated ping: `bound(0, 0.07 − bound(0,(skill+pingskill)*0.005,0.05) + random()*0.01, 0.65)` (`bot.qc:104`). Dead → press jump to respawn (`bot.qc:147`). Then the pluggable `this.bot_ai(this)` (`bot.qc:160`).
3. **Brain** — `havocbot_ai()` (`havocbot.qc:35`), installed by `havocbot_setupbot` (`havocbot.qc:1763`). Per tick: scripting → **goal re-rating only if holding the strategy token** (`havocbot.qc:52`) → `havocbot_chooseenemy` → `havocbot_chooseweapon` every 0.5 s → weapon `wr_aim` fires → `havocbot_movetogoal` → fallback flat aim at goal (`havocbot.qc:174-181`).

**Strategy token** (`bot.qc:784-811`): exactly one bot per server frame runs the expensive goal-rating flood; preference to bots with an empty goal stack, else round-robin. `[PORT]` — fleet-level amortization scheduler; directly useful in our N-bot process.

## §2 Goal stack + rating (navigation.qc / roles.qc)

- Route = explicit 32-deep stack of goals; `goalcurrent, goalstack01..31` (`navigation.qh:19-26`), `goalentity` = final goal (`navigation.qh:34`). Overflow drops the deepest — "know the first 32 steps, then recalculate" (`navigation.qc:795-800`). `[PORT]` as a `Vec`.
- **Rating session**: `navigation_goalrating_start` (`navigation.qc:1831`) resets `bestrating`, picks a ≤50 qu source waypoint (100 qu while bunnyhopping, `navigation.qc:1791`), runs `navigation_markroutes` (one Dijkstra flood). Role code calls `navigation_routerating(this, candidate, f, rangebias)` per candidate; `navigation_goalrating_end` (`navigation.qc:1845`) walks the Dijkstra parent chain (`.enemy`) pushing the route (`navigation.qc:1541-1549`). Nothing reachable → `AI_STATUS_STUCK` (`navigation.qc:1860-1867`).
- **THE formula** (`navigation.qc:1418`): `f *= rangebias / (rangebias + cost)` — `f` = item value, `rangebias` (~2000 qu) and `cost` both converted qu → **travel-time seconds** via `waypoint_getlinearcost` (`navigation.qc:1225-1226`); `cost = wp.wpcost + wp→item leg` (`navigation.qc:1416`). Highest wins. One smooth value/distance tradeoff unifies items, enemies, wander. `[PORT]`
- **Re-rating**: every `bot_ai_strategyinterval` 7 s (5.5 s if goal movable) (`navigation.qc:20-26`). **Evidence-based early expiry**: goal freed, item observed taken (checked only when in PVS! `havocbot.qc:761-779`), unreachable, anticipated timeout when far, teammate-yield (`navigation.qc:56-75`). `[PORT]` — "re-plan on evidence, not just timers".
- **Touch-pop** `navigation_poptouchedgoals` (`navigation.qc:1630`): waypoint touched inside ±12 qu box extended down by jump height (`navigation.qc:1752-1755`); at bunnyhop speed a loose 100 qu + LOS test (`navigation.qh:93`, `navigation.qc:1717-1746`); teleporters/jumppads pop only after a **verified teleport** (`TELEPORT_USED`, `navigation.qh:65`); handles being blasted past intermediate goals (`navigation.qc:1682-1715`).
- **Shorten-path chase cutover** (`navigation.qc:1555`): every 0.25 s, if final goal movable, within `MAX_CHASE_DISTANCE = 700` (`navigation.qc:55`) and `tracewalk` succeeds → discard the whole chain, chase directly; also pops `goalcurrent` if already closer to `goalstack01`. `[PORT]`
- **Item↔waypoint incoming-link cache with negative caching** — unwalkable links cached at cost 999, tracewalk never repeated (`navigation.qh:44-63`, `navigation.qc:1510-1537`). `[PORT]`
- **Unstuck global queue** (`navigation.qc:1908`): one owner bot at a time; scans waypoints ≤1000 qu, tracewalk-tests one per tick, routes to farthest reachable. `[PORT]`

### Item values (`items.qc:885-979` — `bot_pickupevalfunc`)
- Weapons: 0 if owned & no ammo need; else `basevalue * (1 − 0.5*bound(0, arsenal_value/20000, 1))` (rich bots want new guns less). Base: vortex/devastator 8000, mortar 7000, … 0–10000.
- Ammo (`items.qc:909-955`, corrected 2026-07-11 during the Plan 59 port): `rating = ammo_base * min(2, gives / max(0.5, have)) [+ weapon_base*0.1 when the pickup is an owned weapon]` — worth MORE the emptier you are (`noammorating = 0.5`). Health/armor (`items.qc:957-979`): `basevalue * min(2, amount/current)` (armor denominator `armor*2/3 + health*1/3`); base 5000, ammo 1000–2000.
- **item_group multiplier** `min(4, group_count)`: clustered small pickups rate as a bundle; on arrival sweep the group (`havocbot_select_an_item_of_group`, `havocbot.qc:370,858-869`). `[PORT]`
- Enemy-player rating (`roles.qc:176-213`): `t = bound(0, 1 + (my_hp+armor − their_hp+armor)/150, 3)`, `+ max(0,8−skill)*0.05` ("less skilled bots attack more mindlessly"), final `= ratingscale*0.0001 * t * BOT_RATING_ENEMY(2500)`. `[ADAPT]` — enemy health isn't on the Q2 wire; default their hp+armor to a constant (100) or estimate from damage dealt.
- Item timing (`roles.qc:118-137`): taken items still rated if respawn within `bound(0,skill/10,1)*6` s (powerups; 4 s at skill≥9); bot pre-moves and camps (`bot_pickup_respawning`, `havocbot.qc:755-772`). `[ADAPT]` — we already infer respawn timers client-side (`brain::items::ItemMemory`, Plan 30).
- DM role is trivially small — all intelligence is in the ratings (`roles.qc:224-232`): `items(10000, org, 10000); enemyplayers(10000, org, 10000); waypoints(1, org, 3000)`.
- Wander fallback `havocbot_goalrating_waypoints` (`roles.qc:16`): random annulus rings; ×0.1 penalty on the last two visited waypoints (`wp_goal_prev0/1`).

## §3 Waypoints & pathfinding (waypoints.qc)

- 32 outgoing links per waypoint, **sorted by cost**, worst replaced when full (`waypoints.qh:26-37`, `waypoints.qc:1088-1131`).
- **Cost = travel time in seconds**, not distance: `dist/sv_maxspeed`; `dist/(maxspeed*1.25)` when `skill ≥ bot_ai_bunnyhop_skilloffset` — bunnyhoppers literally see a faster map, costs globally recomputed when skill crosses the threshold (`waypoints.qc:1010-1015`, `bot.qc:723-734`); underwater `/(maxspeed*0.7)`; crouch `/(maxspeed*0.5)`; falls: drop > jump height → `cost = max(xy_cost, sqrt(height/(gravity/2)))` — free-fall time (`waypoints.qc:1041-1052`). `[PORT]` as edge-weight transform.
- `navigation_markroutes` (`navigation.qc:1082-1169`): label-correcting Dijkstra over the whole graph; each waypoint gets `wpcost` + `.enemy` parent. Relaxation **adds the target's `dmg` danger score** (`navigation.qc:1134`). Air source = expensive expanding-radius scan (`navigation.qc:1106-1120`).
- **Danger map** `botframe_updatedangerousobjects` (`navigation.qc:1874-1906`): per waypoint, per dangerous entity (`g_bot_dodge`, per-entity `bot_dodgerating`): `d = linearcost(dodgerating) − traveltime(obj→wp)`; positive + LOS → added to `wp.dmg`. A continuous, distance-decaying danger field baked into path costs, refreshed every 0.25 s. `[ADAPT]` — feed from PVS-observed projectiles/players only; that *is* honest visibility.
- Auto-linking `waypoint_think` (`waypoints.qc:1147-1251`): O(n²) with PVS cull (`waypoints.qc:1172`), XY-length cull 1050 qu (`waypoints.qc:1187-1206`), then bidirectional `tracewalk` with player bbox.
- Special types (`bot/api.qh:11-29`): `TELEPORT` (box wp, single one-way link, cost = flight time, `waypoints.qc:2010-2024`), `JUMP`, `LADDER`, `CROUCH`, `SUPPORT` (funnel entry), `PERSONAL`, `ITEM`. `WPFLAGMASK_NORELINK` protects them from relinking. (We already model Jump/Swim/Ride/Teleport edge kinds — Plan 39/42/52.)
- **tracewalk** (`navigation.qc:274-744`) is the everything-primitive: 32 qu/step walk sim with step-up, jump-step (`stepheight + 0.85*jumpheight`, `bot.qc:619`), swim states + binary-search resurfacing (`navigation.qc:245-263`), lava rejection, door pass-through (`navigation.qc:659-670`), ladder escape. `[ADAPT]` on our CM trace; doors must be assumed-open (no live movers in our CM).
- **File formats**: `maps/<map>.waypoints` = 3 lines/wp (mins/maxs/flags) + `//WAYPOINT_VERSION 1.04` header (`waypoints.qc:1801-1825`); `.cache` = link lines `"<from>*<to>"` (`waypoints.qc:1740-1743`); `.hardwired` = same, `*`-prefixed lines are special links (`waypoints.qc:1502-1573`). `[PORT]` idea: a hardwired-link escape hatch for hand-fixing generated graphs.
- **Autowaypointing** (`waypoints.qc:2467`): follows humans; when the link chain to the player breaks, **binary-search the antilag position history** for the farthest-back linkable point, spawn a waypoint there (`waypoints.qc:2313-2362`); `botframe_deleteuselesswaypoints` (`waypoints.qc:2385`) GCs waypoints not needed for item access or triangle-inequality shortcuts. Proves linkability at creation (vs Eraser's blind trail sampling). `[ADAPT]` — we record PVS positions each frame; bisection works identically.

## §4 Movement execution — `havocbot_movetogoal` (`havocbot.qc:446`)

- **Lookahead cornering** (`havocbot.qc:972-1045`): steer target = `origin + max(32, speed*cos(deviation)*0.3)*flatdir`; within lookahead of the current wp with a next goal → slide the steer target along the direction to the **next** goal (`actual_destorg = destorg + dist*next_dir`, `havocbot.qc:1042`) — corner cutting, not waypoint-exact tracking. `[PORT]` (our `pursue_target` look-ahead is similar; theirs blends toward the next leg).
- **Goal-progress watchdog** (`havocbot.qc:344-368`): best-ever 2D and Z distance to goal; no improvement for 0.5 s → tracewalk revalidate; second failure → dump route, force re-rating (`havocbot.qc:1102-1128`). `[PORT]`
- **Obstacle jump** (`havocbot.qc:1060-1100`): foot-level tracebox toward steer target; blocked by steep plane (`normal.z < 0.7`) → retry at `+stepheight`, then `+stepheight+jumpheight`; jump only if the raised trace goes farther; never while deviating >50° or ducked.
- **JUMP waypoints** (`havocbot.qc:993-1010`): press jump in the 50–150 qu window past the wp at speed.
- **Danger ahead** (`havocbot.qc:401-444`): probe `origin + view_ofs + (velocity*0.2 | 32*flatdir)`, trace ahead then 3000 down; classify sky/drop/lava/trigger_hurt; suppresses bunnyhop, marks chase unreachable; **evade vector** perpendicular from the wp segment scaled `bound(1, 3−(skill+dodgeskill), 3)` — "noobs fear dangers a lot" (`havocbot.qc:1249-1268`); `do_break` reverse-velocity braking when overshooting a goal 120 qu below (`havocbot.qc:1166-1174`).
- **Projectile dodge** (`havocbot.qc:1773-1829`): per dangerous entity, distance from its **flight path** (`v -= n*(v*n)`), danger = `dodgerating − vlen(v)`; dodge perpendicular to the path, else radially away; scaled `bound(0, 0.5+(skill+dodgeskill)*0.1, 1)`; suppressed if it points into danger (`havocbot.qc:1185-1204`). `[PORT]` for PVS-visible projectiles (velocity from frame deltas).
- **Compose** (`havocbot.qc:1269-1278`): `dir = normalize(dir*dodge_enemy_factor + dodge + do_break + evadedanger)`; project on view axes → movement; jump if `dir·v_up ≥ jumpvelocity*0.5` (`havocbot.qc:1318`).
- **Keyboard emulation** (`havocbot.qc:272-341`): quantize `movement/maxspeed` to {−1,0,1} per axis, threshold `bot_ai_keyboard_threshold` (0.57), re-decided at key rate `0.05/(sk+kbskill) + random()*0.025/(skill+kbskill)`; skill tiers gate combos (<1.5 forward only, <2.5 no diagonals, <4.5 fwd diagonals only); quantized vector blended over analog by `bound(0, dist_to_goal/250, 1)` — analog when close, clunky when far; applied when `skill < 10`. `[PORT]` — maps perfectly onto Q2 `usercmd` forward/side.
- **Bunnyhop** (`havocbot.qc:215-270`), gated `skill+moveskill ≥ 7`: only roaming, grounded, ≥maxspeed, vel/goal deviation <20°, goal beyond flat-jump landing `52.661 + 0.606*vel` (`havocbot.qc:233-235`) or next-leg turn under `max(4, 80 − 40*(vel−maxspeed)/maxspeed)` with pitch-down <30° (`havocbot.qc:248-251`). Sets `AI_STATUS_RUNNING` → loose touch radius. `[ADAPT]` — constants are Xonotic-physics; Q2 needs its own jump-chain timing. Low priority.
- Low-skill overshoot: `skill+moveskill ≤ 3` at speed, deviation >70° → full stop 0.4–0.6 s (`havocbot.qc:1130-1134`). Swim-up+jump surface breach (`havocbot.qc:946-968`); ladder z-steer (`havocbot.qc:1207-1228`). Jumppad-recovery state machine (`havocbot.qc:527-614`) `[ADAPT if we ever nav jumppads]`; rocketjump/jetpack trigger_hurt escape (`havocbot.qc:617-721`) `[DROP]`.

## §5 Aim — the dynamical system (aim.qc)

Per aim call:
1. **Error offset** (`aim.qc:194-203`): every 0.2–0.5 s, `bot_badaimoffset = randomvec() * bound(0, 1 − 0.1*(skill+offsetskill), 1) * bot_ai_aimskill_offset(1.8)`, vertical ×0.7; ×5 fighting, ×2 roaming.
2. **Anticipation filter cascade** (`aim.qc:230-250`): desired-angle *velocity* through **five chained first-order low-pass filters** (poles `0.2/0.2/0.1/0.2/0.25`), outputs mixed back with weights `0.01/0.075/0.01/0.0375/0.01`, scaled `blend = bound(0, skill+aimskill, 10)*0.1`. Low skill lags a mover; high skill anticipates.
3. **Mouse-think quantization** (`aim.qc:261-265`): internal target updates only every `0.5 − 0.05*(skill+thinkskill)` s with random undershoot — discrete human retargeting.
4. **Turn-rate** (`aim.qc:289-295`): `fixedrate = 15/bound(1,dist_angle,1000)`, `blendrate = 2`, `r = bound(dt, max(fixed,blend)*dt*(2 + (skill+mouseskill)³*0.005 − random()), 1)`; `v_angle += diffang*(r + (1−r)*(1−bot_ai_aimskill_mouse))`.
5. **Fire cone** (`aim.qc:302-330,369-374`): tolerance `maxfiredeviation = 1000/(dist−9) − 0.35` degrees, scaled `((accurate?1:1.6) + bound(0,(10−(skill+aimskill))*0.3,3))`; inside tolerance arms `bot_firetimer = time + bound(0.1, 0.5 − (skill+aggresskill)*0.05, 0.5)` → **bursts**; hesitation `random()*random() > (skill+aggress)*0.05`.
6. **Lead**: `shotlead = targorigin + targvel*(shotdelay + dist/shotspeed)` (`aim.qc:333-337`) using simulated latency (ours: real RTT); ballistic `findtrajectorywithleading` (`aim.qc:16-95`) brute-forces ≤10 `tracetoss` launches raising z 0.1/try. `[ADAPT]` tracetoss → our own gravity-step trace vs CM.

`[PORT]` wholesale — operates purely on desired-angle streams + target velocity (we have both). vs Q3: models the *dynamics* of human aim (filters + discrete retarget + distance-derived fire cone), not just a positional error ellipse.

## §6 Combat — enemy & weapon choice (havocbot.qc)

- `havocbot_chooseenemy` (`havocbot.qc:1334`): re-scan every 2 s (4 s while sticking); sticky keeps the current enemy while visible within 1000 qu, extending 0.5 s per check (`havocbot.qc:1350-1365`). Pick = **nearest visible** (SUPERBOT: minimize `bound(50, health+armor, 250)*distance` — weak+close, `havocbot.qc:1414-1426` `[ADAPT: health unknown]`). Filters: teammates, chat-protected, `alpha < 0.1`, NOTARGET (`aim.qc:97`). Transparent-wall scan `[DROP]`.
- `havocbot_chooseweapon` (`havocbot.qc:1495`): three priority lists (far/mid/close) with thresholds 850/300 qu; effective distance `bound(10, real−200, 10000) * 2^bot_rangepreference` (`havocbot.qc:1564-1565`) — the per-bot sniper/spammer bias. **Weapon combos** (`havocbot.qc:1544-1559`): if the current gun's refire won't finish before `time + 0.4*(4 − 0.3*(skill+weaponskill))` → switch mid-refire (vortex→machinegun); then locked 1 s. `[PORT]` — Q2 refire times known client-side.
- Keepaway: enemies <100 qu not *rated* as goals (`roles.qc:189`); chase halts 80 qu out, steer destination pulled out of the target bbox (`havocbot.qc:915-931`); no chasing >2×maxspeed movers (`roles.qc:191`) or while swimming (`roles.qc:182`).
- SUPERBOT combat strafe: random XY vector every 0.35 s, 15% stand (`havocbot.qc:1281-1307`).

## §7 Skill & personality

Per-bot personality = **12 additive skill offsets** loaded from tab-separated `bot_config_file` rows (`READSKILL`, `bot.qc:275-290`): `keyboardskill, moveskill, dodgeskill, pingskill, weaponskill, aggresskill, rangepreference, aimskill, offsetskill, mouseskill, thinkskill, aiskill` — each added to global `skill` wherever used. Simpler than Eraser's trait file, broader than Q3's fuzzy weights. `skill_auto`: ±1 skill per 5 s when frag gap ≥2 (`bot.qc:569-613`).

Key cvar defaults (`xonotic-server.cfg:136-183`): thinkinterval 0.05 · strategyinterval 7 / movingtarget 5.5 · enemydetectioninterval 2 / sticking 4 · enemydetectionradius 10000 · chooseweaponinterval 0.5 · dangerdetectioninterval 0.25 (64 wp/pass) · aimskill_offset 1.8 · filter poles .2/.2/.1/.2/.25 · mix .01/.075/.01/.0375/.01 · fixedrate 15 / blendrate 2 · keyboard_threshold 0.57 / distance 250 · bunnyhop_skilloffset 7 · turn envelope 4/80/40, pitch 30 · weapon_combo_threshold 0.4 · priority distances "300 850" · ignoregoal_timeout 3 · friends_aware_pickup_radius 500 · timeitems 1.

Scripting (`scripting.qc`, `scripting.qh:13-41`): per-bot command queue (`pause/wait/turn/moveto/aim/presskey/if-else-fi/barrier…`) run before the AI each tick — a test-harness idea, not brain logic.

## §8 Portability verdicts (external Q2 client, PVS-only, own BSP/CM)

| Mechanism | Verdict |
|---|---|
| Strategy-token scheduling | **PORT** — pure fleet scheduling |
| Goal stack + touch-pop + shorten-path + watchdog | **PORT** (Vec, our reach tolerances) |
| Rating formula + item eval + wander annulus | **PORT** — items from `BrainMap.items`, inventory from playerstate |
| Travel-time edge costs (water/fall variants) | **PORT** as runtime edge-weight transform |
| Dijkstra flood + parent chain | **PORT** — add a single-source flood API next to our A* |
| Danger field in path cost (`wp.dmg`) | **ADAPT** — PVS-observed projectiles/players only |
| tracewalk | **ADAPT** on our CM trace; doors assumed open |
| Item timing / camp respawns | **ADAPT** — reuse `ItemMemory` (Plan 30) timers |
| “Did we get a delta for it” goal checks | **PORT** — the honest translation of `checkpvs` |
| Lookahead cornering, obstacle-jump traces, watchdog | **PORT** — static CM + own state |
| Keyboard emulation | **PORT** — quantize into `usercmd` |
| Bunnyhop | **ADAPT** — recalibrate constants for Q2 pmove; low priority |
| Flight-path projectile dodge | **PORT** for PVS-visible projectiles |
| Aim pipeline (filters/mouse/turn-rate/fire cone/lead) | **PORT** — angle streams + target vel from deltas; latency = real RTT |
| `findtrajectorywithleading` | **ADAPT** — own gravity-step trace |
| chooseenemy sticky/nearest-visible | **ADAPT** — enemy set = PVS entities (that *is* visibility); health weighting dropped |
| Weapon combos / far-mid-close lists / 2^rangepref | **PORT** — Q2 arsenal mapping |
| Autowaypointing via position-history bisection | **ADAPT** — our recorded PVS trails; future graph-repair tool |
| Waypoint file formats + hardwired links | **PORT** idea — hand-fix escape hatch |
| Transparent-wall scan, jetpack, rocketjump escape | **DROP** |

## §9 Distinctive vs Q3 / Eraser / 3ZB2 — the adopt-list

1. **One smooth objective**: `value * rangebias/(rangebias+cost)` over travel-time — unifies items/enemies/wander (vs Q3 fuzzy weights, 3ZB2 route tables).
2. **Danger field baked into path cost**, refreshed 0.25 s — none of our brains price *routes* by ambient danger (our heatmap is obituary/presence-based and brain-opt-in).
3. **Goal stack with continuous repair** — touch-pop, chase cutover, 0.5 s progress watchdog, evidence-based expiry. Our brains mostly re-plan on timers; Xonotic re-plans on *evidence*.
4. **Strategy token** — amortize the flood across the fleet.
5. **Negative caching** of proven-unwalkable item links.
6. **Keyboard-emulation steering** with a skill-gated key vocabulary — humanizes movement; maps 1:1 onto Q2 usercmd.
7. **Aim as a dynamical system** — filter cascade + mouse-think + `1000/(dist−9)−0.35` fire cone + burst timer. Richer than Q3's ellipse or Eraser's jitter; fully client-side computable.
8. **Weapon combos** (switch mid-refire when another gun lands sooner) + `2^rangepreference` personality bias.
9. **Item timing** — skilled bots pre-move to powerups and camp them.
10. **12-axis additive personality** from a roster file — one global skill + per-behavior deltas.
11. **Autowaypointing that proves walkability** (tracewalk + history bisection) + useless-waypoint GC + hardwired-link files.

**Port caveat**: Xonotic leans on cheap server-side `checkpvs`/`traceline` against *live* entities for goal validity, enemy LOS, item camping. Each becomes: (a) a trace against our static CM, (b) a BSP PVS-cluster test, or (c) **"did the server send us a delta for that entity this frame"** — and (c) is usually the most honest translation.
