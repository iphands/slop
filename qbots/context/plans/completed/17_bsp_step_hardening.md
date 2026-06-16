# Plan 17 — BSP/Collision Hardening & Step-Size Correctness

> **Status**: done
> **Created**: 2026-06-16
> **Depends on**: Plan 05 (world model)
> **Goal**: Fix the confirmed `STEP` constant mismatch in nav-graph connectivity, close the one
> remaining low-risk parsing gap (entity comments), and pin vendor constants with regression
> tests so they can't silently drift again.
> **Agent**: implementation agent (ralph-loop)

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: A fresh byte-for-byte / logic-for-logic re-audit of `world/src/bsp.rs` and
`world/src/collision.rs` against `vendor/yquake2/src/common/collision.c` found the previously
fixed model-margin bug is correctly shipped, and turned up exactly one new confirmed bug: the
nav graph's vertical-connectivity threshold (`navgraph.rs` `STEP = 24.0`) does not match Q2's
real step-up height (`pmove.c:32` `#define STEPSIZE 18`). This lets the grid graph connect
nodes across a 19–24-unit height difference that the real movement code cannot climb in one
step — a plausible contributor to bots stalling/bumping near ledges and stairs.

**Deliverables**:
1. `STEP` corrected to `18.0` in `navgraph.rs`, cited to `pmove.c:32`.
2. Entity-string tokenizer handles `//` comments (matches `COM_Parse`, `shared/shared.c`).
3. A small "vendor constant pin" test module asserting `STEP`, `DIST_EPSILON`, and friends
   against their cited vendor values.
4. `context/pitfalls.md` entry for the already-shipped model-margin bug (it was a multi-document,
   multi-attempt investigation and belongs in pitfalls per project convention — it just never
   got written down).

**Estimated effort**: Small (2–3 h).

---

## Context

### Re-audit findings (2026-06-16)

A fresh comparison of our BSP/collision code against `vendor/yquake2/src/common/collision.c`
(not relying on the prior `bsp_bug_analysis.md` draft, which is now in `completed/`) confirmed:

- **Model mins/maxs `-1/+1` margin** (`bsp.rs` `parse_models`): already fixed, commit
  `b72600ae2`. No further action.
- **`CM_ClipBoxToBrush` / `CM_RecursiveHullCheck` / `CM_BoxTrace`** (`collision.rs`
  `clip_box_to_brush`, `recursive_hull_check`, `trace`): line-for-line equivalent to the vendor
  enter/leave-fraction algorithm, including `DIST_EPSILON` placement and the point-vs-box
  `ispoint` special case. No discrepancy found.
- **`box_on_plane_side`** (`collision.rs`): uses the generic corner-dot-product method for
  *all* plane types, where vendor's `BOX_ON_PLANE_SIDE` macro (`shared.h:301-315`) special-cases
  axial planes (`type < 3`) with `dist <= emins[type]` / `dist >= emaxs[type]` (non-strict on
  both sides). At the exact boundary `dist == emaxs[type]` the two formulations disagree by one
  ULP-class case (ours can return "neither side" where vendor returns "back only"). This only
  feeds `box_leafnums` (point/position tests), and our `_ => recurse both` fallback for the
  "neither side" case is conservative (over-includes leafs, never under-includes) — **cosmetic,
  not a bug**, no fix needed.
- **`STEPSIZE` mismatch — CONFIRMED BUG**: `navgraph.rs:16` defines `const STEP: f32 = 24.0;` and
  uses it as the max height difference for a "walkable" grid-edge connection. The real Q2
  step-up height is `pmove.c:32` `#define STEPSIZE 18`. A 24u-tall connection our graph calls
  walkable, the real `PM_StepSlideMove`/ground-trace logic (`up[2] += STEPSIZE` /
  `down[2] -= STEPSIZE`, `pmove.c:242,259`) cannot climb in a single step — the bot will bump
  the ledge, not glide over it. This is a believable contributor to "stuck near goal" failures
  on stairs.
