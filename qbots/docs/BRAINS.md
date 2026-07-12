# qbots — Brains, Tunables & Switches

A **brain** is a qbots bot's decision layer: it turns each frame's PVS-limited perception into
movement + fire intent. Brains are a **plugin contract** (`trait Brain`, `crates/brain/src/brains/core.rs`)
selected at startup by `build_brain` (`crates/brain/src/brains/mod.rs`) — exactly mirroring how the
nav layer selects a backend with `NavMode` / `build_navigator`.

> **Two orthogonal axes.** A bot's **brain** (`--brain`, *what to decide*) and its **nav backend**
> (`--navmode`, *how to path*) are independent. Any brain runs on any navmode.

| Brain | Token | One-liner | Combat? | Navigates? |
|-------|-------|-----------|:-------:|:----------:|
| **main** | `main` | Eraser-derived bot — the live fleet default (5-state FSM, aim/lead/dodge, skill drift) | ✅ | ✅ |
| **q3** | `q3` | Quake 3-derived bot — node FSM + aggression-gated retreat/chase + Q3 aim/fire texture | ✅ | ✅ |
| **sentry** | `sentry` | Stationary turret — fires at any LOS enemy, never moves (reference plugin) | ✅ | ❌ |
| **runtester** | `runtester` | Combat-free pathfinder — the `spawn-to-*` movement-scenario brain | ❌ | ✅ |

Source: `crates/brain/src/brains/{main,q3,sentry,runtester}.rs`. Research distillations:
`context/distilled/eraser.md` (main), `context/distilled/quake3.md` (q3). Implementation history:
`context/plans/completed/` (Plans 06–08 = main, 23–26 = the seam, 36–38 = q3).

---

## 1. Switches (runtime — no rebuild)

These are the **only** user-facing knobs. Everything else (skill level, personality floats, the
internal constants in §4) is a code-level default that requires editing the source and rebuilding.

### Brain selection

| Switch | Where | Values | Default |
|--------|-------|--------|---------|
| `--brain <kind>` | `connect-one`, `run` | `main` \| `sentry` \| `runtester` \| `q3` \| `zb2` \| `xon` | `main` (`run`: `[fleet].brain`) |
| `--brain <kind>` | `spawn-to-spawn`, `spawn-to-weapon` | `runtester` \| `main` (A/B pathing; combat forced off) | `runtester` |
| `--brains a,b,…` | `competition` | comma list; **`runtester` rejected** (non-combat) | `main` |
| `[fleet].brain` | `config.yaml` | `"main"` \| `"sentry"` \| `"runtester"` \| `"q3"` \| `"zb2"` \| `"xon"` | `main` |

### Q3 personality (only affects `--brain q3`)

| Switch | Where | Values | Default |
|--------|-------|--------|---------|
| `--char <name>` | `connect-one`, `run` | `grunt` \| `major` \| `sarge` \| `camper` | skill-derived (`Q3Character::from_skill(5)`) |
| `--chars a,b,…` | `competition` | comma list; fields one group/skin per character | one default-character `q3` group |
| `[fleet].char` | `config.yaml` | `"grunt"` \| `"major"` \| `"sarge"` \| `"camper"` | skill-derived |

### Xonotic personality (only affects `--brain xon`; Plans 59–62)

| Switch | Where | Values | Default |
|--------|-------|--------|---------|
| `--xonchar <name>` | `connect-one` | `rus`(rusher) \| `shp`(sharp) \| `trt`(turtle) \| `nob`(noob) | neutral `XonSkill` at master skill |
| `--xonchars a,b,…` | `competition` | comma list; one group/skin per preset (only expands `xon`) | one neutral `xon` group |
| `[fleet].xonchar` | `config.yaml` | same names | neutral |

`xon` (Plan 60, `context/distilled/xonotic.md`) is the Xonotic-havocbot brain: one smooth
goal objective (`value·rangebias/(rangebias+travel_time)` over items/enemies/wander, one
Dijkstra flood per 7 s session), evidence-based re-planning (observed pickups, progress
watchdog, 3 s ignore list), sticky nearest-visible enemy, far/mid/close weapon lists with
mid-refire combos + a probe-and-learn inventory, the `XonAim` dynamical system (5-filter
anticipation cascade, mouse-think, `1000/(dist−9)−0.35` fire cone, burst timer), flight-path
projectile dodge, and keyboard-emulated movement. Personality = a 12-axis additive `XonSkill`
(`xonchar.rs`); tuning history in `context/mode_perf.md` (2026-07-11). The `xg` navmode
(Plan 61, `xonnav.rs`) carries the matching route texture and works with EVERY brain.

