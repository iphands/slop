# Plan 69 — Roster-file competitions (`--roster <file.yaml>`) + ranked roster dump at FINAL

> **Status**: in-progress
> **Created**: 2026-07-15
> **Depends on**: Plan 21 (competition runner), Plan 67/68 (scoreboard columns the dump ranks by)
> **Goal**: Let `qbots competition --roster <file.yaml>` field an explicit hand-picked group list (replacing the CLI matrix), and emit a ranked, ready-to-edit roster YAML at the end of every competition run.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: A YAML roster file describes the exact competition groups to field, so you can
run "the top 8 from yesterday" instead of a full CLI matrix — and every run dumps a
ranked roster you edit down for the rematch.

**Deliverables**:
1. `--roster <PATH>` on `qbots competition`, mutually exclusive with the matrix switches.
2. New `crates/qbots/src/roster.rs`: schema + loader + validation + `emit_ranked_yaml`.
3. `run_competition` refactored to consume `Vec<GroupSpec>` (the matrix path builds the
   same specs — behavior-identical).
4. FINAL roster dump to `./logs/roster/<unix_ts>.yaml`, groups annotated with
   `# rank N  kd=… kills=… deaths=…` in K/D order.

**Estimated effort**: Medium (1 day)

## Context

Competition mode only accepts a CLI matrix (`--navmodes × --brains × --chars/--xonchars`,
uniform `--count`). There's no way to field an arbitrary hand-picked lineup, and no way to
carry a run's standings into the next round except by hand-copying group tags. The two
marathons in `context/bot_perf.md` are exactly this workflow — "keep the top groups, rerun."

**Key structural win** (verified): the short codes in scoreboard tags (`mai`, `sg`, `cam`)
are *exactly* the clap `#[value(name=…)]` tokens of `BrainKind`/`NavMode`/`CharPreset`/
`XonCharPreset`. So an emitted roster round-trips through `ValueEnum::from_str(s, true)` with
no hand-written token tables — the emitter reuses `brain_code`/`mode_code`/`char_code`/
`xon_char_code` (supervisor.rs:492-568).

Config is already YAML (`serde_yaml`, `config.rs:183`) — no new dependency.

## Step-by-Step Tasks

### T1: `GroupSpec` + pure `matrix_specs` + visibility widening

**File**: `crates/qbots/src/supervisor.rs`

`pub(crate) struct GroupSpec { mode: NavMode, brain: BrainKind, gc: GroupChar, count: usize,
skin: Option<String>, tag: String }`. Widen `GroupChar` + its accessors, `ModeScore` + fields,
and the four `*_code` fns to `pub(crate)`. Pure `pub(crate) fn matrix_specs(modes, brains,
chars, xonchars, per_group_count, skins_per_mode) -> Vec<GroupSpec>` porting the 664-701
nesting (mode-major × brains × `chars_for`, `gc.skin().or(mode_skin)`, `group_tag`).

**Tests first** (equivalence): modes=[as,nm] × brains=[mai,q3] × chars=[gru,cam], count=2 →
tags `[mai_as, q3_as_gru, q3_as_cam, mai_nm, q3_nm_gru, q3_nm_cam]`; cumulative qport offsets
`[0,2,4,6,8,10]`; q3-char skin beats mode skin; empty xonchars ⇒ one `GroupChar::None` xon
group; proportional clamp `((count*max_bots)/total).max(1)` == old `(max_bots/num_groups).max(1)`
for uniform counts.

### T2: refactor `run_competition` to `Vec<GroupSpec>`

**Files**: `crates/qbots/src/supervisor.rs`, `crates/qbots/src/main.rs`

`run_competition(cfg, addr, specs: Vec<GroupSpec>, qport_base_override, loose_botcap)`. qports
by running offset (cumsum of per-group counts). Empty guard → `specs.is_empty()`. Proportional
max_bots clamp per spec. `log_competition_legend` derives axes from specs (first-seen dedup).
Heartbeat + FINAL `group_tags` = `specs.iter().map(|s| s.tag)`. Dispatch calls `matrix_specs`.

