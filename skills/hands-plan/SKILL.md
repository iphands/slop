---
name: hands-plan
description: Numbered plan + tracker system under context/plans/ (RULES.md, SERIES.md, NN_example.md, completed/, abandoned/) with a context/ knowledge base. Verbs — init (scaffold into a new project or sub-project), new (draft the next plan + tracker), status (read-only dashboard + hygiene lint), resume (execute the next task under the rules), close (complete or abandon a plan), resync (compare a project's plan system with the current templates and show a migration plan, never applying it).
argument-hint: "<init|new|status|resume|close|resync> [args]"
disable-model-invocation: true
allowed-tools: Bash(${CLAUDE_SKILL_DIR}/bin/hp-scan:*), Bash(${CLAUDE_SKILL_DIR}/bin/hp-probe:*), Bash(git log:*), Bash(git status:*), Bash(git ls-files:*), Bash(git diff:*), Bash(git rev-parse:*), Bash(date:*), Bash(readlink:*), Bash(realpath:*), Bash(ls:*), Bash(grep:*), Bash(head:*), Bash(wc:*), Bash(command -v:*)
---

# hands-plan

**Arguments:** `$ARGUMENTS`

**SKILL_DIR = `${CLAUDE_SKILL_DIR}`** — wherever a verb file says `$SKILL_DIR/…`, it means this
literal path. Verb files are not substituted; only this file is.

## Route

1. Split the arguments: the **first word is the verb**, the rest are its arguments. Pull out a
   `--dir <path>` option if present: it sets the **project directory** (default: the current
   working directory — not the git root; a sub-project's `context/` lives in the sub-project).
2. **`cd` into the project directory first.** Every path and command in the verb files is relative
   to it (`git add context/plans/…`, `mkdir -p context/plans/completed`).
3. Verbs:

   | Verb | Args | Does | Writes? |
   |---|---|---|---|
   | `init` | `[--dir P] [--commit-format F] [--packs measurement,parity] <description>` | Scaffold `context/plans/` + `context/AGENTS.md`; **suggest** root AGENTS.md/CLAUDE.md additions | only missing files; root files only on approval; commit on approval |
   | `new` | `[--dir P] [--parent NN] [--series-rules TOPIC] <what>` | Research + draft the next `NN_name.md` + tracker, register in SERIES | yes (asks before commit) |
   | `status` | `[--dir P] [NN]` | Dashboard of active plans + hygiene lint | **never** |
   | `resume` | `[--dir P] [NN] [--ralph]` | Load rules/plan/tracker, do the next task under the rules | yes (one task + commit) |
   | `close` | `[--dir P] NN [--abandon "reason"]` | Audit, harvest, move to `completed/`/`abandoned/`, update SERIES | yes (asks before commit) |
   | `resync` | `[--dir P]` | Compare against the current templates, print a migration plan | **never** |

4. **No arguments** → run `status` if `<project>/context/plans/RULES.md` exists, else print the
   table above plus "Start with `/hands-plan init <one line about the project>`."
   **Unknown verb** → print the table; do not guess what was meant.
5. Otherwise **Read `$SKILL_DIR/verbs/<verb>.md` in full and follow it.** Read other skill files
   (templates, guides, packs, reference) only when the verb file says to.

## Global rules (every verb)

- **Never push. No co-author trailers.** The human pushes after review.
- **Git history is append-only**: never `--amend`, `rebase`, `reset --hard`, `revert`,
  `push --force`, and never `stash` / `checkout -- <path>` / `restore` / `clean` work you did not
  make — unless the human asks for it in that moment. A wrong commit is fixed by a new commit.
- **Chain edit-then-commit** (`… && git commit …`) and stage **explicit paths** (`git add <paths>`,
  never `-A`). Re-read every file a commit message describes before writing the message.
- **Never overwrite an existing file.** Create what is missing; for anything that exists, show a
  diff and ask.
- **Root `AGENTS.md`/`CLAUDE.md`, and anything outside the project directory (a parent's files):
  suggest, ask, then edit.** Never silently.
- **`status` and `resync` never write** — nothing in the project or the skill, no commits, no
  `mkdir`. (hp-scan's `mktemp` scratch lives outside both and is removed on exit.)
- **Leave no artifacts.** Probes and research runs must not litter the tree (`__pycache__/`,
  build dirs, lockfiles); prefer read-only probes (`PYTHONDONTWRITEBYTECODE=1`, temp dirs). Report
  any generated file; never stage it unasked.
- **Dates come from `date +%F`**, never from memory.
- **Rules are cited by section id** (`hp:append-only`, `hp:lifecycle`, …) or by the project's own
  heading — never by a letter you assume. Templates carry `{{R_*}}` placeholders that resolve to
  each project's letters (`$SKILL_DIR/reference/placeholders.md`).
- **The project's `RULES.md` governs execution.** Where it disagrees with the skill's templates,
  follow the project and *report* the disagreement (suggest `/hands-plan resync`).
- **Honesty:** never claim something is written, verified, moved or committed unless the command
  ran and you checked its result.
- Helpers (read-only): `"$SKILL_DIR/bin/hp-scan" [--brief] [dir]` — inventory + lint;
  `"$SKILL_DIR/bin/hp-probe" <file> [--section id|--heading ERE|--lines A-B] -- "<probe>" …` —
  section-scoped probe matching. If one fails, fall back to `ls`/`grep` and say so.
