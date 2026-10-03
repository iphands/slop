# Reference — section ownership, legacy aliases, probes

`resync` and `status` read this file. It is the **single source of probes**: every probe runs on
every resync, whatever the project's stamp says. `CHANGELOG.md` explains *when and why* a probe was
added; this file says *where to look and what for*.

## How to run a probe

```bash
"$SKILL_DIR/bin/hp-probe" <file> --section <id>          -- "<probe>" …   # stamped file
"$SKILL_DIR/bin/hp-probe" <file> --heading '<Legacy ERE>' -- "<probe>" …  # legacy file
"$SKILL_DIR/bin/hp-probe" <file> --lines A-B             -- "<probe>" …   # header blocks / mapped ranges
```

hp-probe scopes the match to the section (heading → next heading of the same or higher level),
strips `*` and backticks, joins wrapped lines, and matches case-insensitively, so `**zero**
warnings` and a phrase wrapped across two lines both match. `a||b` = alternatives. A probe with no
scope column below runs on the whole file.

**Sections are mapped by id, never by letter.** Stamped file → the `<!-- hp:<id> -->` marker.
Legacy file → the *Legacy heading* ERE; if it doesn't find the section, map by meaning (read the
file). A legacy `Rule C` is lifecycle in five projects and evidence in one.

**Reading results.** A section found but a probe missed → **UPDATE** (the section is behind), not
ADD. A section not found at all → **ADD**. A legacy section may hold an older wording that says the
same thing — read it before proposing text.

## Ownership kinds

| Kind | Who owns the text | What `resync` does |
|---|---|---|
| `core` | the skill | Probes → ADD / UPDATE / OK. Project text inside the section goes under `#### Project addendum` and is always preserved. |
| `core-preamble` | the skill, above a seeded sub-section | As `core`, but only for the preamble; the sub-section is `seeded`. |
| `seeded` | the project, after init wrote it | Presence and shape only (SHAPE finding); never reworded. |
| `seeded-optional` | the project | Missing → INFO only. |
| `local` | the project | Never touched. A missing `project-rules` section → ADD of the empty shell (its content is never proposed); other missing `local` sections → INFO. |
| `param` | the project (a value) | Reported, compared with root files and `git log`; never changed without asking. |
| `heuristic` | — (root files) | Not probe-driven: dead refs, restated rules, stale sections. Findings are UPDATE (optional) with the snippet-3 replacement. |

**Addendum vs drift.** Extra text inside a core section is **addendum** (keep, move under
`#### Project addendum`) when it is project substance, **stricter** than the core rule, or **the
only place a requirement is written** (e.g. qbots' Rule B NOTEs are the only place RULES demands
`fmt` and tests). It is **drift** (UPDATE replaces it) only when it is an older, weaker wording of
something the template now says, a stale fact, or a foreign example copied from another project.

---

## `context/plans/RULES.md`

