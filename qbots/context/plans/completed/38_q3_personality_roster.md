# Plan 38 — Quake 3 personality roster + tuning

> **Status**: pending
> **Created**: 2026-06-19
> **Depends on**: Plan 37 (`Q3Brain`), Plan 21 (competition runner)
> **Goal**: Make the `Q3Brain` a *roster* of distinct, selectable Q3 personalities (named
> characters), tunable from live frag data — so a server can be filled with recognizably
> different Q3 bots (a spray-and-pray Grunt, a precise Major, a jumpy Sarge, a Camper).
> **Agent**: ralph-loop

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Expose the Plan 36 `Q3Character` presets as a selectable personality on the `q3`
brain (`--q3char <name>` / `[fleet].q3char` / competition matrix), add an optional observed-
inventory upgrade so `bot_aggression` sees more than the held weapon, and tune the preset
value sets against live A/B frag data.

**Deliverables**:
1. `--q3char <grunt|major|sarge|camper|...>` on `connect-one`/`run`; `[fleet].q3char` config;
   `competition --q3chars …` to field the whole roster at once.
2. Per-character distinct skins/names so the scoreboard + in-game view distinguish them.
3. (Optional) `observed_inventory` — mine weapon/item pickups from prints/obituaries
   (`observed.rs`) so aggression ranks best-*owned*, not just held, weapon.
4. A live tuning pass: run the roster on q2dm1, record per-character frag rates in
   `context/mode_perf.md`/`brain_notes.md`, adjust preset floats, re-run.
5. Dated `context/brain_notes.md` Plan 38 entry with the final tuned value sets.

**Estimated effort**: Medium (1 day) + a live tuning loop.

---

## Context

Plan 37 ships *one* default `q3` personality (`Q3Character::from_skill(skill)`). The Q3 bots
people remember are *characters* — this plan turns the single brain into a roster and tunes it.

### Why this is its own plan

Plan 37 is already Large; rostering + a live tuning loop is separable and shouldn't block the
core brain landing. Competition wiring (Plan 21) already supports a `{navmode}×{brain}` cross
product; this plan adds a `×{q3char}` axis only for the `q3` brain.

### Reference value sets

`vendor/Quake-III-Arena/.../bots/*.c` are the stock Q3 character files (Grunt, Major, Sarge,
Visor, Mynx, …) — read them for plausible `[0,1]` trait values per archetype (do **not** commit
the files; distill the *shape* into `Q3Character` presets, per the slop "behavior not source"
rule). Distilled §3/§7 lists the archetypes.

### The held-weapon limitation (from Plan 36 / distilled §2)

`bot_aggression` ranks only the **held** weapon because the wire gives no free per-weapon
inventory. The optional `observed_inventory` (T3) closes part of that gap by *remembering*
weapons/ammo the bot has picked up (visible in `svc_print` pickup messages / obituary weapon
ids parsed in `observed.rs`), giving a closer match to Q3's "best owned weapon" aggression.
This is the only part of this plan that touches perception; keep it optional and additive.

---

## Step-by-Step Tasks

### T1: `--q3char` selection + per-character skin/name

**Files**: `crates/qbots/src/{main.rs,config.rs,supervisor.rs}`, `brains/mod.rs`