A selected character also pins a recognizable **skin** (grunt→`male/grunt`, major→`male/major`,
sarge→`male/sarge`, camper→`female/athena`). Non-`q3` brains ignore `--char`.

### Nav backend (orthogonal — see [README](../README.md))

`--navmode <mode>` (`connect-one`/`run`/`spawn-*`) and `--navmodes a,b,…` (`competition`):
`astar` (default), `navmesh`, `hybrid-fallback`, `hybrid-race`, `hybrid-hier`, `hybrid-segment`.

### Competition naming

`competition` spawns the full `{brain} × {navmode} [× {char}]` cross-product in one process and
names each bot `<brain>_<navmode>[_<char>]_<i>` — e.g. `main_astar_1`, `q3_race_1`,
`q3_astar_grunt_1`. The per-group frag scoreboard groups by that tag.

```bash
# Brain A/B on one map, all navmodes:
qbots competition --navmodes astar --brains main,q3 --count 4
# Full Q3 roster head-to-head:
qbots competition --navmodes astar --brains q3 --chars grunt,major,sarge,camper --count 2
# Fleet of one character:
qbots run --brain q3 --char sarge --count 8
qbots connect-one --brain q3 --char major
```

> **Not a switch:** the master **skill level** (0–10) is fixed at `BotSkill::default()` = **5** in
> the binary — there is no `--skill`/`[fleet].skill`. `main`/`sentry` bots all start at skill 5
> (then drift via auto-skill, §2); `q3` derives its default character from skill 5. To change skill
> you edit the `BotSkill::default()` call sites (`bot_task`, `build_brain`) or add a flag.

---

## 2. `main` — the Eraser brain

The live fleet default. A flat 5-state FSM (`Roam / Hunt / Engage / Flee / Pickup`, `fsm.rs`) with
Eraser-derived combat (aim + per-weapon lead + jitter, projectile-dodge, weapon select) over the
injected navigator, plus stuck recovery and string-pulled paths. Implementation:
`crates/brain/src/brains/main.rs` (orchestration) + `combat.rs`/`aim.rs`/`danger.rs`/`steer.rs`/
`recover.rs`/`items.rs`/`heatmap.rs`.

**Personality model — `BotSkill`** (`crates/brain/src/skill.rs`; code-level, default `skill 5,
Balanced`):

| Field | Range | Drives |
|-------|-------|--------|
| `skill` | 0–10 | aim jitter, reaction delay, combat/accuracy ratings, turn rate |
| `personality` | `Conservative` \| `Balanced` \| `Aggressive` | flee threshold, item-search, heatmap popularity pull |
| `ratings` (`accuracy`/`aggression`/`combat`) | 1.0–5.0 | derived from skill via `adjust_to_skill` (Eraser `bot_misc.c:1065`) |
| `auto_skill` | live float | **drifts at runtime**: `+0.2` on kill, `−0.2` on death → re-derives ratings |
| `camper` / `quad_freak` | bool | dwell ~5× longer per roam node / over-value the Quad |

Personality affects: `flee_health_threshold` (Conservative 40% / Balanced 25% / Aggressive 15%),
`reaction_delay_frames`, `aggressiveness` (0.3/0.5/0.8), `heatmap_weights` (danger-avoid scales with
skill; popularity pull scales with personality).

**Combat constants** (`combat.rs`, code-level): target lock 5 frames, LOS sight-grace 2 frames,
weapon-switch fire lockout 0.9 s, reaction delay `0.8·(5 − combat·0.5)/5` s, per-weapon fire
intervals (`weapons.rs::fire_interval_secs`). **Ideal-range** (`main.rs`): `IDEAL_DIST = 160`,
`BACKUP_DIST = 80`; circle-strafe engages only when `combat > 1.5`.

---

## 3. `q3` — the Quake 3 brain

