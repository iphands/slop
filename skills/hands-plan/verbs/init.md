# verb: init — scaffold the plan system into a project

`init [--dir P] [--commit-format "F"] [--packs measurement,parity] <description>`

Creates `context/plans/` (RULES, SERIES, NN_example, NN_example_tracker), `context/AGENTS.md`
(+ `CLAUDE.md` symlink) and the knowledge files whose role nothing else covers — **only files that
do not exist yet**. Then **suggests** root `AGENTS.md`/`CLAUDE.md` additions as a diff and applies
them only on approval. `<description>` is free text: what the project is, its stack, its goal.

All commands run from the project directory (SKILL.md, Route step 2).

## 1. Arguments

- No description → ask for one line: *what is this project, and what is its north star?* Don't
  proceed on a guess.
- Note `--commit-format` and `--packs` if given; they settle the matching questions in step 4.

## 2. Survey (read-only)

```bash
date +%F
"$SKILL_DIR/bin/hp-scan" .
git status --short | head -20
git log -50 --format=%s
ls -la
ls -la context 2>/dev/null || echo "(no context/ yet)"
```

From these and a quick look at the tree, establish:

| Fact | How |
|---|---|
| Git repo? Project dir == git root, or a **sub-project**? | hp-scan `ROOT` (`git=`, `sub=`). Not a repo → warn that commits (the commit rule) can't apply and ask whether to continue; **never `git init` unasked**. |
| Parent conventions (sub-project) | hp-scan `parent_agents`, `parent_context`, `PARENTFILE`. Read the parent agent file's git section (append-only "Git discipline"?) and its `context/` convention. |
| Root agent files and symlink direction | hp-scan `ROOTFILE` (or `ROOTFILE none`). With none in the project dir, the direction to use is the nearest ancestor pair's (`PARENTFILE … link=`), else `CLAUDE.md -> AGENTS.md`. |
| Existing knowledge files and their roles | hp-scan `KNOWLEDGE`. Map each to a role: confirmed facts (`distilled`), bugs/gotchas (`pitfalls`), dependency choices (`high_level`), project-specific logs. **Existing files keep their names**; create a knowledge file only when no file — in the project, or the parent's context for a role the project already records there — serves that role. A sub-project still gets its own `pitfalls.md`/`distilled.md` (project-specific); cross-cutting entries go up. |
| Existing plan system | hp-scan `FILE context/plans/RULES.md exists=y` → **stop**: "This project already has a plan system — run `/hands-plan resync` to see how it differs from v<N>." Numbered plan files but no RULES → **adopt mode** (below). |
| Stray plan documents | `PLAN*.md`, `TODO*.md`, `*_PLAN.md`, `plans/*.md`, `docs/plans/` outside `context/plans/`. Never move them; list them in SERIES (adopt mode) and in the report. |
| Stack, runners, tools | Marker files per `$SKILL_DIR/guides/rule_a.md`; `justfile`/`Makefile`/CI workflows; `command -v` for each tool the gate will need (configured ≠ installed). |
| Commit style | From `git log`: a plan-commit style already in use (`[P<n>][T<n>]…`, `task(TN):`, `task(P<n>-T<n>)`, `[PNN][TNN]`) → **adopt it**. Otherwise the default `[P<n>][T<n>][<topic>] <summary>` for plan work; keep a repo-wide non-plan style (e.g. ≥ 5 commits of conventional `type(scope):`) for non-plan commits. **Sub-project in a monorepo** → recommend the scoped form `[<sub>][P<n>][T<n>][<topic>]` (plan numbers repeat across sub-projects). |
| User-facing doc | `README.md` if present; else whatever the description names; else `README.md`. |
| `vendor/` | Present → third-party source, read-only (goes into `context/AGENTS.md`). |
| Packs | Suggest `measurement` when the description/stack is about performance, profiling, benchmarks, tuning, evaluation; `parity` when it ports/reimplements/emulates/must byte-match an original. Otherwise none (settled — don't ask). |

**Adopt mode** (numbered plans exist, no RULES): keep every existing file and number. Next free =
hp-scan `NEXT computed=`. Build SERIES rows from the existing plans (status from their metadata or
location). Add a *pre-plan history* note if work shipped before any plan existed — **never
fabricate a plan file for it**. List stray plan docs under a `## Retired` note in SERIES only if
the user agrees.

## 3. Draft the decisions

- **Project gate** — write it from `$SKILL_DIR/guides/rule_a.md`: commands in order, mechanical
  pass conditions, blind spots, build-actually-loaded, look-at-it, the full pre-commit set,
  prerequisites, one gate per component. Prefer the project's own runner. Empty repo with an
  unknown stack → ask what "working" means.
- **Run the draft once as a baseline** (guides/rule_a.md). Record the result in the gate text. A
  failing step is not hidden: it becomes the first task of the first plan, and a SERIES standing
  constraint if it needs the human (missing tools). Note every file the run generated.
- **Plan-gate rows** — 2–4 rows naming this project's risky change types ("a new API endpoint",
  "a schema migration", "a new upstream route") marked **Yes**, drawn from the description and tree.
- **Commit format** — from the survey.
- **Packs** — from the survey or `--packs`. For `parity`, the oracle and tier names (defaults in
  `reference/placeholders.md`).

## 4. One decision round