| id | kind | Legacy heading ERE | Probes (feature: "phrase") | Legacy notes |
|---|---|---|---|---|
| `rules-header` | core | `--lines 1-10` | stamp: "hands-plan:v" · authority: "This file wins" · addendum: "Project addendum" | All legacy: "Read this before writing any plan file" — no authority clause. |
| `plan-gate` | seeded | `When Does a Change Need a Plan` | table: "\| change \| plan? \|" · "When in doubt, write the plan" | cache only. Missing elsewhere → ADD with rows drafted from the project's real risky change types. |
| `naming` | core | `^#+ .*Naming` | "Never renumber" · "at least two digits" · "unused across" | `### Naming` under `## Plan File Format` (all). "two-digit zero-padded" past plan 99 (materia) → UPDATE (drift), not CONFLICT. qbots/qctrl examples `65_modelview_skel_fix.md`, `15_1_worldmap…` are foreign (from materia) → drift. |
| `plan-format` | core | `^## Plan (File )?Format` (+ `^## Mandatory Header` for the header probe) | metadata: "Depends on" · revised: "Revised" · header: "re-read context/plans/RULES.md in full" · table: "Must contain" · verify: "the command that exercises\|\|the exact command" · expected: "Expected observation" · repro: "verbatim output" · facts: "How confirmed" · rejected: "Rejected Claims" · scope: "instead of expanding scope" · waves: "exit gate" · questions: "Never delete one\|\|Do not delete a question" · evidence: "only against evidence" · n/a: "N/A —" | Legacy embeds the whole plan skeleton → propose the Must-contain table (the skeleton lives in NN_example). gpu `**Hardware**:` metadata and qbots' "explicit dependency matrix" → addendum. materia's `**Done when**:` on tasks ≈ Verify (alias). |
| `tracker-format` | core | `^## Tracker (File )?Format` | invalid: "skipped \| invalid" · reasons: "carry a reason" · negatives: "negative and inconclusive" · deviations: "Deviations" · evidence: "evidence (required when\|\|measurements table" · no self-hash: "own commit hash" · follow-ups: "Follow-ups" · start flips: "When the first task starts" · template: `file:context/plans/NN_example_tracker.md` | gpu's Measurements table = Evidence. qctrl trackers record their own commit hash → note in INFO. |
| `gate` | core-preamble | `^### Rule [A-Z0-9]+ — (Zero build\|Prove it\|The verification)` | "Reading the diff is not verification" · "zero warnings" · "Know what the gate cannot see\|\|blind to" · "actually loaded\|\|actually load" · "looked at" · "Keep the gate current" · "Never mark a task" | Legacy Rule A body = the project gate → map it to `gate-project` verbatim; propose only the missing preamble points. The heading title changes ("Zero build errors…" → "The verification gate") — a rename; offer to keep the old title. |
| `gate-project` | seeded | the legacy gate rule's body | Shape, against `guides/rule_a.md`'s required parts: commands (a fenced or inline command) · pass conditions · blind spots ("blind") · build loaded · look at it · pre-commit set · prerequisites | Missing parts → SHAPE. Also compare with what the root file / `justfile` demand before commits (fmt, tests, coverage): if they are stricter, that is a finding (the gate must be the full pre-commit set). |
| `commit` | core + param | `^### Rule [A-Z0-9]+ — Commit` | "COMMIT BEFORE MARKING ANY TASK COMPLETE" · "end of every task" · "One task per commit\|\|only the changes for that task" · "Tracker row in the same commit" · "never git add -a" · "passes before every commit\|\|pass before every commit\|\|run before every commit" · "observable behavior" · "Never push" · "co-author" · "baked in\|\|bake commit reminders" | **param** = the format line. Tally `git log --format=%s -300 -- <dir>` variants and report drift (qbots: `task(TN)` documented, ~90 commits use `task(P<n>-T<n>)`, 17 `plan(NN):`). materia: format lives in root AGENTS.md (authority inversion, see below). |
| `append-only` | core | `append-only` (heading, or gpu Rule B item 6) | "append-only" · "git commit --amend" · "git clean" · "new commit" · "looks unpushed" · "Chain edit-then-commit" · "factual claim" · "Why this rule exists" | cache Rule B2 present (misses the `git clean`/stash row). gpu: one line + pointer. qbots/qctrl/materia: absent from RULES (materia's root file has a stricter git section → carry it as addendum). containers: contradicted (CONFLICT). |
| `lifecycle` | core | `^##+ (Rule [A-Z0-9]+ — )?(Move completed\|Completed Plans\|Plan lifecycle)` | move: "git mv context/plans/NN_name.md context/plans/completed/" · "mkdir -p" · "partially complete" · "abandoned/" · "stated reason\|\|recorded reason" · "Currently Active" · "hypothesis, not a contract" · "fix links" | `### Rule C — Move completed plans…` (cache, qbots, containers, gpu); qbots and containers also repeat it as a `## Completed Plans` section → merge. qctrl and materia have only the unlettered `## Completed Plans` (counts as present → UPDATE; give it a free letter). |
| `harvest` | core | `^### Rule [A-Z0-9]+ — Harvest` | "as you go" · "unless the bytes are" · "more than one attempt" | cache Rule D present (misses "as you go"). gpu's Rule D is measurement. Others: absent (the idea lives in root "Knowledge Management"). |
| `evidence` | core | `^### Rule [A-Z0-9]+ — (Evidence\|Done means matched)` | "proves nothing" · "premise is a claim" · "Below the bar" · "Inconclusive" · "path:line\|\|file:line" | materia Rule C items 2–4 (its tier table = the parity pack, already present → no pack action). gpu Rule D.6 ("Inconclusive…"). |
| `project-rules` | local | `^## Project(-Specific)? (Rules\|Conventions)` | — | containers' section holds a CONFLICTING git workflow. gpu Rules D/E and materia's tiers may stay where they are (letters are cited by old plans). |
| `series-rules` | core | `^## Series-Scoped Rules` | "_RULES.md" · "copy from X" | materia `REUSE_RULES.md` is a live (now stale) instance; qbots SERIES "Brain-notes discipline (Plans 23–33…)" is one in disguise. |
| `style` | core | `^## Content Style` | "date +%F" · "carry a language" · "file:line" | All have the section; legacy "Absolute paths preferred…" → addendum. |
| `templates` | core | `^## (Templates & History\|Canonical Template)` | "NN_example_tracker.md" · "newest first" | qctrl, qbots, materia: "Plans 60–67 are the most recent" — stale copy-paste → drift (UPDATE). |

## `context/plans/SERIES.md`

| id | kind | Legacy heading ERE | Probes | Legacy notes |
|---|---|---|---|---|
| `series-header` | core | `--lines 1-15` | stamp: "hands-plan:v" · trigger: "Update it whenever a plan is added\|\|Update this file whenever" · hypothesis: "hypothesis" | materia: "should be treated as a hypothesis" ✓. |
| `next-free` | core | (whole file) | "Next free plan number" | cache only (and stale there). Value = hp-scan `NEXT computed=` at execution time. |
| `north-star` | seeded | (whole file) | "North star" | qbots, gpu ✓. materia's arc statement (SERIES.md:3) counts — suggest labelling it. |
| `active` | seeded | `Currently Active` | "Currently Active\|\|Active set" | materia buries it in a blockquote; qbots "Active set" lists closed plans → SHAPE. |
| `plans` | local | (table with a Status column) | — | All have one. Row fixes are the project's (status lint), never resync's. |
| `abandoned` | core | `^## (Abandoned\|Retired)` | "\| reason \|" | gpu ✓; containers `## Retired` ✓ (alias). qbots/materia: narrative only → ADD (seed rows from existing notes as a suggestion). |
| `constraints` | seeded-optional | `^## Standing Constraints?` | — | containers ✓. Missing → INFO with candidates. |
| `compaction` | core | (whole file; legacy footer `^## Completed Plans`) | "SERIES_ARCHIVE" | Footer text only (cache/qctrl: a `## Completed Plans` footer → replace). Doing the compaction is a separate, human-approved task. |

## `context/plans/NN_example.md`, `NN_example_tracker.md`

| id | kind | Probes (whole file) | Legacy notes |
|---|---|---|---|
| `example-plan` | core | `file:context/plans/NN_example.md` · stamp "hands-plan:v" · "EVERY section" · "## Scope\|\|SCOPE CONSTRAINT" · "verbatim\|\|State the evidence" · "How confirmed" · "Rejected Claims" · "verify:" · "Expected observation" · "commit:" · "only against evidence" · "git mv'd to completed/" | Missing in qbots/qctrl (their RULES and root files reference it → dead refs). Legacy `// old code` blocks and "≥ 90% coverage" lines are foreign/drift. |
| `example-tracker` | core | `file:context/plans/NN_example_tracker.md` · "Resume Instructions" · "row in the same commit" · "Deviations" · "Follow-ups" · "## Evidence" | No legacy project has one → ADD. |

## `context/AGENTS.md` (+ `CLAUDE.md` symlink)

| id | kind | Probes | Legacy notes |
|---|---|---|---|
| `context-agents` | core | `file:context/AGENTS.md\|\|file:context/CLAUDE.md` · stamp "hands-plan:v" · "authoritative" · "unless the bytes are" | No legacy project has one. Build its rows from hp-scan `KNOWLEDGE` records — the project's **actual** files (materia: `impl-bugs.md`, `materia-learnings.md`, topical `ff7-*.md`, the decomp KB; qbots: `distilled/`, `brain_notes.md`, `acceptance.md`, …). A role served by the parent's context (qbots dependency choices → `../../context/high_level.md`) routes there; never create a parallel file. |
| `context-map`, `context-routing` | seeded | rows exist; every path they name exists | Never reworded. |
| `context-provenance` | core | "never upgrade a tag" | The tag set itself is seeded — a project may use its own evidence vocabulary (materia: rig / exe table / decomp / capture). |
| `context-style` | core | "40 KB" · "No full source" | slop root CLAUDE.md `./context` rules. |

## Root `AGENTS.md` / `CLAUDE.md`

| id | kind | Check | Notes |
|---|---|---|---|
| `root-block` | core | markers "hands-plan:begin v" + "hands-plan:end"; version vs skill VERSION | UPDATE shows the new block. Legacy: no markers → ADD + snippet 3. Place after "Development Workflow", never next to a section snippet 3 removes. |
| `root-tree` | heuristic | the fenced tree's `context/` lines name files that exist | hp-scan cannot see inside fences — read it (qbots tree names a missing `high_level.md`). |
| `root-refs` | heuristic | hp-scan `DEADREF` | qctrl: `NN_example.md`, `distilled.md`, `01_setup.md`; qbots: `high_level.md`. |
| `root-restated` | heuristic | restated rules agree with RULES | qbots §5a = Rule C copy; qbots §4 claims RULES' gate is stricter (it isn't); materia "Plans" restates naming/minimum sections/tracker columns. |
| `root-stale` | heuristic | Status / Phase / Getting Started agree with SERIES | qctrl "Phase: Planning / Scaffolding" with 13 plans shipped; qbots "Getting Started" lists Plans 01–07. |

