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
`VERSION`. A template edit with no entry here is invisible to stamped projects.

---

## v1 — 2026-10-03

Baseline, synthesized from the hand-copied plan systems in slop `cache`, `gpu`, `qbots`, `qctrl`,
`game/materia-engine` and `private/containers`. **Projects without a stamp are "legacy"**: resync
runs every probe in `reference/sections.md` against them and maps their sections by meaning.

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
- `rule-a` · RULES.md · changed — core preamble (reading the diff is not verification; blind
  spots; build actually loaded; look at the UI) above a project-owned `#### Project gate`.
  migrate: keep the project's gate text verbatim as the Project gate; add the missing preamble
  points.
- `commit` · RULES.md · changed — tracker row in the same commit; explicit-path staging; user
  doc in the same commit as behavior changes; never push / no co-author stated in RULES.
  Default format for new projects `[P<n>][T<n>][<topic>] <summary>`. migrate: add missing items;
  never change an existing project's format without asking.
- `append-only` · RULES.md · added — ban table (incl. stash/restore/clean on others' work),
  fix-forward, `&&` chaining, commit message as a factual claim, the 2026-07-18 incident.
  migrate: add as Rule B2 (or next free letter); resolve CONFLICTs first.
- `lifecycle` · RULES.md · changed — `mkdir -p`; `abandoned/` with a recorded reason; parallel
  plans allowed when listed under Currently Active; pending plans are hypotheses (Revised line).
  migrate: merge the duplicate "## Completed Plans" section into the rule.
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
