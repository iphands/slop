# verb: init — scaffold the plan system into a project

`init [--dir P] [--commit-format "F"] [--packs measurement,parity] <description>`

Creates `context/plans/` (RULES, SERIES, NN_example, NN_example_tracker), `context/AGENTS.md`
(+ `CLAUDE.md` symlink) and the knowledge files whose role nothing else covers — **only files that
do not exist yet**. Then **suggests** root `AGENTS.md`/`CLAUDE.md` additions as a diff and applies
them only on approval. `<description>` is free text: what the project is, its stack, its goal.

## 1. Arguments

- No description → ask for one line: *what is this project, and what is its north star?* Don't
  proceed on a guess.
- Note `--commit-format` and `--packs` if given; they skip the matching questions in step 4.

## 2. Survey (read-only)

Run these and keep the results; everything below depends on them:

```bash
date +%F
"$SKILL_DIR/bin/hp-scan" "<project-dir>"
git -C "<project-dir>" status --short | head -20
git -C "<project-dir>" log -50 --format=%s 2>/dev/null
ls -la "<project-dir>" "<project-dir>/context" 2>/dev/null
```

From them and a quick look at the tree, establish:

| Fact | How |
|---|---|
| Git repo? Project dir == git root, or a **sub-project**? | hp-scan `ROOT` (`git=`, `sub=`). Not a repo → warn that Rule B (commits) can't apply and ask whether to continue; **never `git init` unasked**. |
| Parent conventions (sub-project) | hp-scan `parent_agents` / `parent_context`. Read the parent agent file's git section; note whether it has an append-only "Git discipline" rule and a `context/` convention. |
| Root agent files and symlink direction | hp-scan `ROOTFILE` (`CLAUDE.md -> AGENTS.md` or the reverse, or neither). |
| Existing `context/` files and their roles | List `context/*.md`. Map each to a role: confirmed facts (`distilled`), bugs/gotchas (`pitfalls`), dependency choices (`high_level`), plus project-specific logs. **Existing files keep their names**; only create a knowledge file when no existing file serves that role. |
| Existing plan system | hp-scan `FILE context/plans/RULES.md exists=y` → **stop**: "This project already has a plan system — run `/hands-plan resync` to see how it differs from v<N>." Numbered plan files but no RULES → **adopt mode** (below). |
| Stray plan documents | `PLAN*.md`, `TODO*.md`, `*_PLAN.md`, `plans/*.md`, `docs/plans/` outside `context/plans/`. Never move them; list them in SERIES (adopt mode) and in the report. |
| Stack and runners | Marker files per `$SKILL_DIR/guides/rule_a.md` (Cargo.toml, package.json, pyproject.toml, go.mod, CMake/meson, Dockerfile/compose/nginx, `*.sh`), plus `justfile`/`Makefile`/CI workflows. |
| Commit style | From `git log`: `[P<n>][T<n>]…` / `task(TN):` / `[PNN][TNN]` already in use → **adopt it**. Otherwise default to `[P<n>][T<n>][<topic>] <summary>` for plan work and keep the repo's existing style (e.g. conventional `type(scope):`) for non-plan commits. Empty history → the default. |
| User-facing doc | `README.md` if present; else whatever the description names; else `README.md`. |
| `vendor/` | Present → it is third-party source, read-only (goes into `context/AGENTS.md`). |
| Packs | Suggest `measurement` when the description/stack is about performance, profiling, benchmarks, tuning, evaluation; `parity` when it ports/reimplements/emulates/must byte-match an original. Otherwise none. |

**Adopt mode** (numbered plans exist, no RULES): keep every existing file and number. Next free =
hp-scan `NEXT computed=`. Build SERIES rows from the existing plans (status from their metadata or
location). Add a *pre-plan history* note if work shipped before any plan existed — **never
fabricate a plan file for it**. List stray plan docs under a `## Retired` note in SERIES only if
the user agrees.

## 3. Draft the decisions

- **Project gate** (Rule A) — write it from `$SKILL_DIR/guides/rule_a.md`: commands in order, pass
  conditions, blind spots, build-actually-loaded, look-at-it, one gate per component. Prefer the
  project's own runner. Empty repo with an unknown stack → ask what "working" means.
- **Plan-gate rows** — 2–4 rows naming this project's risky change types ("a new API endpoint",
  "a schema migration", "a new upstream route") marked **Yes**, drawn from the description and tree.
- **Commit format** — from the survey.
- **Packs** — from the survey or `--packs`. For `parity`, the oracle and tier names (defaults:
  `stub` → `behaves` (same results on the same inputs) → `exact` (value/byte/frame-exact on the
  reference set) → `verified`).

## 4. One question round

