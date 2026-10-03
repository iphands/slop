# Root agent-file suggestions (used by `init` and checked by `resync`)

`init` never writes a root `AGENTS.md`/`CLAUDE.md` silently. It builds a **diff tailored to the
existing file** from the three snippets below, shows it, and asks. Follow the project's existing
heading levels, voice and numbering — the snippets are content, not layout.

If the root files are a symlink pair (`CLAUDE.md -> AGENTS.md` or the reverse), edit the real
file once. If neither exists, offer to create `AGENTS.md` containing a one-line project title,
the description, and snippet 2 — plus `CLAUDE.md -> AGENTS.md`.

---

## Snippet 1 — layout-tree lines

Only if the file already has a directory tree (usually a ```` ```text ```` block under
"Workspace Layout" / "Directory Structure"). Insert under the project root, aligned with the
tree's existing columns. No markers here — HTML comments inside a code fence render as text.

```text
├── context/                  # living memory — READ context/AGENTS.md before new work
│   ├── plans/                # plan system — RULES.md is authoritative; read in full
│   │   ├── RULES.md          #   format + Rules A–E + project rules
│   │   ├── SERIES.md         #   dependency chain, status, next free plan number
│   │   ├── NN_example*.md    #   skeletons to copy for a new plan + tracker
│   │   ├── completed/        #   closed plans (git mv here at 100%)
│   │   └── abandoned/        #   dropped plans, reason recorded in SERIES
│   ├── distilled.md          # confirmed facts (provenance-tagged)
│   ├── pitfalls.md           # bugs & gotchas, especially multi-attempt fixes
│   └── high_level.md         # dependency choices: pros/cons, revisit-if
```

Replace the three knowledge lines with the project's real files when they differ (e.g.
`impl-bugs.md`, `<project>-learnings.md`).

---

## Snippet 2 — the marked block

Place it where the file describes how work is done (after "Development Workflow" /
"Getting Started" if present, otherwise near the top, after the project summary). Keep the
markers exactly; `resync` looks for them. `{{COMMIT_FORMAT_SHORT}}` is the Rule B format.

```markdown
<!-- hands-plan:begin v1 -->
## Plans & Context

`context/` is this project's living memory and `context/plans/` its plan system.
**`context/plans/RULES.md` is authoritative** — where this file and RULES.md disagree, RULES.md wins.

- **Before non-trivial work:** read `context/plans/RULES.md` in full (its first table says what
  needs a plan), then `context/plans/SERIES.md` (active plans, next free number, north star).
  New plan = copy `NN_example.md` + `NN_example_tracker.md`, register it in SERIES.
- **Working a plan:** follow the tracker's Resume Instructions → pass the Rule A gate → commit
  `{{COMMIT_FORMAT_SHORT}}` with the tracker row in the same commit. One task per commit.
- **Git:** never push; no co-author trailers; history is append-only — fix a bad commit with a
  new one, and chain `edit && git commit`.
- **Knowledge:** read `context/AGENTS.md` for what lives where; record findings as you go, and
  harvest them before a plan moves to `completed/`.
- **Current state lives in `SERIES.md`**, not in this file.
- **Honesty:** never claim something is done, verified or recorded unless it is — on disk, in the
  command output.
<!-- hands-plan:end -->
```

---

## Snippet 3 — de-duplication hints

Root files tend to restate RULES.md, and the copies drift. In the diff, **list** each of these
that the file contains (quote its heading and first line) and offer the replacement — the user
picks. Never delete project-specific substance (domain rules, architecture, constraints).

| Pattern in the root file | Why it rots | Offer instead |
|---|---|---|
| A "Planning" / "Plans" section restating naming, required sections, tracker columns | Duplicates RULES `plan-format`/`tracker-format`; e.g. "two-digit numbers" survives past plan 99 | One line: "Plans: see the *Plans & Context* section and `context/plans/RULES.md`." |
| A "Commits" section restating commit-every-task / format | Format drifts from RULES Rule B (e.g. `task(TN):` vs the RULES format) | Keep project-only notes; point to RULES Rule B for format and cadence. |
| "Move completed plans to `completed/`" section | Verbatim copy of RULES Rule C | Delete; the block covers it. |
| "Status" / "Current phase" / "Next step" sections | Stale the day after they are written | "Current state lives in `context/plans/SERIES.md`." |
| "Getting Started: create Plan 01 …" on a project past Plan 01 | Stale | Point to SERIES *Currently Active*. |
| References to files that do not exist (`NN_example.md` missing, renamed context files) | Dead references mislead every agent | Fix the path, or create the missing file. |
| Git rules that contradict RULES Rule B2 ("rebase to clean up", "squash", "revert to last good") | Conflict — the human decides | Flag as **CONFLICT**; do not silently pick a side. |