Ask the open decisions **once**, with AskUserQuestion (max 4 questions; skip any settled by flags
or the survey). Recommended option first, marked `(Recommended)`; drafts go in `preview`:

1. Commit format — detected/default vs alternatives.
2. Project gate — the draft with its baseline result (preview) vs "I'll describe it" (Other).
3. Packs — only if the survey suggested one: the suggestion (Recommended) / none.
4. Only if existing knowledge files don't match the default roles — the proposed role mapping.

Steps 7 and 9 later ask to **confirm** what will be written; they are not new decisions.

## 5. Fill and create

Read each template from `$SKILL_DIR/templates/…` and fill **every** `{{PLACEHOLDER}}` per
`$SKILL_DIR/reference/placeholders.md` — including the `{{R_*}}` rule names (a new project gets
the defaults: Rule A gate, B commit, B2 append-only, C lifecycle, D harvest, E evidence). Write
only files that don't exist. **Never overwrite** — an existing target is skipped and routed to
from `context/AGENTS.md`.

| Target | Template |
|---|---|
| `context/plans/RULES.md` | `templates/plans/RULES.md` |
| `context/plans/SERIES.md` | `templates/plans/SERIES.md` |
| `context/plans/NN_example.md` | `templates/plans/NN_example.md` |
| `context/plans/NN_example_tracker.md` | `templates/plans/NN_example_tracker.md` |
| `context/AGENTS.md` | `templates/context/AGENTS.md` |
| `context/CLAUDE.md` | a symlink, in the direction chosen in step 2 (default `ln -s AGENTS.md context/CLAUDE.md`; mirrored pair: real `CLAUDE.md` + `ln -s CLAUDE.md context/AGENTS.md`) |
| `context/distilled.md`, `pitfalls.md`, `high_level.md` | `templates/context/…` — only for roles no existing file covers |

Do **not** create `completed/` or `abandoned/` (close does, with `mkdir -p`). Do not copy
`TOPIC_RULES.md` (`new --series-rules` does, on request). Packs: insert each block from
`$SKILL_DIR/packs/<pack>.md` where its `→` heading says, numbering rules `P1`, `P2`, …

## 6. Verify the scaffold

```bash
"$SKILL_DIR/bin/hp-scan" .
```

Pass conditions: no `PLACEHOLDER` records; `FILE … stamp=v<N>` on RULES, SERIES, NN_example,
NN_example_tracker, context/AGENTS.md; the `context/CLAUDE.md` link resolves (`readlink -f`); no
`DEADREF`; the `gate-project` section has real commands; `NEXT computed=` equals SERIES' next
free number. Fix anything that fails. Re-read RULES.md once end to end — it must read as one
coherent document for *this* project.

## 7. Suggest the root agent-file additions — confirm before writing

Read `$SKILL_DIR/templates/root/agents_block.md` and the project's real root agent file (follow
the symlink). Build a **unified diff** tailored to it:

- **Existing root file:** snippet 1 tree lines inserted into an existing directory tree (real
  knowledge-file names), snippet 2 placed per agents_block.md, and a numbered snippet-3 list of
  sections that restate, contradict, or have gone stale against RULES (heading + first line, each
  with its one-line replacement). Contradictions with append-only git are **CONFLICTs**; text
  stricter than RULES is carried into RULES as an addendum, never dropped.
  Ask: **Apply block + tree lines (Recommended)** / **Apply block + tree + the listed de-dups** /
  **Skip — I'll paste it myself**.
- **No root file:** propose creating it per agents_block.md (title, description, build & verify
  line, snippet 2; symlink direction from step 2). Ask: **Create it (Recommended)** / **Skip**.
- **Sub-project:** the sub-project's own root file as above, including the parent line; then
  separately suggest the one-line pointer for the parent's agent file — apply it only on its own
  approval, since it is outside the project.

Apply exactly what was chosen, to the real file once.

## 8. Suggest `.gitignore` lines (don't write unasked)

Stack caches the gate or probes produce and the `.gitignore` lacks (`target/`, `__pycache__/`,
`.pytest_cache/`, `.ruff_cache/`, `node_modules/`, `dist/`), pack entries (e.g. `/captures/`), and —
only if the user runs ralph loops — `.claude/ralph-loop.local.md`.

## 9. Commit — confirm first

Ask: **Commit the scaffold now (Recommended)** / **Leave it uncommitted**. On yes, stage exactly
the files and symlinks created or edited — re-read each first — and chain:

```bash
git add context/plans/RULES.md context/plans/SERIES.md context/plans/NN_example.md \
        context/plans/NN_example_tracker.md context/AGENTS.md context/CLAUDE.md \
        <created knowledge files> <root file + its symlink, if created/edited> \
  && git commit -m "<non-plan format, e.g. [plans] scaffold context/plans + context/ knowledge base (hands-plan v<N>)>"
```

Never `git add -A`. Files generated by the baseline gate run (lockfiles, caches) are not staged —
they are listed in the report.

## 10. Report

- Files created (and skipped because they existed), the commit hash if committed.
- The decisions: commit format, the gate (one line per step) and its baseline result, packs,
  parent links, prerequisites the human must install.
- Survey findings worth acting on: stray plan docs, generated files, oversized files, dead refs,
  CONFLICTs.
- Next step: `/hands-plan new <first piece of work>` — if the baseline failed, that first plan
  starts by making the gate pass.
