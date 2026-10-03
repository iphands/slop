# verb: resync — compare with the current templates, show a migration plan (read-only)

`resync [--dir P]`

Compares the project's plan system — stamped by an older hands-plan, or a hand-made legacy one —
with the skill's current templates and prints an ordered migration checklist. **It changes
nothing**: no edits, no `mkdir`, no commits, no plan files. Executing the migration is a separate,
deliberate step (`/hands-plan new migrate …`, step 10).

## 1. Load the skill side

Read `$SKILL_DIR/VERSION`, `$SKILL_DIR/CHANGELOG.md`, `$SKILL_DIR/reference/sections.md` (ids,
kinds, probes, legacy headings, addendum-vs-drift, never-touch, known patterns),
`$SKILL_DIR/reference/placeholders.md` (how to fill the text you propose, `{{R_*}}` letters),
`$SKILL_DIR/guides/rule_a.md` (the gate's required parts), `$SKILL_DIR/packs/*.md`, and every
template under `$SKILL_DIR/templates/`.

## 2. Load the project side

```bash
"$SKILL_DIR/bin/hp-scan" --brief .                  # incl. COMMITFMT: commit-format shapes in use
"$SKILL_DIR/bin/hp-probe" <root agent file> --toc   # real headings (fenced code skipped)
```

Read in full: `context/plans/RULES.md`, `NN_example.md`, `NN_example_tracker.md`,
`context/AGENTS.md`, any `*_RULES.md` (hp-scan `RULESFILE`). Read SERIES' header and structure, not
every row. For the root agent file (follow its symlink), read the sections that matter — layout
tree, workflow/planning/knowledge/commit/verification/git/status sections (from `hp-probe --toc`);
a 50 KB root file is not read end to end. Note each file's stamp (`FILE … stamp=`): `vN` =
stamped, `none` = legacy. No `context/plans/` at all → suggest `/hands-plan init`; numbered plans
but no RULES → suggest `/hands-plan init` (adopt mode).

## 3. Map sections — by id, never by letter

- Stamped file → by `<!-- hp:<id> -->` marker (hp-scan `MARKER`); a marker that is missing falls
  back to the legacy heading ERE (and its absence is itself a finding: add the marker).
- Legacy → by the *Legacy heading ERE* in `reference/sections.md`; if that finds nothing, by
  meaning (read the file). An unlettered section that does the job (materia's `## Completed
  Plans`) counts as **present** → UPDATE, not ADD.
- Build the project's **letter map** now: which letter each existing rule uses, and the letters
  missing core rules will get (`reference/placeholders.md` § *Assigning letters*). Every `{{R_*}}`
  in text you propose resolves through it.
- Extra text inside a core section → addendum or drift, per `reference/sections.md`. When in
  doubt, keep it as addendum: dropping the only home of a requirement silently loosens the rules.

## 4. Probe

Run every probe in `reference/sections.md` through hp-probe, scoped to the mapped section, e.g.:

```bash
"$SKILL_DIR/bin/hp-probe" context/plans/RULES.md --heading '^### Rule [A-Z0-9]+ — Commit' -- \
  "COMMIT BEFORE MARKING ANY TASK COMPLETE" "Tracker row in the same commit" "never git add -a"
```

**Aggregate per section id** — one checklist item per section, listing its missed features — and
classify:

| Class | Meaning | Applies to |
|---|---|---|
| **OK** | every probe hit | any |
| **ADD** | the section is missing entirely | `core`; `seeded` when missing; `local`: only the empty `project-rules` shell (other `local` → INFO); `seeded-optional` → INFO |
| **UPDATE** | the section exists but probes missed (older wording, missing items) | `core`, `core-preamble`; root `heuristic` items (optional) |
| **SHAPE** | present but malformed — the project gate lacks required parts from `guides/rule_a.md`, or is weaker than what the root file / `justfile` demand before commits; SERIES lacks Next free | `seeded` |
| **CONFLICT** | local text contradicts a core rule (`reference/sections.md` *Known patterns*) | any |
| **TIGHTENING** | local text is stricter than core — keep as addendum (or offer to relax; the human decides) | any |
| **INFO** | worth knowing, no action proposed | any |

For each ADD/UPDATE give the text: either verbatim, or `template <file>:<lines> verbatim` plus the
listed adaptations (letters, the project's commit format, its gate, its paths) — whichever is
shorter to review. Items that are *pure* "template lines verbatim + letter map" may be grouped
into one item per file; keep the full why/text/risk form for anything with an addendum, a rename,
a TIGHTENING, or a risk. Note heading **renames** explicitly (old plans may cite titles). Use
`CHANGELOG.md` entries newer than the project's stamp (all of them for legacy) for the *why* and
the migrate hint.

## 5. Packs and params

- **Packs:** if the project already has the substance of a pack (materia's tier rule ≈ parity; gpu
  Rules D/E ≈ measurement) → INFO "present, no action". If the project clearly fits one it lacks →
  INFO suggestion with the pack's blocks.
- **Commit format (param):** compare RULES, the root file, and hp-scan `COMMITFMT`.
  Drift (history uses forms RULES doesn't document) → INFO with the counts and a question for the
  human; never change the format unasked.

## 6. Structure, hygiene, active plans

- From hp-scan: missing files (`NN_example_tracker.md`, `context/AGENTS.md` + symlink),
  `PLACEHOLDER`s, `DUP`s, `SIZE`, `DEADREF`s. From the SERIES probes: missing structure (Next free
  line, Abandoned table, compaction footer).
- SERIES **row** problems (`LINT series-*`, `next-free-stale`) and archive metadata drift
  (`meta-location-closed`) → **INFO only**; their fixes belong to `status` / the human, not to the
  migration.
- `context/AGENTS.md` (ADD): build its rows from hp-scan `KNOWLEDGE` — the project's actual files;
  route a role the parent's context already serves there; never propose a parallel file. Symlink
  direction per `reference/placeholders.md` § *Symlink direction*.
- **Active plans:** if an active plan's Critical Files include RULES.md, SERIES.md or the root
  agent file, say so and recommend sequencing the migration after it.

## 7. Root agent files

hp-scan `ROOTFILE … block=`: missing → ADD snippet 2 (+ snippet 1 tree lines); older than skill
VERSION → UPDATE with the new block. Then, per `templates/root/agents_block.md` snippet 3: dead
references (hp-scan `DEADREF`, plus the fenced layout tree, which hp-scan can't see), sections
restating RULES that now disagree with it, authority inversions (RULES deferring to the root file),
stricter root text (carry it into RULES as addendum before any de-dup), stale Status / Phase /
Getting Started sections.

## 8. Respect the never-touch list

Never propose edits to real plans/trackers, `completed/`, `abandoned/`, SERIES rows and narrative,
knowledge-file content, `local`/`seeded` text, `#### Project addendum` blocks, or rule letters.

## 9. Output (chat only)

```
hands-plan resync — <project>   (<sub-project of <parent>; parent rules <rel>/CLAUDE.md, parent context <rel>/context> | standalone)
Local: <vN | legacy (unstamped)>   Skill: v<N>
Files: RULES ✓  SERIES ✓  NN_example ✗  tracker template ✗  context/AGENTS.md ✗  root block ✗  <TOPIC>_RULES: <list|—>
Letter map: gate=A commit=B append-only=<B2|new X> lifecycle=<C|new X> harvest=<…> evidence=<…>
Summary: <k> CONFLICT · <k> TIGHTENING · <k> ADD · <k> UPDATE · <k> SHAPE · <k> INFO
```

Then the checklist, ordered **CONFLICT → TIGHTENING → core (RULES) → structure (SERIES, templates,
context/AGENTS.md) → root files → INFO**. Each item:

```
[UPDATE] RULES.md · hp:commit (Rule B) — CHANGELOG v1
  why:  missing: tracker row in the same commit; explicit-path staging; never push
  text: template RULES.md:<lines> verbatim, with {{COMMIT_FORMAT}} = <project format>, {{R_GATE}} = Rule A
  risk: <e.g. "heading title changes — old plans cite it by letter only">
```

CONFLICT items quote the local line (`file:line`) and offer 2–3 resolutions; never pick one. End
with a short **order of execution** (CONFLICTs and TIGHTENINGs first; files that reference each
other land together; stamps last).

## 10. Close

End with exactly this, filled:

> **Nothing was changed.** To execute this migration under the plan rules:
> `/hands-plan new migrate the plan system to hands-plan v<N>` — `new`'s migrate mode turns this
> checklist into tasks (CONFLICTs first); its last task adds the `<!-- hp:<id> -->` marker to every
> mapped heading (including sections left untouched) and stamps `hands-plan:v<N>` on RULES, SERIES,
> both `NN_example*` files, `context/AGENTS.md`, and the root block's begin marker.
