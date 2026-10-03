# verb: resync — compare with the current templates, show a migration plan (read-only)

`resync [--dir P]`

Compares the project's plan system — stamped by an older hands-plan, or a hand-made legacy one —
with the skill's current templates and prints an ordered migration checklist. **It changes
nothing**: no edits, no `mkdir`, no commits, no plan files. Executing the migration is a separate,
deliberate step (`/hands-plan new migrate …`, step 9).

## 1. Load the skill side

Read `$SKILL_DIR/VERSION`, `$SKILL_DIR/CHANGELOG.md`, `$SKILL_DIR/reference/sections.md` (ids,
kinds, probes, legacy aliases, never-touch list, known conflicts), every template under
`$SKILL_DIR/templates/`, and `$SKILL_DIR/templates/root/agents_block.md`.

## 2. Load the project side

```bash
"$SKILL_DIR/bin/hp-scan" "<project-dir>"
```

Read in full: `context/plans/RULES.md`, the header and structure of `SERIES.md` (not every row),
`NN_example.md`, `NN_example_tracker.md`, `context/AGENTS.md`, any `*_RULES.md`, and the root agent
file (following its symlink). Note each file's stamp (`FILE … stamp=`): `vN` = stamped, `none` =
legacy. No `context/plans/` at all → say so and suggest `/hands-plan init`; numbered plans but no
RULES → this is an *adopt*: suggest `/hands-plan init` (adopt mode) instead.

## 3. Map sections — by id, never by letter

- Stamped file → map each section by its `<!-- hp:<id> -->` marker (hp-scan `MARKER`).
- Legacy file, or a marker missing → map by **meaning**, using the *Legacy aliases* column of
  `reference/sections.md`. A legacy `Rule C` may be lifecycle or evidence; read it.
- Text inside a core section that the template doesn't have: decide **addendum** (project-specific
  substance — keep, and suggest moving it under `#### Project addendum`) or **drift** (an older
  wording of something the template now says better — UPDATE).

## 4. Probe

For every row in `reference/sections.md`, run its probes against the mapped file (`grep -iF`, or a
file-exists check). All probes run on every resync, whatever the stamp. Classify each feature:

| Class | Meaning | Applies to |
|---|---|---|
| **OK** | probe found | any |
| **ADD** | section or feature missing | `core`; `seeded`/`local` only when the section is missing entirely |
| **UPDATE** | present, but behind the template (older wording, missing items) | `core` only |
| **SHAPE** | present but malformed (e.g. Rule A gate has no commands; SERIES lacks Next free) | `seeded` |
| **CONFLICT** | local text contradicts a core rule (see the *Known CONFLICT patterns* table) | any |
| **INFO** | worth knowing, no action required (oversized files, legacy duplicates) | any |

For each ADD/UPDATE, take the **exact text** from the current template, adapted to the project:
its own rule letter (never renumber — missing core rules take the next free letter + their `hp:`
marker), its commit format, its gate. Use `CHANGELOG.md` entries newer than the project's stamp (all
of them for legacy) for the *why* and the migrate hint.

## 5. Structure and hygiene

From hp-scan: missing files (`NN_example_tracker.md`, `context/AGENTS.md` + symlink, Next free
line, Abandoned table), `PLACEHOLDER`s, `DUP`s (report only), `SIZE` (SERIES compaction / file
splits), `LINT series-*` and `next-free-stale` (SERIES rows to fix), metadata/location mismatches.
For `context/AGENTS.md`, build the proposed routing rows from the project's **actual** knowledge
files (never propose creating a parallel `distilled.md` when e.g. `impl-bugs.md` already serves).

## 6. Root agent files

hp-scan `ROOTFILE … block=`: block missing → propose snippet 2 (+ snippet 1 tree lines); block
older than skill VERSION → UPDATE with the new block text. Then: `DEADREF`s; sections that restate
RULES and now disagree with it (commit format, numbering, lifecycle); stale Status / Phase /
Getting Started sections — each with the replacement from snippet 3.

## 7. Respect the never-touch list

Never propose edits to real plans/trackers, `completed/`, `abandoned/`, SERIES rows and narrative,
knowledge-file content, `local`/`seeded` text, `#### Project addendum` blocks, or rule letters.

## 8. Output (chat only)

```
hands-plan resync — <project>
Local: <vN | legacy (unstamped)>   Skill: v<N>   Files: RULES ✓  SERIES ✓  NN_example ✓  tracker template ✗  context/AGENTS.md ✗  root block ✗
Summary: <k> CONFLICT · <k> ADD · <k> UPDATE · <k> SHAPE · <k> INFO
```

Then the checklist, grouped and ordered **CONFLICT → core (RULES) → structure (SERIES, templates,
context/AGENTS.md) → root files → INFO**. Each item:

```
[ADD] RULES.md · hp:append-only (as "Rule B2", next free letter after B) — CHANGELOG v1
  why:  <one line>
  text: <the exact block to insert, or a unified diff for UPDATE>
  risk: <e.g. "old plans cite Rule C as 'completed/'; letter unchanged">
```

CONFLICT items quote the local line (`file:line`) and offer 2–3 resolutions; never pick one.

## 9. Close

End with exactly this, filled:

> **Nothing was changed.** To execute this migration under the plan rules:
> `/hands-plan new migrate the plan system to hands-plan v<N>` — it will turn this checklist into
> tasks (resolve CONFLICTs first; the last task stamps the files `hands-plan:v<N>`).