---

## Never touch (resync never proposes edits to these)

- Any real plan or tracker (`NN_*.md` other than the two `NN_example*` files), anything in
  `completed/` or `abandoned/` — report metadata/row drift as INFO only.
- SERIES rows, narrative, milestones, backlog — only the structural items above (adding a missing
  *structure* such as the Next-free line or the Abandoned table is allowed; editing rows is not).
- The *content* of knowledge files; `local` and `seeded` text; `#### Project addendum` blocks.
- Rule **letters** — never renumber. Missing core rules get letters per
  `reference/placeholders.md` § *Assigning letters*.

## Known patterns

**CONFLICT** — local text contradicts a core rule. Quote it; offer options; the human decides.

| Pattern | Seen in | Conflicts with |
|---|---|---|
| "Rebase to clean up history", "squash", "minor tweaks can be squashed into the original commit" | containers RULES (Git Workflow, Rule B.6) | `append-only` |
| "REVERT to last known good commit" / "Emergency Recovery" | containers RULES | `append-only` |
| A reviewer persona that must sign off before a task is done ("Neckbeard AND Hoodie") | containers RULES Rule B.6–7 | `commit` (commit before marking done) — ask whether it is real |
| RULES defers a core param to the root file ("See Commit message format in AGENTS.md") | materia | `rules-header` authority — options: move it into RULES as addendum / qualify the clause |