Ask **once**, with AskUserQuestion, only what is still open (max 4 questions; skip any already
settled by flags or an unambiguous survey). Put the recommended option first, marked
`(Recommended)`, and show drafts in `preview`:

1. Commit format — detected/default vs alternatives.
2. Rule A project gate — the draft (preview) vs "I'll describe it" (Other).
3. Packs — multiSelect: measurement / parity / none.
4. Only if existing context files don't match the default roles — the proposed role mapping.

## 5. Fill and create

Read each template from `$SKILL_DIR/templates/…`, replace every `{{PLACEHOLDER}}` (table below),
and Write it to the project. **Create only missing files. Never overwrite** — if a target exists
(e.g. `context/pitfalls.md`), skip it and route to it from `context/AGENTS.md`.

| Target | Template |
|---|---|
| `context/plans/RULES.md` | `templates/plans/RULES.md` |
| `context/plans/SERIES.md` | `templates/plans/SERIES.md` |
| `context/plans/NN_example.md` | `templates/plans/NN_example.md` |
| `context/plans/NN_example_tracker.md` | `templates/plans/NN_example_tracker.md` |
| `context/AGENTS.md` | `templates/context/AGENTS.md` |
| `context/CLAUDE.md` | symlink → `AGENTS.md` (`ln -s AGENTS.md context/CLAUDE.md`); if the root pair links the other way (`AGENTS.md -> CLAUDE.md`), mirror it: real `CLAUDE.md`, `AGENTS.md -> CLAUDE.md` |
| `context/distilled.md`, `pitfalls.md`, `high_level.md` | `templates/context/…` — only for roles no existing file covers |

Do **not** create `completed/` or `abandoned/` (close does, with `mkdir -p`). Do not copy
`TOPIC_RULES.md` (`new --series-rules` does, on request). Packs: insert each block from
`$SKILL_DIR/packs/<pack>.md` where its `→` heading says, numbering rules `P1`, `P2`, …

### Placeholders

Author-time placeholders (`NN`, `[Title]`, `YYYY-MM-DD`, `<…>`, `path/to/file.ext`) in
`NN_example*.md` **stay** — they are filled when a plan is written. Every `{{…}}` must go:

| Placeholder | Where | Value |
|---|---|---|
| `{{DATE}}` | stamps, SERIES north star | `date +%F` output |
| `{{PROJECT_NAME}}` | SERIES, context files | the name from the description, else the directory name |
| `{{PLAN_GATE_ROWS}}` | RULES `plan-gate` | rows `\| <change type> \| **Yes.** <why> \|`, one per line |
| `{{PROJECT_GATE}}` | RULES `rule-a-gate` | the approved gate (numbered steps, fenced commands, pass conditions, blind spots) |
| `{{COMMIT_FORMAT}}` | RULES Rule B item 3 | the format in backticks + one sentence + 2 example sub-bullets, e.g. `` `[P<n>][T<n>][<topic>] <imperative summary>` — plan number (not zero-padded), task, short area tag. Outside a plan: `[<topic>] <summary>`. `` then `   - Example: \`[P3][T2][parser] reject truncated headers\`` |
| `{{USER_DOC}}` | RULES Rule B item 6 | `` `README.md` `` (or the real user-facing doc) |
| `{{PARENT_GIT_RULE}}` | RULES end of B2 | sub-project whose parent has a git-discipline rule → `> Full rule: [\`<rel>/CLAUDE.md\`](<rel>/CLAUDE.md) § Git discipline.` with `<rel>` relative to `context/plans/` (e.g. `../../..`); otherwise delete the line |
| `{{PROJECT_RULES}}` | RULES `project-rules` | pack rule blocks, or `_None yet — add **P1** when a project-specific rule earns its place._` |
| `{{CODE_LANG}}` | RULES style, NN_example | the main language fence: `rust`, `python`, `ts`, `go`, `bash`, `nginx`, … |
| `{{NEXT_FREE}}` | SERIES | `01`, or hp-scan `NEXT computed=` in adopt mode |
| `{{NORTH_STAR}}` | SERIES | one or two sentences from the description — the outcome, not the tasks |
| `{{ORDERING_PRINCIPLE}}` | SERIES | one line if the description implies one ("measure before optimizing"), else `TBD — set when the second plan is written.` |
| `{{ACTIVE}}` | SERIES | `None yet — start with \`/hands-plan new <first piece of work>\`.` (adopt mode: the in-progress plans) |
| `{{PLAN_ROWS}}` | SERIES | adopt mode: one row per existing plan; otherwise **delete the line** (empty table) |
| `{{CONSTRAINTS}}` | SERIES | standing constraints from the description/survey (e.g. "this workstation is not the server"), else `None recorded yet.` |
| `{{VERIFY_CMD}}` | NN_example | the gate's main command line(s) |
| `{{COMMIT_EXAMPLE}}` | NN_example, NN_example_tracker | the format instantiated for the template, e.g. `` `[PNN][T1][<topic>] <short description>` `` |
| `{{CONTEXT_MAP_ROWS}}` | context/AGENTS.md map | one row per knowledge file that exists after init (real names), e.g. `` \| `pitfalls.md` \| Bugs & gotchas, esp. multi-attempt fixes \| Before new work; after any multi-try fix \| `` — plus `` `../context/` `` when a parent context exists |
| `{{ROUTING_ROWS}}` | context/AGENTS.md routing | rows for the core roles pointing at the **actual** files: confirmed fact → `distilled.md` (or its equivalent), bug/gotcha → `pitfalls.md`, dependency choice → `high_level.md`, plus project-specific logs found in the survey |
| `{{PARENT_CONTEXT}}` | context/AGENTS.md routing | sub-project with a parent `context/` → `` \| Anything that generalizes beyond this project \| also `<rel>/context/pitfalls.md` (or `<lib>.md`, `high_level.md`) \| Same shape; keep project-specific detail here \| ``; else delete the line |
| `{{VENDOR_LINE}}` | context/AGENTS.md style | `vendor/` exists → `- \`vendor/\` holds third-party source clones: **read-only** reference; never write docs into it — distill into \`context/\`.`; else delete the line |
| `{{PARENT_PITFALLS}}` | pitfalls.md | sub-project with a parent context → `Cross-cutting entries also go up to \`<rel>/context/pitfalls.md\`; project-specific ones stay here.`; else delete the line |
| `{{ORACLE}}`, `{{TIER_*}}` | parity pack | from step 4 / the defaults above |
| `{{COMMIT_FORMAT_SHORT}}` | root block | the bare format, e.g. `[P<n>][T<n>][<topic>] <summary>` |

