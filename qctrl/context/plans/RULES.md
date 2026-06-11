# Plans — Rules & Conventions

> Read this before writing any plan file or tracker file in `context/plans/`.

---

## Plan File Format

### Naming

- `NN_name.md` — two-digit zero-padded number, snake_case name (e.g. `65_modelview_skel_fix.md`)
- Sub-plans: `NN_N_name.md` (e.g. `15_1_worldmap_parser_terrain.md`)
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
> **Agent**: implementation agent (ralph-loop) | sub-agent | etc.

---
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

Background, rationale, prior findings, and decisions made. Use H3 subsections for complex plans:

- `### Pre-Identified Bug/Issue` — confirmed bugs documented before coding starts
- `### Why [Approach]` — justification for a design choice
- `### Key Facts` — research findings, format details

#### `## Step-by-Step Tasks`

One H3 per task, labeled `T1`, `T2`, etc.:

```markdown
### T1: [Task title]

**File**: `path/to/file.rs`

**What to do**: Detailed instructions.

**Before**:
```rust
// old code
```

**After**:
```rust
// corrected code
```
```

For large plans, group tasks into parallel waves with an explicit dependency matrix.

#### `## Critical Files`

| File | Change | Priority |
|------|--------|----------|
| `path/to/file.rs` | Description of change | P0 |

Priority values: `P0` = blocking, `P1` = important, `P2` = nice-to-have.

#### `## Open Questions / Risks`

Numbered list. Each point names the risk and suggests a mitigation.

#### `## Verification Checklist`

One checkbox per task, each a testable assertion:

```markdown
- [ ] T1: `cargo test` passes with ≥ 90% coverage on touched modules
- [ ] T2: `./bin/debug-image` confirms humanoid silhouette
```

---

## Tracker File Format

Every non-trivial plan gets a paired tracker: `NN_name_tracker.md`.

```markdown
# [Plan Title] — Tracker

## Overview
- Status: N% complete
- Start date: YYYY-MM-DD
- [Other plan-specific metrics]

## Resume Instructions
[How to pick up work if interrupted]

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: ... | `path/file.rs` | pending | |
```

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped`

---

## Per-Task Execution Rules

These rules apply to **every task** (T1, T2, …) during implementation. They are not optional.

### Rule A — Zero build errors and warnings

After completing each task:

1. Run `cargo build` — must exit 0 with **zero** errors and **zero** warnings.
2. Run `cargo clippy` — must exit 0 with **zero** warnings.
3. If any warnings remain, fix them before marking the task done.
4. **Never mark a task `done` while compiler warnings are outstanding.**

### Rule B — Commit at every task boundary OR MORE FREQUENLTY

1. Make a commit **at the end of every task** — no exceptions! You can make smaller commits too. DO NOT WAIT UNTIL A FULL PLAN IS COMPLETE TO COMMIT (commit ealy and often)
2. Intermediate commits within a task are encouraged for logical checkpoints.
3. Commit message format: `task(TN): <short description>` where `TN` is the plan task number.
   - Example: `task(T1): fix bone mesh pre-translation placement`
   - Example: `task(T2): apply Y/Z coordinate flip to renderer`
4. The commit for a task must include only the changes for that task — do not batch multiple tasks into one commit unless they are inseparable.

NOTE: YOU MUST make sure that linting, auto formating and (in rust) cargo clippy is run before every commit
NOTE: Fix all warnings before each commit
NOTE: All unit tests must pass before each commit!

---

## Content Style

- **Bold** for important terms; `code` for file names, variable names, commands.
- Dates always ISO format: `YYYY-MM-DD`.
- Absolute paths preferred in doc sections; relative paths acceptable inside task code blocks.
- Code blocks always carry a language specifier (` ```rust `, ` ```bash `, etc.).
- Cross-reference other plans as "Plan N" or "Plan N T2".

---

## Canonical Template

Use `context/plans/NN_example.md` as the template for every new plan. Copy it, rename it to
`NN_name.md` (with the next zero-padded plan number), and fill in all sections.

For historical context and real examples of the live format, browse `context/plans/completed/`.
Plans 60–67 are the most recent and reflect current conventions.

---

## Completed Plans

When a plan and its tracker reach 100% completion, move them to `context/plans/completed/`:

```bash
git mv context/plans/NN_name.md context/plans/completed/NN_name.md
git mv context/plans/NN_name_tracker.md context/plans/completed/NN_name_tracker.md
```

Update `SERIES.md` to mark the plan **done** if not already marked.
New plans may look into `context/plans/completed/` for historical context.

---

## Mandatory Header in Every New Plan

Every plan file must include this reminder block immediately after the metadata block:

```markdown
> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.
```