**What to do**: Add a `Q3CharPreset` enum (or reuse `Q3Character` named ctors) selectable via
`--q3char <name>` on `connect-one`/`run` and `[fleet].q3char` in config; thread it into
`build_brain` (extend the factory signature or add a `build_q3_brain` variant — keep
`build_brain`'s other arms untouched). Assign each preset a distinct skin + name prefix so the
bot is recognizable in-game and on the scoreboard. Default preset = `from_skill` (Plan 37
behavior unchanged when `--q3char` is absent).

**Commit**: `task(T1): --q3char preset selection + per-character skin/name`

### T2: `competition --q3chars` roster matrix

**File**: `crates/qbots/src/supervisor.rs`

**What to do**: Add a `--q3chars grunt,major,sarge,camper` axis that (for the `q3` brain) fields
one group per character; extend the group-tag/scoreboard grouping to include the char tag when
present (mirrors how `--brains` already regroups). qport blocks stay per-group disjoint.

**Commit**: `task(T2): competition --q3chars roster matrix + scoreboard`

### T3 (optional): observed-inventory aggression upgrade

**Files**: `crates/brain/src/observed.rs`, `brains/q3/mod.rs`, `q3char.rs`

**What to do**: Track weapons/ammo the bot has acquired by parsing pickup `svc_print`s /
obituary weapon ids (`observed.rs` already parses obituaries). Feed an `ObservedInventory` into
`bot_aggression` so it ranks the best *owned* weapon, not just the held one. Gate behind a
flag/field so Plan 37's held-weapon behavior remains the default and tests stay stable.

**Commit**: `task(T3): observed-inventory feeds Q3 aggression (best-owned weapon)`

### T4: live tuning pass

**Files**: `context/mode_perf.md`, `context/brain_notes.md`

**What to do**: Run the roster live on q2dm1 (`competition --navmodes astar --brains q3
--q3chars grunt,major,sarge,camper --count 2`), record per-character frag rates + qualitative
behavior, and adjust the preset `[0,1]` floats so the characters are *distinct and balanced*
(Grunt sprays + dies more, Major precise + efficient, Sarge aggressive + mobile, Camper holds
spots). Re-run until the spread is intentional. Capture the final value sets.

**Commit**: `task(T4): tune Q3 character presets from live frag data`

### T5: brain-notes + docs

**Files**: `context/brain_notes.md`, README/help, `q3char.rs` preset docs.

**What to do**: Dated Plan 38 `brain_notes.md` entry: roster wiring, the observed-inventory
decision (shipped vs deferred), and the final tuned preset table. Update README/help with the
`--q3char`/`--q3chars` flags and the character list.

**Commit**: `task(T5): brain_notes Plan 38 entry + roster docs`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/qbots/src/{main,config,supervisor}.rs` | `--q3char`/`--q3chars`, skins/names | P0 |
| `crates/brain/src/brains/mod.rs` | preset → `build_brain` plumbing | P0 |
| `crates/brain/src/q3char.rs` | tuned preset value sets | P1 |
| `crates/brain/src/observed.rs` | observed-inventory (optional) | P2 |
| `context/mode_perf.md`, `context/brain_notes.md` | tuning data + entry | P1 |

---

## Open Questions / Risks

1. **Factory signature churn** — adding a preset arg to `build_brain` touches all call sites.
   *Mitigation*: add a `q3char: Option<Q3CharPreset>` that only the `Quake3` arm reads, default
   `None` → `from_skill` (other brains ignore it); or a dedicated `build_q3_brain`.
2. **Observed inventory drift** — pickups seen but ammo spent isn't tracked precisely.
   *Mitigation*: keep it a coarse "has weapon" hint with conservative ammo; it only *raises*
   aggression toward Q3 parity, never fabricates ammo the bot lacks. Optional/flagged.
3. **Tuning is subjective.** *Mitigation*: anchor to stock Q3 character shapes; define the
   intended spread up front (T4) and tune toward it, not to a single "best" bot.

---

## Verification Checklist

- [ ] T1: `--q3char major` selects a distinct preset; absent → Plan 37 default; distinct skin/name.
- [ ] T2: `competition --brains q3 --q3chars grunt,major` fields both; scoreboard separates them.
- [ ] T3: (if shipped) observed inventory raises aggression toward best-owned weapon; tests stable.
- [ ] T4: per-character frag rates recorded in `mode_perf.md`; presets tuned to an intentional spread.
- [ ] T5: `context/brain_notes.md` has a dated Plan 38 entry with the final value sets; README updated.
- [ ] Whole plan: `cargo build`/`clippy -D warnings`/`test`/`fmt` clean; non-`q3` brains unchanged.