## 6. Verify the scaffold

```bash
"$SKILL_DIR/bin/hp-scan" "<project-dir>"
```

Pass conditions: no `PLACEHOLDER` records; `FILE … stamp=v<N>` on RULES, SERIES, NN_example,
NN_example_tracker, context/AGENTS.md; `context/CLAUDE.md` link resolves (`readlink -f`); the
`rule-a-gate` section has real commands; `NEXT computed=` equals SERIES' next free number. Fix
anything that fails before going on. Re-read RULES.md once end to end — it must read as one
coherent document for *this* project.

## 7. Suggest the root agent-file additions — do not write yet

Read `$SKILL_DIR/templates/root/agents_block.md` and the project's real root agent file (follow
the symlink). Build a **unified diff** tailored to that file:

- Snippet 1 tree lines, inserted into an existing directory tree (if there is one), using the
  project's real knowledge-file names.
- Snippet 2, the marked block, placed after the workflow/getting-started section (or near the top),
  with `{{COMMIT_FORMAT_SHORT}}` filled. Match the file's heading levels.
- Snippet 3: a numbered list of sections in the file that restate, contradict, or have gone stale
  against RULES (quote heading + first line), each with its proposed one-line replacement.
  Contradictions with append-only git are **CONFLICTs** — never resolved silently.
- No root agent file → propose creating `AGENTS.md` (title, the description, snippet 2) and
  `CLAUDE.md -> AGENTS.md`.

Show the diff, then ask (AskUserQuestion): **Apply block + tree lines (Recommended)** / **Apply
block + tree + the listed de-dups** / **Skip — I'll paste it myself**. Apply exactly what was
chosen, to the real file once.

## 8. Suggest `.gitignore` lines (don't write unasked)

Packs' entries (e.g. `/captures/`), and `.claude/ralph-loop.local.md` if the user runs ralph loops.

## 9. Commit — ask first

Ask: **Commit the scaffold now (Recommended)** / **Leave it uncommitted**. On yes, stage exactly
the files created or edited — re-read each first — and chain:

```bash
git add context/plans/RULES.md context/plans/SERIES.md context/plans/NN_example.md \
        context/plans/NN_example_tracker.md context/AGENTS.md context/CLAUDE.md <created knowledge files> <root file if edited> \
  && git commit -m "<outside-plan format: e.g. [plans] scaffold context/plans + context/ knowledge base (hands-plan v<N>)>"
```

Never `git add -A`; never touch pre-existing uncommitted changes.

## 10. Report

- Files created (and skipped because they existed), the commit hash if committed.
- The decisions: commit format, the gate (one line per step), packs, parent links.
- Survey findings worth acting on: stray plan docs, oversized files, dead refs, CONFLICTs.
- Next step: `/hands-plan new <first piece of work>`.
