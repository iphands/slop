# Plan 54 — Shorten competition bot names with brain/mode/char codes

> **Status**: done
> **Created**: 2026-07-11
> **Depends on**: N/A (independent of Plan 53)
> **Goal**: Competition bot names fit Q2's 15-char limit by using short brain (3),
> navmode (2), and Q3-character (3) codes — e.g. `mai_as_1`, `zb2_fb_1`, `q3_fb_maj_1`.
> **Agent**: sub-agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Replace the full brain/mode/char tag strings in `group_tag` with short codes so
no competition bot name can exceed Q2's 15-char `netname` limit.

**Deliverables**:
1. `mode_code`/`brain_code`/`char_code` fns + rewritten `group_tag`.
2. Updated + new tests (all combos ≤ 15 chars at a realistic index).
3. Refreshed help-text/name examples; optional scoreboard legend.

**Estimated effort**: Small (2 h)

## Context

Q2 truncates player names to **15 usable chars** — the gamecode copies `name` into
`char netname[16]` via `Q_strlcpy` (`vendor/yquake2/src/game/player/client.c:1849`; 16th
byte is the NUL). qbots enforces nothing, so it sends the name verbatim and the server
silently truncates. Competition names `<brain>_<mode>[_<char>]_<i>` (`group_tag`,
`crates/qbots/src/supervisor.rs:530`) overflow fast: `runtester_navmesh_10` = 20,
`q3_fallback_camper_10` = 21, `q3_astar_grunt_1` = 16.

### Key Facts (name parsing coupling)
Only ONE place parses a name back: `mode_scoreboard`'s `rsplit_once('_')`
(`supervisor.rs:561`), which strips only the trailing `_<index>` and looks the remainder up
in a `group_tags`-seeded map (both come from `group_tag`, so they always match). CLI/config
parsing uses clap `ValueEnum` names, **separate** from these tag strings — unaffected.
Keep `brain_tag`/`mode_tag`/`CharPreset::tag` full strings for logs; add new code fns.

### Code scheme (approved)
- Brain (3): main→`mai`, sentry→`sen`, runtester→`run`, q3→`q3`, zb2→`zb2`.
- NavMode (2): astar→`as`, navmesh→`nm`, hybrid-fallback→`fb`, hybrid-race→`rc`,
  hybrid-hier→`hr`, hybrid-segment→`sg`.
- Q3 char (3): grunt→`gru`, major→`maj`, sarge→`sar`, camper→`cam`.
- Longest name: `q3_hr_cam_100` = 13 < 15. ✓

## Step-by-Step Tasks

### T1: short-code fns + rewrite `group_tag`
**File**: `crates/qbots/src/supervisor.rs`
Add `mode_code`/`brain_code`/`char_code` (`&'static str`) with the scheme above; rewrite
`group_tag` to `<brain_code>_<mode_code>[_<char_code>]`. Fix the stale `mode_tag` "skin
label" doc comment and the naming doc comments/examples.

### T2: update + extend tests
**File**: `crates/qbots/src/supervisor.rs`
Update `group_tag_is_brain_first_underscore_joined` → `mai_as`, `q3_rc`, `q3_rc_gru`,
`sen_nm`. Add a combo-enumeration test: every `BrainKind × NavMode × {None, each
CharPreset}` has `group_tag(...).len() + 3 <= 15` (the `_99` index suffix).

### T3: update doc-comment name examples
**File**: `crates/qbots/src/main.rs`
`Cmd::Competition` help examples → `mai_as_1`, `q3_rc_1`, `q3_rc_gru_1`.

### T4: scoreboard legend (optional)
**File**: `crates/qbots/src/supervisor.rs`
Print a one-line code→full-name legend at competition launch so `mai_as` is readable.

### T5: live verify + close
Live smoke (`competition` + `status` shows short, ≤15-char names); fmt/clippy/test green;
move plan+tracker to `completed/`, mark SERIES.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/qbots/src/supervisor.rs` | `*_code` fns, `group_tag`, tests, legend | P0 |
| `crates/qbots/src/main.rs` | competition help-text examples | P1 |

## Open Questions / Risks
1. **Scoreboard readability.** Short codes are terser in the board. Mitigation: T4 legend.
2. **Doc drift.** `AGENTS.md`/`CLAUDE.md` show old example names. Mitigation: refresh if
   quick (P2, non-blocking).

## Verification Checklist
- [x] T1: `cargo build`/`clippy` clean; `group_tag` emits short codes.
- [x] T2: updated + new length-bound tests pass (`cargo test -p qbots`).
- [x] T3: `competition --help` shows short-code examples.
- [x] T4: launch prints a code legend (`name-code legend — brain/mode/char`).
- [x] T5: live `status` showed `q3_fb_maj_8`/`zb2_fb_1`/`mai_as_5` (longest 11, none
      truncated); workspace fmt/clippy/test green (216 tests).
