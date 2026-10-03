# hands-plan — changelog

`resync` reads this to explain **what changed since the version a project was stamped with**, and
how to migrate it. Detection itself lives in `reference/sections.md` (every probe there runs on
every resync); this file is the human-readable *why*.

## Entry format

```
## vN — YYYY-MM-DD
- <section-id> · <file> · added|changed|removed|renamed — <one-line description>
  migrate: <what an existing project should do; "keep the project's letter" for rules>
```

Every entry must also add or update the matching probe row in `reference/sections.md`, and bump
`VERSION`. A template edit with no entry here is invisible to stamped projects. `bin/hp-selftest`
must pass.

**Stamps.** A scaffolded or migrated project carries `<!-- hands-plan:vN YYYY-MM-DD -->` on the
first line of `context/plans/RULES.md`, `SERIES.md`, `NN_example.md`, `NN_example_tracker.md`,
`context/AGENTS.md` (and any `*_RULES.md` created from `TOPIC_RULES.md`), and `vN` in the root
block's `<!-- hands-plan:begin vN -->` marker. Knowledge files and real plans are never stamped.

---

## v1 — 2026-10-03

Baseline, synthesized from the hand-copied plan systems in slop `cache`, `gpu`, `qbots`, `qctrl`,
`game/materia-engine` and `private/containers`. **Projects without a stamp are "legacy"**: resync
runs every probe in `reference/sections.md` against them and maps their sections by legacy heading
or by meaning.

*Revised the same day, before any project used it, after four end-to-end simulations (scratch
init→new→resume→close; a monorepo sub-project with an abandon; read-only resync of qbots and
materia): `{{R_*}}` rule-name placeholders instead of hard-coded letters (section ids `rule-a` /
`rule-a-gate` became `gate` / `gate-project`), section-scoped probes via `bin/hp-probe`, the
started-state flip, plan bookkeeping commit forms, file-relative parent paths, and the hp-scan
accuracy fixes. No project was stamped with the earlier wording.*

**Letters.** v1 adds core rules a legacy project may lack. Each keeps the project's own letter if
it has one; a missing rule takes the template's default letter if free, else the next free letter
(`reference/placeholders.md` § *Assigning letters*). Never re-letter an existing rule.

Relative to the most common legacy shape (qbots/qctrl RULES), v1 adds or changes:

- `rules-header` · RULES.md · added — stamp line; "This file wins" authority clause.
  migrate: add both to the header.
- `plan-gate` · RULES.md · added — "When Does a Change Need a Plan?" table (from cache).
  migrate: add the section with rows drafted from the project's real change types.
- `naming` · RULES.md · changed — "at least two digits", never renumber, check the next number
  against `completed/` and `abandoned/`. migrate: replace "two-digit zero-padded".
- `plan-format` · RULES.md · changed — the plan skeleton moves out of RULES into NN_example only;
  RULES keeps a Section | Must contain table. New hard requirements: **Verify** + **Commit** on
  every task, **How confirmed** on Key Facts, **Expected observation** on measurement tasks,
  verbatim repro for bugs, open questions never deleted, tick only against evidence, `N/A — reason`.
  Optional `Revised` metadata, `Scope`, `Rejected Claims`, waves with exit gates.
  migrate: replace the embedded skeleton with the table; keep project metadata lines (e.g.
  `**Hardware**:`) under `#### Project addendum`.
- `tracker-format` · RULES.md + NN_example_tracker.md · added — tracker template file; statuses
  gain `invalid`; reasons required; Evidence table; Notes / Deviations; Follow-ups; no self-hash.
  migrate: add the template file; update the RULES section.
- `gate` · RULES.md · changed + **renamed heading** ("Zero build errors and warnings" → "The
  verification gate") — core preamble (reading the diff is not verification; blind spots; build
  actually loaded; look at the UI; keep the gate current) above a project-owned `#### Project gate`
  (`gate-project`). migrate: keep the project's gate text verbatim as the Project gate; add the
  missing preamble points; offer to keep the old heading title. If the root file or a `justfile`
  demands more before commits (fmt, tests, coverage), the gate must absorb it (SHAPE).
- `commit` · RULES.md · changed — tracker row in the same commit; explicit-path staging; user
  doc in the same commit as behavior changes; never push / no co-author stated in RULES; plan
  bookkeeping forms (`[P<n>][plan]`, `[revise]`, `[close]`). Default format for new projects
  `[P<n>][T<n>][<topic>] <summary>` (sub-projects: `[<sub>]` prefix). migrate: add missing items;
  keep project NOTE lines that are the only home of a requirement (addendum); never change an
  existing format without asking — when `git log` has drifted from it, report the tally.
- `append-only` · RULES.md · added — ban table (incl. stash/restore/clean on others' work),
  fix-forward, `&&` chaining, commit message as a factual claim, the 2026-07-18 incident (as the
  rule's origin story). migrate: add under its letter; resolve CONFLICTs first; stricter local
  text (materia: never stash/checkout/restore — period) becomes an addendum, never loosened.
- `lifecycle` · RULES.md · changed — `mkdir -p`; fix links after the move; `abandoned/` with a
  recorded reason; parallel plans allowed when listed under Currently Active; pending plans are
  hypotheses (Revised line); the started-state flip on the first task. migrate: merge the
  duplicate "## Completed Plans" section into the rule (and drop root-file copies, e.g. qbots
  §5a); a project with only that unlettered section gets a free letter for it. A legacy "never
  start a new plan before finishing the previous one" is a TIGHTENING — keep it as addendum unless
  the human relaxes it.
- `harvest` · RULES.md · added — Rule D from cache. migrate: add under next free letter if the
  project's D is taken.
- `evidence` · RULES.md · added — oracle, premise-is-a-claim, below-the-bar row, inconclusive is
  recordable (from materia Rule C 2–4 and gpu Rule D.6). migrate: add; if the project already has
  a parity/measurement rule, keep it and add only the missing points.
- `project-rules`, `series-rules` · RULES.md · added — home for P-rules and packs; series-scoped
  `<TOPIC>_RULES.md` convention (generalized from materia REUSE_RULES).
- `next-free`, `north-star`, `active`, `abandoned`, `constraints`, `compaction` · SERIES.md · added.
  migrate: add the missing structure; never rewrite rows or narrative.
- `context-agents` · context/AGENTS.md (+ CLAUDE.md symlink) · added — map, routing table,
  provenance tags, house style, honesty. migrate: create it with routing rows built from the
  project's actual knowledge files.
- `root-block` · root AGENTS.md/CLAUDE.md · added — the marked *Plans & Context* block.
  migrate: insert the block; offer the de-dup list.
- packs: `measurement` (from gpu Rules D/E), `parity` (from materia Rule C tiers) — optional.
  migrate: a project that already has the substance (gpu, materia) needs no action; others get an
  INFO suggestion only.