### T3: `roster.rs` loader + validation

**File**: `crates/qbots/src/roster.rs` (new; add `mod roster;` to main.rs)

Serde structs (`#[serde(deny_unknown_fields)]`), `Roster::load(path)` (mirrors `Config::load`),
`into_specs() -> Result<Vec<GroupSpec>, String>`. Validation (each a red test first):
non-empty groups; tokens parse via `ValueEnum::from_str(s, true)`; RunTester rejected;
`char`⇔q3, `xonchar`⇔xon, never both; effective count = group→file→8, all ≥1; tag = custom or
`group_tag`, non-empty/no-whitespace, `tag.len()+1+digits(count) ≤ 15`; duplicate resolved
tags = hard error; skin = explicit → `gc.skin()` → None.

### T4: `--roster` flag + dispatch

**File**: `crates/qbots/src/main.rs`

`roster: Option<String>` with `conflicts_with_all = ["count","modes","brains","chars","xonchars"]`.
Dispatch: roster → `Roster::load` → `into_specs` (fill None skins via `skins::distinct_skins`
over unique modes) → `run_competition`; matrix → `matrix_specs`. `try_parse_from` conflict tests.

### T5: `emit_ranked_yaml` + FINAL dump hook

**Files**: `crates/qbots/src/roster.rs`, `crates/qbots/src/supervisor.rs`

`emit_ranked_yaml(ranked: &[ModeScore], specs: &[GroupSpec]) -> String` hand-formats YAML
(comments per group in rank order). Hook between `log_competition_scoreboard(…,"FINAL")` and
`fleet_join_result` (writes even on a join-failure run). `./logs/roster/<unix_ts>.yaml`
(`time::OffsetDateTime`, `create_dir_all`), path via `tracing::info!`. Round-trip test:
emit → parse → `into_specs` → assert spec equality + `# rank N` comments in order.

### T6: docs + close

`context/acceptance.md` competition section + a roster YAML example; SERIES.md done row;
move plan + tracker to `completed/`.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/qbots/src/roster.rs` (new) | schema, loader, validation, emitter | P0 |
| `crates/qbots/src/supervisor.rs` | GroupSpec, matrix_specs, run_competition refactor, dump hook | P0 |
| `crates/qbots/src/main.rs` | `--roster` flag + dispatch, `mod roster` | P0 |
| `context/acceptance.md` | competition roster docs | P2 |

## Open Questions / Risks

1. **max_bots clamp assumed uniform counts** — proportional formula is bit-identical for
   uniform, sane for variable. Asserted by an integer-identity test.
2. **`skins_per_mode` is positionally indexed by `modes`** — folded into specs at expansion;
   roster path fills None skins via `distinct_skins` over unique modes.
3. **Duplicate resolved tags → duplicate bot names → merged FleetStats rows** — hard error in
   `into_specs`, not a silent merge.
4. **RunTester rejection currently only in dispatch** (main.rs:2447) — the loader needs its own.
5. **serde_yaml can't emit comments** — `emit_ranked_yaml` is a hand formatter; the round-trip
   test guards its parseability.
6. **Live verification needs a running server** — unit tests gate all logic; live is best-effort.

## Verification Checklist

- [ ] T1: equivalence + clamp-identity tests green. **Commit.**
- [ ] T2: existing scoreboard/name tests stay green; matrix path behavior-identical. **Commit.**
- [ ] T3: full validation matrix (one red case per rule) green. **Commit.**
- [ ] T4: `--roster` + `--brains` → clap conflict error (try_parse_from test). **Commit.**
- [ ] T5: round-trip test green; live run writes a re-loadable dump. **Commit.**
- [ ] T6: docs + SERIES + move to completed/. **Commit.**
- [ ] `cargo fmt` + `cargo clippy -- -D warnings` + full `cargo test` before every commit.