**SHAPE** — a seeded section is weaker than it must be. Report what is missing; the project words the fix.

| Pattern | Seen in |
|---|---|
| The root file or a `justfile` mandates a pre-commit step the RULES project gate lacks (coverage ≥ 90%, `just all`, fmt/tests) — the gate must be the full pre-commit set | materia (coverage), qbots (fmt/test), cache (`stats/` cargo + npm) |
| One gate for a multi-component repo; no "look at it" step for a UI | cache (`stats/` dashboard, `containers/`) |

**TIGHTENING** — local text is stricter than core. Not a conflict: keep it as `#### Project
addendum` (or offer to relax it, the human's call).

| Pattern | Seen in |
|---|---|
| "NEVER start a new plan without finishing / deferring the previous one" | qbots, cache, containers (legacy Rule C.5) |
| "never stash/checkout/restore/reset/clean — period" | materia AGENTS.md |

**DRIFT** — stale or foreign text; UPDATE replaces it.

| Pattern | Seen in |
|---|---|
| "Plans 60–67 are the most recent and reflect current conventions" | qctrl, qbots, materia |
| Examples naming another project's files (`65_modelview_skel_fix.md`, "humanoid silhouette", "bone mesh") | qctrl, qbots |
| "two-digit zero-padded" numbering in a project past plan 99 | materia RULES + AGENTS.md |
