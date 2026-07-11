# Plan 62 — Xonotic roster, tuning & acceptance (capstone)

> **Status**: pending
> **Created**: 2026-07-11
> **Depends on**: Plan 60 (`xon` brain), Plan 61 (`xg` navmode), Plan 21 (competition), Plan 47 (acceptance suite)
> **Goal**: Field `xon` as a roster of named 12-axis personalities selectable in fleet/competition, fold `xon` and `xg` into the Plan 47 acceptance matrix with a recorded baseline, and run the live tuning loop.
> **Agent**: implementation agent (ralph-loop)

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: The Plan 38 pattern for xon: roster selection everywhere, distinct names/skins, acceptance-matrix rows, iterative live tuning with the noise-floor aggregator.

**Deliverables**:
1. `--xonchar` (connect-one/spawn-to-*), `[fleet].xonchar`, `competition --xonchars` matrix — with `char_code` in bot names (15-char netname limit, Plan 54 codes).
2. Acceptance matrix rows for `xon` (4 traversal rows × brain) and an `xg` navmode column entry; baseline recorded in `context/acceptance.md`.
3. A tuning pass over the 4 presets with multi-run K/D + control noise floor; results in `mode_perf.md`.
4. Optional (time-boxed): strategy-token supervisor plumbing if Plan 60's timing log showed rating-session clustering.

**Estimated effort**: Medium (1 day + live tuning loop)

## Context

- Plan 38 precedent: roster+tuning kept separate because the brain plan is already Large; tuning without the aggregator is noise (Plan 30's variance lesson — control kd 1.00→2.60 on identical code).
- `XonSkill` presets + `char_code()` exist since Plan 59 T1; Plan 60 T1 wired `--xonchar` for connect-one. This plan finishes fleet/competition surfaces and makes the personalities *measurably* distinct.
- Acceptance driver: `crates/tools/src/bin/acceptance.rs` (Plan 47) — matrix = traversal rows × brains, per-map batches, pass/fail + exit code; EVT counters (weapon thrash, drown, lava_escape, wall_press) are the behavior gates that caught real bugs on first runs. A new brain plugs in as a group name.

## Step-by-Step Tasks

### T1: roster selection surfaces

**Files**: `crates/qbots/src/main.rs`, `crates/qbots/src/config.rs`, `crates/qbots/src/supervisor.rs`

**What to do**: `competition --xonchars <list>` (ValueEnum, Plan 56 style — auto possible-values); `[fleet].xonchar` per-bot; group naming `xon_<mode>_<char>_<i>` via `char_code` (≤3 chars; verify ≤15 total with the Plan 54 legend log); distinct skin per preset (`XonSkill::skin()`).

**Verify live**: `competition --brains xon --xonchars rusher,sharp,turtle,noob --count 1` — four distinctly named/skinned bots connect; legend logged.

**Commit**: `task(T1): xon roster selection (--xonchars, fleet config, names/skins)`

### T2: acceptance-matrix integration + baseline

**Files**: `crates/tools/src/bin/acceptance.rs` (row/group config if hardcoded), `context/acceptance.md`

**What to do**: Add `xon` to the traversal rows (s2s, swim, ride, lift — the Plan 47 4×N matrix) and record a full live baseline run; add one `xg` A/B batch (q3 brain, `as` vs `xg`). Any new named findings (the Plan 47 convention) get documented, not silently passed. Gate: xon rows reach; EVT behavior counters clean (0 drown, thrash under threshold).

**Commit**: `task(T2): xon/xg acceptance matrix rows + recorded baseline`

### T3: tuning loop

**Files**: preset constants in `crates/brain/src/xonchar.rs`, results in `context/mode_perf.md`

**What to do**: N=3+ 5-min competitions `--brains q3,xon --xonchars all` with the aggregator + `--control q3`: (a) presets are *distinguishable* (rusher ≠ turtle in K/D / deaths / item pickups beyond the noise floor — or document that they aren't and why, the Plan 38 "spread stands as-is" precedent); (b) best xon preset lands within q3's band. Adjust axis offsets only (not brain logic); each adjustment = a commit with the run evidence in the message or tracker.

**Commit**: `task(T3): xon preset tuning pass (evidence-recorded)`

### T4: optional strategy token (time-boxed ½ day)

**Files**: `crates/qbots/src/supervisor.rs`, `crates/brain/src/brains/xon/goals.rs`

**What to do**: ONLY if Plan 60's rating-session timing log shows clustering/CPU spikes with 8 bots: a shared `AtomicU32` round-robin token in the supervisor (empty-goal bots first, per `bot.qc:784-811`), consulted by `XonGoals` before a session. Otherwise mark `skipped` with the timing evidence.

**Commit**: `task(T4): fleet strategy token for xon rating sessions` (or tracker `skipped` note)

### T5: docs + closeout

**Files**: `context/brain_notes.md`, `context/acceptance.md`, `README`, SERIES, plan+tracker

**What to do**: Dated brain_notes entry (preset spreads, tuning deltas, token decision); README brain/navmode lists mention xon/xg; SERIES → done; `git mv` both files to `completed/`. This closes the Xonotic series (58–62).

**Commit**: `task(T5): xon series docs; close Plan 62`

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/qbots/src/main.rs` | `--xonchars` | P0 |
| `crates/qbots/src/supervisor.rs` | group naming + optional token | P0 |
| `crates/qbots/src/config.rs` | `[fleet].xonchar` | P1 |
| `crates/tools/src/bin/acceptance.rs` | xon rows / xg batch | P0 |
| `context/{acceptance,mode_perf,brain_notes}.md` | baselines + tuning record | P1 |

## Open Questions / Risks

1. **Tuning is variance-limited** (Plan 30/47 lesson). *Mitigation*: aggregator + control group mandatory; no conclusion from a single run; time-box the loop and record "indistinguishable" honestly if so.
2. **Acceptance runtime** grows with each brain (4 rows × now 5 brains × maps). *Mitigation*: per-map batches already supported; if wall-clock hurts, add xon to the regression contract but run the full matrix only at series close.
3. **Preset name collisions** with q3 chars in competition names. *Mitigation*: `char_code` namespace is per-brain (names embed brain code first: `xon_fb_rus_1`); verify legend disambiguates.

## Verification Checklist

- [ ] T1: 4-preset competition connects w/ distinct names/skins + legend; all ≤15 chars
- [ ] T2: acceptance matrix xon rows pass live; xg A/B batch recorded; EVT gates clean; `acceptance.md` updated
- [ ] T3: N≥3-run tuning table in `mode_perf.md` w/ control noise floor; preset spread documented either way
- [ ] T4: token shipped with evidence OR skipped with timing evidence
- [ ] T5: brain_notes entry; README updated; plans 58–62 all in `completed/`; SERIES rows done
- [ ] Whole plan: zero warnings, clippy clean, fmt, tests green at every commit
