# hands-plan

A Claude Code skill that drops a numbered **plan + tracker** system and a `context/` knowledge
base into any project or sub-project, and keeps it in step with this skill over time.

It is the plan system that was hand-copied (and drifted) across slop `cache`, `gpu`, `qbots`,
`qctrl`, `game/materia-engine` and `private/containers`, synthesized once, here.

## Install

```bash
ln -s /home/iphands/prog/slop/skills/hands-plan ~/.claude/skills/hands-plan
```

User-invoked only (`disable-model-invocation: true`): type `/hands-plan …`. Read-only helpers
(hp-scan, hp-probe, `git log/status/diff`, `ls`, `grep`, …) are pre-approved while the skill runs;
every write — files, `git add/commit/mv`, `mkdir`, symlinks — still asks.

## Verbs

| Command | What it does |
|---|---|
| `/hands-plan init <one line about the project>` | Scaffold `context/plans/` + `context/AGENTS.md` (+ `CLAUDE.md` symlink) + missing knowledge files; **suggest** a diff for the root `AGENTS.md`/`CLAUDE.md` and apply it only if you say so. Flags: `--commit-format "…"`, `--packs measurement,parity`, `--dir P`. |
| `/hands-plan new <what>` | Research the work, write `NN_name.md` + `NN_name_tracker.md`, register in SERIES, commit on approval. Flags: `--parent NN` (sub-plan), `--series-rules TOPIC`. |
| `/hands-plan status [NN]` | Read-only dashboard + hygiene lint (unmoved finished plans, SERIES drift, stale plans, dead refs, oversized files). |
| `/hands-plan resume [NN]` | Do the next task: gate → tracker → one commit. `--ralph` prints a self-contained `/ralph-loop` command instead. |
| `/hands-plan close NN [--abandon "why"]` | Audit, harvest findings into `context/`, `git mv` to `completed/` or `abandoned/`, update SERIES, one commit. |
| `/hands-plan resync` | Read-only: compare the project (stamped or legacy) with the current templates and print an ordered migration checklist. Execute it with `/hands-plan new migrate the plan system to hands-plan vN`. |
| `/hands-plan` | `status` if the project has a plan system, else help. |

## What `init` creates

```text
<project>/
├── AGENTS.md / CLAUDE.md      # only if you approve the suggested diff (marked hands-plan block)
└── context/
    ├── AGENTS.md              # map of context/, where findings go, provenance tags, house style
    ├── CLAUDE.md -> AGENTS.md # auto-loaded by Claude Code whenever an agent reads context/
    ├── distilled.md           # confirmed facts        ┐ only when no existing file
    ├── pitfalls.md            # bugs & gotchas         │ already serves the role
    ├── high_level.md          # dependency choices     ┘
    └── plans/
        ├── RULES.md           # authoritative: format, per-task rules, Project Rules
        ├── SERIES.md          # north star, next free number, plans table, abandoned table
        ├── NN_example.md      # plan skeleton
        └── NN_example_tracker.md
```

`completed/` and `abandoned/` appear on the first `close`.

## The rules, by section id

Templates never assume a rule letter: they say `{{R_GATE}}`, `{{R_LIFECYCLE}}`, … and each project
resolves them (a new project gets A gate, B commit, B2 append-only, C lifecycle, D harvest,
E evidence; a legacy project keeps its own letters — `reference/placeholders.md`).

| id | Rule | Origin |
|---|---|---|
| `plan-gate` | When does a change need a plan (table) | cache |
| `gate` + `gate-project` | Verification gate: observe it working; keep it current; the project's own gate (baseline-run at init) | all; cache/gpu blind-spot wording |
| `commit` | Commit every task, tracker row in the same commit, explicit paths, never push | qbots/cache/materia |
| `append-only` | No amend/rebase/reset/revert/force-push; fix forward; `&&` chaining | slop CLAUDE.md, cache B2 |
| `lifecycle` | `completed/`, `abandoned/` with a reason, pending plans are hypotheses | qbots/gpu/materia |
| `harvest` | Findings on disk before close | cache D |
| `evidence` | Reality is the oracle; a premise is a claim; below the bar is a row | materia C, gpu D |
| `project-rules` | P1…Pn, project-owned; packs land here | — |

Packs (optional, `--packs`): **measurement** (gpu: N≥3, spread, provenance, capture hygiene) and
**parity** (materia: oracle + tiers, "done means matched").

## Ownership model (what `resync` may touch)

Full table in `reference/sections.md`:

- **core** — the skill's text; resync proposes ADD/UPDATE. Project text inside a core section goes
  under `#### Project addendum` and is always preserved — including anything *stricter* than core.
  Structural parts of SERIES (Next free line, Abandoned table, compaction footer) are core too.
- **seeded** — written by init, owned by the project afterwards (the project gate, plan-gate rows,
  north star, Currently Active); resync checks presence and shape only.
- **local** — Project Rules, SERIES rows and narrative, real plans, knowledge-file content; never
  touched (drift in them is reported as INFO).
- **param** — the commit format; reported against `git log`, never changed unasked.

Sections carry invisible `<!-- hp:<id> -->` markers; files carry a first-line
`<!-- hands-plan:vN date -->` stamp. Projects keep their own rule letters — the skill cites ids.

## Layout

```text
SKILL.md              router + global rules (the only file with ${CLAUDE_SKILL_DIR} substitution)
verbs/<verb>.md       step-by-step instructions per verb
templates/            plans/, context/, root/ — the scaffolded files, with {{PLACEHOLDERS}}
guides/rule_a.md      how init drafts the verification gate
packs/                optional Project Rules packs
reference/sections.md ids, kinds, probes, legacy aliases, never-touch list, known conflicts
reference/placeholders.md  every {{PLACEHOLDER}}, {{R_*}} letters, parent-path rules
bin/hp-scan           read-only inventory + lint (bash/awk, no deps) used by every verb
bin/hp-probe          read-only, section-scoped, markdown-normalized probe matching; --toc lists real headings
bin/hp-selftest       maintainer check of the skill itself (python3)
CHANGELOG.md, VERSION
```

`bin/hp-scan [--brief] [project-dir]` is useful on its own: one tab-separated record per line
(`PLAN`, `NEXT`, `LINT`, `DUP`, `DEADREF`, `SIZE`, `KNOWLEDGE`, …).

## Maintaining the skill

When a template changes:

1. Edit the template. Refer to rules only through `{{R_*}}`; new placeholders go in
   `reference/placeholders.md`.
2. Update its row(s) in `reference/sections.md` — probes, legacy headings, aliases.
3. Bump `VERSION`; add a `CHANGELOG.md` entry (`id · file · change` + `migrate:`).
4. `bin/hp-selftest` must print `OK` (probes hit the templates, placeholders documented, paths
   exist, no hard-coded letters, scripts lint clean).
5. Regression: `bin/hp-scan --brief` on the six legacy projects and `/hands-plan resync`
   (read-only) in cache, qbots and materia; confirm their `git status` is unchanged.
