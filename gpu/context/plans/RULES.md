# Plans — Rules & Conventions

> Read this before writing any plan file or tracker file in `context/plans/`.

---

## Plan File Format

### Naming

- `NN_name.md` — two-digit zero-padded number, snake_case name (e.g. `03_replay_harness.md`)
- Sub-plans: `NN_N_name.md` (e.g. `02_1_utrace_json_parser.md`)
- Trackers: `NN_name_tracker.md` — always paired with the plan
- `SERIES.md` — master dependency chain across all plans (no number)

### Metadata Block

Every plan file must open with a title and this metadata block:

```markdown
# Plan NN — [Title]

> **Status**: pending | in-progress | done
> **Created**: YYYY-MM-DD
> **Depends on**: Plan N | N/A
> **Goal**: One-sentence deliverable description.
> **Hardware**: A310 / DG2-G11 / `xe` — note if the plan assumes otherwise.

---
```

The `Hardware` line exists because several plans here are only valid on one kernel
driver or one Mesa build. If a plan requires booting `i915`, or a patched Mesa, say so
in the metadata — not buried in a task.

### Mandatory Header in Every New Plan

Immediately after the metadata block:

```markdown
> **Before starting, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.
```

### Required Sections (in this order)

#### `## TL;DR`

```markdown
**What**: One sentence describing what is being done.

**Deliverables**:
1. Concrete output one
2. Concrete output two

**Estimated effort**: Small (2 h) | Small–Medium (half day) | Medium (1 day) | Large (3 days)
```

#### `## Context`

Background, rationale, prior findings, decisions made. Use H3 subsections for complex
plans:

- `### Prior Measurements` — what earlier plans measured, with capture provenance.
  **Cite the capture, not the memory of it.**
- `### Why [Approach]` — justification for a design choice
- `### Key Facts` — tool syntax, hardware limits, driver behavior. Anything confirmed
  here should also land in `context/distilled.md`.

#### `## Step-by-Step Tasks`

One H3 per task, labeled `T1`, `T2`, etc.:

```markdown
### T1: [Task title]

**File**: `path/to/file.rs`

**What to do**: Detailed instructions.

**Before** / **After**: code blocks for edits, or exact commands for capture tasks.

**Expected observation**: For measurement tasks — what result would confirm the
hypothesis, and what result would refute it. Write this BEFORE running it.
```

The `Expected observation` field is not optional on measurement tasks. Writing down what
would refute the hypothesis *before* collecting the data is the only defense against
reading a win into noise.

#### `## Critical Files`

| File | Change | Priority |
|------|--------|----------|
| `path/to/file.rs` | Description of change | P0 |

Priority values: `P0` = blocking, `P1` = important, `P2` = nice-to-have.

#### `## Open Questions / Risks`

Numbered list. Each point names the risk and suggests a mitigation. For this project,
**always consider**: is the effect larger than run-to-run noise? Does this measurement
survive a reboot? Does it hold on the other kernel driver?

#### `## Verification Checklist`

One checkbox per task, each a **testable assertion**:

```markdown
- [ ] T1: `cargo test` passes, zero clippy warnings
- [ ] T2: `gputop` output covers the full 120 s window with no gaps
- [ ] T3: replaying the same capture 3× gives frame times within 2%
```

"It looks faster" is not a testable assertion. "3 runs, mean improved 8%, spread 2%" is.

---

## Tracker File Format

Every non-trivial plan gets a paired tracker: `NN_name_tracker.md`.

