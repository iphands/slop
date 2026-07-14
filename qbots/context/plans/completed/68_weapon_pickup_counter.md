# Plan 68 — Weapons-picked-up counter (`wp=` scoreboard column)

> **Status**: in-progress
> **Created**: 2026-07-13
> **Depends on**: Plan 67 (hp/ap pickup metric — same recording pipeline)
> **Goal**: Count weapon pickups per bot from the wire's pickup-message stat and carry them as a `wp=` column on the competition scoreboard.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Extend Plan 67's pickup metric with a weapon *count* (weapons don't move a stat value like health/armor, so they need a different wire signal: `STAT_PICKUP_STRING`).

**Deliverables**:
1. `brain::is_weapon_pickup_name` — case-insensitive match against Q2's weapon `pickup_name`s.
2. `BotTally.weapons_picked` + `FleetStats::record_weapon_pickup`.
3. `bot_task` watches `playerstate.stats[STAT_PICKUP_STRING=8]` transitions, resolves the configstring, counts weapons, emits `EVT pickup kind=weapon item=<name>`.
4. `wp=` column on live/FINAL scoreboards + final stats.

**Estimated effort**: Small (1–2 h)

## Context

### The wire signal (verified in vendor, 2026-07-13)
On every item touch the game sets `ps.stats[STAT_PICKUP_STRING] = CS_ITEMS + ITEM_INDEX(item)`
for 3 s (`g_items.c:1163`, cleared by `G_SetStats` after `pickup_msg_time`). The stat **value is
an absolute configstring index** (`CS_ITEMS = 1056`, `shared.h:1207`) whose string is the item's
display `pickup_name` (`g_items.c:2717`) — e.g. `Railgun`, `Super Shotgun`, `HyperBlaster`.
So pickup *identity* IS on the wire after all — Plan 67's "amounts only" caveat applied to
health/armor deltas, not to this stat.

### Why our own weapon switching can't false-positive
`g_cmds.c` also writes the stat on `weapprev`/`weapnext` (only when `g_quick_weap` is set,
`g_cmds.c:805,869`) and `cycleweap` (`:1780`). Our bots switch exclusively via the plain
`use <name>` stringcmd (`main.rs` weapon-switch path; `weapons.rs` doc) → `Cmd_Use_f`, which
never touches the stat. Genuine `Touch_Item` pickups always do.

### Decisions
- **Count = transitions**: count when the stat *changes* to a nonzero index naming a weapon.
  Re-picking the same weapon inside its 3 s message window shows no transition → undercounts;
  rare (any other pickup or the 3 s zero-clear re-arms it) and acceptable for a counter.
- **"Grenades" excluded**: hand grenades are `IT_AMMO|IT_WEAPON`; counting every grenade-box
  restock would swamp the metric. The counter tracks the 9 real guns (Shotgun…BFG10K).
- **Blaster excluded**: it has no world item; its name can only reach the stat via the cycle
  commands we never send.
- **Case-insensitive match**: vendor `pickup_name` is `HyperBlaster`; our `Weapon::name()` says
  `Hyperblaster`. `eq_ignore_ascii_case` sidesteps the whole class of mismatch.

## Step-by-Step Tasks

### T1: `brain::is_weapon_pickup_name`

**File**: `crates/brain/src/weapons.rs`

**What to do**: `pub fn is_weapon_pickup_name(name: &str) -> bool` — case-insensitive match
against the 9 pickable gun names (excl. Blaster/Grenades, doc-commented with the vendor
citations above). Unit test incl. the `HyperBlaster` casing and the Grenades/Blaster exclusions.

### T2: qbots wiring — tally field, detection, scoreboard column

**Files**: `crates/qbots/src/stats.rs`, `crates/qbots/src/main.rs`, `crates/qbots/src/supervisor.rs`

**What to do** (one dataflow, one commit — the recorder alone is a Rule A dead-code warning):
- `BotTally.weapons_picked: u64` + `record_weapon_pickup` + totals fold + unit test.
- `bot_task`: `last_pickup_cs: Option<i16>` beside the Plan 67 trackers; in the per-frame block,
  on stat change to nonzero resolve `cs.get(stat as usize)` and if `is_weapon_pickup_name` →
  debug `EVT pickup kind=weapon item=<name>` + record. Reset the tracker in the Plan 64
  map-change block.
- `ModeScore.weapons_picked`, `wp=` column on the board, `weapons_picked` in `log_final_stats`
  totals + per-bot rows; extend the aggregation test.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/weapons.rs` | `is_weapon_pickup_name` | P0 |
| `crates/qbots/src/stats.rs` | `weapons_picked` + recorder | P0 |
| `crates/qbots/src/main.rs` | STAT_PICKUP_STRING watcher | P0 |
| `crates/qbots/src/supervisor.rs` | `wp=` column | P0 |
| `context/acceptance.md` | extend the pickup-counters note | P2 |

## Open Questions / Risks

1. **Same-weapon re-pick inside the 3 s window is missed** — accepted undercount (documented above).
2. **Mods with custom item names won't match** — we only run stock DM.
3. **`g_quick_weap` servers + human players using weapnext** — irrelevant: the stat is per-client
   (our own playerstate), and our bots never send cycle commands.

## Verification Checklist

- [ ] T1: unit tests — 9 guns match (any case), `Grenades`/`Blaster`/ammo/health names don't. **Commit.**
- [ ] T2: aggregation tests green; live run shows `EVT pickup kind=weapon` with sane names and the
  `wp=` column equals the per-group EVT count. **Commit.**
- [ ] fmt/clippy/full tests green before each commit (Rule A); plan+tracker → `completed/`, SERIES updated.