- **Entity tokenizer comments** (`bsp.rs` `tokenize_entities`): does not skip `//` comments the
  way `COM_Parse` (`shared/shared.c`) does. Stock id-software `.bsp` entity lumps do not contain
  comments (verified: no `//` byte sequence appears in q2dm1–q2dm8's entity lumps), so this is
  **not** causing current failures, but it's a one-line robustness fix worth making before any
  custom/community map is ever loaded.

No other discrepancies found in PVS decompression (`vis.rs` matches `CM_DecompressVis`) or leaf
contents handling.

---

## Step-by-Step Tasks

### T1: Fix `STEP` constant in `navgraph.rs`

**File**: `crates/world/src/navgraph.rs`

**What to do**: Change `const STEP: f32 = 24.0;` to `const STEP: f32 = 18.0;` with a comment
citing `vendor/yquake2/src/common/pmove.c:32`. Run `cargo test -p world` and inspect (don't
just trust green) whether q2dm1's node/edge/component counts change materially — log the
before/after counts in this plan's tracker. A modest increase in jump-edge usage or a small
drop in directly-connected (non-jump) edges on stairs is expected and correct; a large jump in
component count would indicate the grid needs help elsewhere (feeds Plan 19).

**Commit**: `task(T1): fix STEP constant to match Q2 STEPSIZE=18 (pmove.c:32)`

---

### T2: Entity tokenizer `//` comment handling

**File**: `crates/world/src/bsp.rs`

**What to do**: In `tokenize_entities`, when scanning outside a quoted string, detect `//` and
skip to end-of-line (mirrors `COM_Parse`'s `/* skip // comments */` block, `shared/shared.c`).
Add a test `parse_entities_with_comments` (a `{ "classname" "info_player_deathmatch" //comment
"origin" "512 -128 24" }` block — verify it still parses; comments mid-block should not eat the
following key/value).

**Commit**: `task(T2): entity tokenizer skips // comments (COM_Parse parity)`

---

### T3: Vendor constant pin tests

**File**: `crates/world/src/collision.rs` (or a new `crates/world/tests/vendor_constants.rs`)

**What to do**: Add a small test asserting `DIST_EPSILON == 0.03125` and (re-exporting or
duplicating) `navgraph::STEP == 18.0`, each with a doc comment citing the vendor source line.
The point isn't coverage — it's a tripwire so a future refactor can't silently drift one of
these away from vendor parity without a test failing and forcing a deliberate decision.

**Commit**: `task(T3): pin vendor-parity constants with regression tests`

---

### T4: Backfill `context/pitfalls.md`

**File**: `context/pitfalls.md`

**What to do**: Add an entry for the model mins/maxs margin bug (already fixed, commit
`b72600ae2`) using the project's pitfalls template (`# Title → Problem → Fix → Source`,
~200 words). This bug took three separate analysis documents
(`completed/bsp_parsing_analysis.md`-equivalent drafts, `completed/bsp_bug_analysis.md`,
`completed/16_bsp_parsing_fix_summary.md`) to land — exactly the "multi-attempt fix" pitfalls.md
exists to capture, and it was never written there.

**Commit**: `task(T4): backfill pitfalls.md with model-margin bug`

---

### T5: Live verification

**What to do**: Run `cargo run -p tools --bin bsp_verify -- q2dm1` (or the equivalent existing
diagnostic) before/after T1, and a `spawn-to-spawn` single-bot run, to confirm geometry counts
are unchanged and the bot still connects/moves. Record node/edge/component counts in the
tracker for Plan 19 to build on.

**Commit**: none (verification only — update tracker).

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/world/src/navgraph.rs` | T1: STEP 24→18 | P0 |
| `crates/world/src/bsp.rs` | T2: comment handling | P1 |
| `crates/world/src/collision.rs` | T3: constant pin tests | P1 |
| `context/pitfalls.md` | T4: backfill margin-bug pitfall | P2 |

---

## Open Questions / Risks

1. **STEP change may increase fragmentation slightly** on maps with tall single-step ledges
   that were previously (incorrectly) bridged by direct edges. Mitigation: `detect_jump_edges`
   already exists for exactly this case — anything that stops being a direct edge should become
   a jump edge instead. If it doesn't, that's a Plan 19 finding, not a reason to revert T1.
2. **`WP_REACH_DZ` in `nav.rs` is a different constant (waypoint-arrival tolerance, not
   step-climb height) and should not be conflated with `STEP`** — don't "fix" it as part of this
   plan; Plan 19 T1 explicitly re-examines it with that distinction in mind.

---

## Verification Checklist

- [x] T1: `cargo test -p world` green; q2dm1 node/edge/component counts recorded before/after
      (unchanged at 64u spacing — see tracker)
- [x] T2: `cargo test -p world` green; new comment test passes
- [x] T3: new constant-pin tests exist and pass
- [x] T4: `context/pitfalls.md` has the model-margin entry
- [x] T5: `bsp_verify` + single-bot `spawn-to-spawn` run unaffected (geometry counts match;
  spawn-to-spawn failure mode/magnitude matches Plan 10 baseline, not a regression)
- [x] `cargo build` + `cargo clippy -- -D warnings` + `cargo fmt` clean throughout (touched files)