```markdown
# [Plan Title] — Tracker

## Overview
- Status: N% complete (X/Y tasks)
- Start date: YYYY-MM-DD

## Resume Instructions
[How to pick up work if interrupted — task ordering constraints, unknowns to resolve first]

## Measurements

| Date | Capture / build | Metric | Result | Runs | Spread |
|------|-----------------|--------|--------|------|--------|

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: ... | `path/file.rs` | pending | |
```

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped`

The **Measurements** table is mandatory for any plan that produces numbers. It is the
project's lab notebook — a claim not in this table has no provenance and cannot be cited
by a later plan.

Trackers should also record **negative and inconclusive results**. "Tried X, no
measurable effect, 3 runs" is valuable and stops the next session from re-running it.

---

## Per-Task Execution Rules

These apply to **every task** (T1, T2, …) during implementation. They are not optional.

### Rule A — Zero build errors and warnings

For our own code, after completing each task:

1. `cargo build` — exit 0, **zero** errors, **zero** warnings.
2. `cargo clippy -- -D warnings` — exit 0.
3. `cargo test` — green.
4. `cargo fmt` — applied.

For Mesa patches:

1. `meson compile -C build` — exit 0, **zero new warnings** in touched files.
2. The driver actually loads: `VK_DRIVER_FILES=… vulkaninfo | grep driverInfo` reports
   your build, not the distro's. A silently-not-loaded local build is the classic way to
   "measure" a patch that was never running.

**Never mark a task `done` while warnings are outstanding.**

### Rule B — Commit at every task boundary OR MORE FREQUENTLY

**CRITICAL: YOU MUST COMMIT AT EVERY TASK COMPLETION. DO NOT WAIT.**

1. Commit at the end of every task — no exceptions. Smaller commits are welcome.
   **DO NOT WAIT UNTIL A FULL PLAN IS COMPLETE TO COMMIT.**
2. Message format: `task(TN): <short description>`.
3. One task per commit — do not batch unless the changes are inseparable.
4. **YOU MUST COMMIT BEFORE MARKING ANY TASK COMPLETE.** If you haven't committed, you
   haven't finished.
5. Lint, format, and tests must pass before every commit.
6. Git history is **append-only**: no amend, rebase, reset, revert, or force-push.
   Correct a bad commit with a new commit. *(See `../../../CLAUDE.md` — there is an
   incident report attached to this rule.)*

**MANDATORY:** bake commit reminders into the plan's task list.

### Rule C — Move completed plans to `completed/`

**CRITICAL: WHEN A PLAN IS 100% COMPLETE, MOVE IT IMMEDIATELY.**

```bash
git mv context/plans/NN_name.md context/plans/completed/NN_name.md
git mv context/plans/NN_name_tracker.md context/plans/completed/NN_name_tracker.md
```

1. Update `SERIES.md` to mark the plan **done**.
2. **DO NOT LEAVE COMPLETED PLANS IN THE ACTIVE DIRECTORY.**
3. Partially complete (some tasks done, some pending) → **DO NOT MOVE IT**.
4. Abandoned plans go to `abandoned/` **with a recorded reason** in SERIES.md. A plan
   dropped without a stated reason will be re-attempted by someone six months from now.
5. Before starting a new plan, verify the previous one is moved or marked
   deferred/blocked in SERIES.md.

### Rule D — Every number carries provenance and variance

This project's numbers are its product. An unsourced number is worse than no number,
because it gets cited.

1. **N ≥ 3 runs.** Report mean and spread. One run is an anecdote.
2. **A delta inside the spread is "within noise"** — write those words. Do not report it
   as a win, a regression, or a trend.
3. **Name the source**: capture file, driver build (git SHA or `driverInfo` string),
   kernel driver, resolution, and settings. Put it in the tracker's Measurements table.
4. **Do not compare across changed conditions.** Different Mesa build *and* different
   settings in one comparison measures nothing. Change one thing.
5. **Verify the throttle was not active.** Clocks below boost while busy invalidates
   the run; re-measure rather than caveat.
6. **Inconclusive is a valid, recordable outcome.** Record it. Do not round it up.

### Rule E — Capture hygiene

1. Captures, traces, and CSVs go in `captures/` and are **gitignored**. Never commit
   them — they are gigabytes and regenerable.
2. Name captures so they are identifiable six months later:
   `<workload>_<date>_<what-varies>.<ext>`. An orphan `capture.gfxr` is a dead capture.
3. Record in the tracker which capture backs which measurement.
4. **Restore the system after a session**: unset `perf_stream_paranoid`, remove debug
   env vars from Steam launch options, restore the stock Mesa ICD. The next session's
   baseline depends on the machine being in a known state.

---

## Content Style

- **Bold** for important terms; `code` for file names, variables, commands.
- Dates always ISO format: `YYYY-MM-DD`.
- Absolute paths preferred in doc sections; relative paths fine inside task code blocks.
- Code blocks always carry a language specifier (` ```bash `, ` ```c `, ` ```rust `).
- Cross-reference other plans as "Plan N" or "Plan N T2".
- Cite driver/tool source as `path/file.c:line` when stating what a tool does.

---

## Canonical Template

Copy `context/plans/NN_example.md` for every new plan. Rename to `NN_name.md` with the
next zero-padded number and fill in all sections.

For real examples of the live format, browse `context/plans/completed/`.