A sibling to `main` (not a fork). Reproduces Quake 3's deathmatch decision loop on top of the same
`Navigator`/`steer`/`recover`/`los`. Implementation: `crates/brain/src/brains/q3/{mod,aim,move}.rs`;
personality + decision scalars in `crates/brain/src/q3char.rs`. Research: `context/distilled/quake3.md`.

### Node FSM (`q3/mod.rs`)

`SeekLtg` (roam to item) · `SeekNbg` (grab nearby item) · `BattleFight` · `BattleChase` (lost sight)
· `BattleRetreat` (out-gunned/hurt — **the distinctive Q3 state**) · `BattleNbg` (grab item
mid-fight). Transitions are gated by the **aggression scalar** (below). Timers: chase 10 s,
retreat-unseen 4 s, NBG 5 s deadline / 0.5 s poll; per-tick switch guard `MAX_NODESWITCHES = 50`.

### Aggression — the engage/disengage scalar (`q3char.rs`)

`bot_aggression(view, enemy_height_delta)` → 0–100 from the **held** weapon + health + armor
(`ai_dmq3.c:2199`, adapted to wire-visible inventory — we see only the held weapon via
`SelfState.held_weapon`, not Q3's full inventory). `wants_to_retreat` / `wants_to_chase` compare it
to a character-biased threshold (`Q3Character::retreat_threshold = 50 − (aggression−0.5)·40`).

- Health guards: `health < 60` → 0; `health < 80 && armor < 40` → 0.
- Enemy `> 200 u` above → 0 (bad angle).
- Held weapon ranks by `Weapon::power_tier` (BFG 100 > Rail 95 > HB/RL 90 > GL 80 > S/SSG 50 >
  MG/CG 25 > Blaster 0), gated by that weapon's ammo. Tiers `< 50` (MG/CG) → 0 (flee).
- **Q2 blaster-floor:** a *healthy* bot holding the blaster floors at 50 (it's Q2's infinite-ammo
  start weapon, unlike Q3's melee gauntlet) so q3 bots engage on the spawn loadout instead of
  fleeing forever. Hurt bots still flee.

### Personality — `Q3Character` (`q3char.rs`; code-level floats `[0,1]` unless noted)

| Field | Drives |
|-------|--------|
| `attack_skill` | combat movement: `<0.2` stand still, `<0.4` close/open gap only, `≥0.4` circle-strafe |
| `aim_accuracy` / `aim_skill` | aim error magnitude / prediction (lead `>0.4`, radial ground-aim `>0.6`, reaction gate `>0.95`) |
| `reaction_time` (s, `[0,5]`) | sight delay before firing |
| `jumper` / `croucher` | combat dodge chance (jump real; **crouch best-effort no-op** — see §4) |
| `aggression` | biases the retreat/chase threshold (presets, not the scalar) |
| `self_preservation` | abort splash shots that would hit own feet (`>0.3`) |
| `alertness` | enemy detection range `900 + alertness·4000` + awareness FOV width |
| `firethrottle` | burst-fire duty cycle (higher = sprays more) |
| `camper` / `vengefulness` / `walker` / `easy_fragger` | camp tendency / revenge / walk-vs-run / target greed |
| `per_weapon_accuracy: Option<[f32;10]>` | per-weapon accuracy override (else `aim_accuracy`) |

**Presets** (`Q3Character::{grunt,major,sarge,camper}`, selected by `--char`):

| Preset | aim_skill | reaction | aggression | jumper | camper | firethrottle | Character |
|--------|:---------:|:--------:|:----------:|:------:|:------:|:------------:|-----------|
| **grunt** | 0.30 | 0.80 | 0.50 | 0.20 | 0.10 | 0.70 | cannon fodder — sprays + dies |
| **major** | 0.90 | 0.30 | 0.60 | 0.30 | 0.20 | 0.20 | precise + efficient (crack shot) |
| **sarge** | 0.60 | 0.40 | 0.90 | 0.80 | 0.00 | 0.40 | aggressive + mobile brawler |
| **camper** | 0.70 | 0.50 | 0.20 | 0.10 | 0.90 | 0.30 | cautious, holds spots |

`Q3Character::from_skill(s)` (the no-`--char` default) is a monotonic remap of skill `0–10`:
`aim_accuracy/attack_skill/alertness = 0.30+0.60·(s/10)`, `aim_skill = 0.20+0.70·(s/10)`,
`reaction_time = 1.20−1.00·(s/10)`, `self_preservation = 0.30+0.50·(s/10)`,
`firethrottle = 0.70−0.50·(s/10)`, with aggression-flavored traits neutral.

Live tuning (q2dm1, `mode_perf.md`): the presets give an intentional spread — major K/D 5.00,
sarge 1.25, camper 1.00, grunt 0.00.

### Aim / fire texture (`q3/aim.rs`)

Per-weapon accuracy; reaction-time sight gate (precise bots, `aim_skill > 0.95`, wait
`0.5·reaction_time`); 0.5 s velocity memory + direction-change penalty; constant-velocity lead
(AAS-predict substitute); radial ground-aim for splash (`aim_skill > 0.6`); fire FOV gate (120°
close / 50° far); fire-throttle duty cycle; self-preservation splash abort.

### Movement (`q3/move.rs`)

Circle-strafe with `IDEAL_DIST = 300`, `DIST_RANGE = 100`; strafe-flip cadence
`0.4 + (1−attack_skill)·0.2` s (flip on `roll > 0.935`); random back-up; jump/crouch dodge (1 s
cooldowns). Steering turn-rate scales with `aim_skill` (`Steering::new(1.0 + aim_skill·4)`).

---

## 4. `sentry` & `runtester`

**`sentry`** (`brains/sentry.rs`) — the minimal reference plugin proving the seam runs with >1
brain. Stands still, aims + fires at any LOS enemy via the shared `CombatDriver`. Tunable only by
`BotSkill` (code-level, default 5). No navigation. A valid (if weak) competitor.

**`runtester`** (`brains/runtester.rs`) — the combat-free movement-scenario brain used by
`spawn-to-spawn` / `spawn-to-weapon`. Drives the injected navigator to a per-tick `goal_override`
via the corner-cut-safe `pursue_target_safe` look-ahead + a 7-ray escape recovery; never fires. **No
combat tunables.** Rejected by `competition --brains` (it never frags).

---

## 5. Shared, code-level tunables

These constants are shared by `main`/`q3`/`sentry` and live in the source (edit + rebuild to change):

| File | Constants |
|------|-----------|
| `combat.rs` | `TARGET_LOCK_FRAMES=5`, `SIGHT_GRACE_FRAMES=2`, `SWITCH_LOCKOUT_SECS=0.9`, `TICK_HZ=10`; reaction `0.8·(5−combat·0.5)/5` s |
| `steer.rs` | `YAW_SPEED_BASE=720°/s` `+ (combat−1)·120`; `ARRIVE_RADIUS=80`, `ARRIVE_MIN=0.25`; `STRAFE_PERIOD_SECS=3` |
| `recover.rs` | `DEADBAND=16 u`, `SAMPLE_EVERY_SECS=1`, `JUMP_AFTER_SECS=1`, `HARD_REPATH_SECS=3.5` |
| `weapons.rs` | per-weapon `power`, `power_tier`, `effective_range`, `min_safe_distance`, `fire_interval_secs`, `projectile_speed` |
| `aim.rs` | `PITCH_CLAMP_DEG=15`; per-weapon lead factors |

**Known deferral:** `MovementIntent.crouch` is a controller no-op today (the wire/pmove duck path
isn't wired), so the q3 `croucher` characteristic is best-effort — jump is the real dodge.

---

## 6. Adding a brain

1. Implement `trait Brain` (`brains/core.rs`) in `crates/brain/src/brains/<name>.rs`; register
   `pub mod <name>;` in `brains/mod.rs`.
2. Add a `BrainKind` variant (pin the CLI token with `#[value(name = "...")]` if needed), a
   `brain_tag` arm, and a `build_brain` arm.
3. The `--brain`/`[fleet].brain`/`competition --brains` plumbing is automatic (clap `ValueEnum`).
4. Append a dated entry to `context/brain_notes.md` (the running brain-work log).

See `context/plans/completed/23_*`–`26_*` (the seam) and `36_*`–`38_*` (q3) for worked examples.
