# Reference — section ownership, legacy aliases, detect probes

`resync` (and `status`) read this file. It is the **single source of probes**: every probe here is
checked on every resync, whatever the project's stamp says. `CHANGELOG.md` explains *when and why*
a probe was added; this file says *what to look for*.

## Conventions

**Section ids.** Every skill-owned heading carries an invisible marker, e.g.
`### Rule B2 — Git history is append-only <!-- hp:append-only -->`. Projects keep their own rule
letters; the skill, verbs and CHANGELOG cite **ids, never letters**. In a stamped project, map a
section by its marker. In an unstamped (legacy) project, map it by **meaning**, using the aliases
below — never by letter (legacy `Rule C` is "completed/" in five projects and "done means
matched" in one).

**Ownership kinds.**

| Kind | Who owns the text | What `resync` does |
|---|---|---|
| `core` | the skill | Runs the probes; proposes **ADD** (missing) or **UPDATE** (present but behind the template). Text under a `#### Project addendum` sub-heading inside a core section is project-owned and always preserved. |
| `seeded` | the project, after init wrote it | Checks **presence and shape** only (e.g. the gate has commands). Never proposes rewording. |
| `local` | the project | Never touched. At most: "section missing". |
| `param` | the project (a value) | Reports the value and whether root files / git history agree with it. |

**Probe syntax.** `"phrase"` = case-insensitive fixed-string match (`grep -iF`) in the stated
file. `"a" | "b"` = any of them. `file:<path>` = the file exists. Probes are written to sit on one
line of the template — keep it that way when editing templates.

**Legacy projects** the aliases were taken from: slop `cache`, `gpu`, `qbots`, `qctrl`;
`game/materia-engine` (+ its older fork `game/repo-agent-b`); `private/containers`.

---

## `context/plans/RULES.md`

| id | kind | Canonical heading | Probes (feature: probe) | Legacy aliases / notes |
|---|---|---|---|---|
| `rules-header` | core | (H1 + blockquote) | stamp: `"hands-plan:v"` · authority: `"This file wins"` | All legacy: `"Read this before writing any plan file"` — present but no authority clause. |
| `plan-gate` | seeded | When Does a Change Need a Plan? | table: `"When Does a Change Need a Plan"` | Only cache has it (identical heading). Others: missing → ADD with rows drafted from the project. |
| `naming` | core | Files & Naming | never-renumber: `"Never renumber"` · ≥2 digits: `"at least two digits"` · collision check: `"unused across"` | `### Naming` under `## Plan File Format` (all). Legacy says "two-digit zero-padded" — CONFLICT-lite once a project passes plan 99 (materia at 145). |
| `plan-format` | core | Plan Format | metadata: `"**Depends on**"` · revised: `"**Revised**"` · header: `"Before writing any code"` · Section/Must-contain table: `"Must contain"` · verify: `"**Verify**"` · expected obs: `"Expected observation"` · repro: `"verbatim"` · how confirmed: `"How confirmed"` · rejected claims: `"Rejected Claims"` · scope: `"instead of expanding scope"` · waves: `"exit gate"` · never delete Q: `"Never delete one"` \| `"Do not delete a question"` · tick on evidence: `"only against evidence"` · N/A: `"N/A —"` | `## Plan File Format` + `### Metadata Block` + `### Required Sections (in this order)` + `## Mandatory Header in Every New Plan` (all legacy). Legacy RULES embed the full plan skeleton — propose replacing it with the Must-contain table (the skeleton lives in NN_example). gpu adds `**Hardware**:` metadata → keep as addendum. |
| `tracker-format` | core | Tracker Format | template file: `file:context/plans/NN_example_tracker.md` · invalid status: `` "`invalid`" `` · reasons: `"always carry a reason"` · negative results: `"negative and inconclusive"` · deviations: `"Deviations"` · evidence table: `"Evidence"` \| `"Measurements"` · no self-hash: `"own commit hash"` · follow-ups: `"Follow-ups"` | `## Tracker File Format` (all). gpu's `## Measurements` table = Evidence (alias). cache's `## Notes / Deviations` ✓. qctrl trackers record their own commit hash (impossible with row-in-same-commit) → note, not CONFLICT. |
| `rule-a` | core preamble | Rule A — The verification gate | not-diff: `"Reading the diff is not verification"` · zero warnings: `"zero warnings"` · blind spots: `"Know what the gate"` \| `"blind to"` · loaded: `"actually load"` · look: `"looked at"` · never-done: `"Never mark a task"` | `### Rule A — Zero build errors and warnings` (qctrl, qbots, materia, containers, gpu); `### Rule A — Prove it runs and caches` (cache). The legacy text is mostly the **project gate** — map it to `rule-a-gate`, and propose only the missing preamble points as ADD. |
| `rule-a-gate` | seeded | #### Project gate | shape: contains at least one inline/fenced command and a pass condition | Legacy: the body of Rule A. Never reworded by resync. Empty/placeholder → flag. |
| `commit` | core + param | Rule B — Commit at every task boundary | before-done: `"COMMIT BEFORE MARKING ANY TASK COMPLETE"` · every task: `"every** task"` \| `"end of every task"` · one per commit: `"One task per commit"` \| `"only the changes for that task"` · row same commit: `"Tracker row in the same commit"` · explicit paths: `"git add -A"` · gate first: `"passes **before** every commit"` \| `"must pass **before** every commit"` · user doc: `"observable behavior"` · never push: `"Never push"` · co-author: `"co-author"` · baked in: `"bake"` | `### Rule B — Commit at every task boundary` (+ "OR MORE FREQUENTLY"). **param** = the message format line: `task(TN):` (cache/gpu/qbots/qctrl), `[PNN][TNN]` (containers), `[P<n>][T<n>][<topic>]` (materia). Report it; never change it silently. containers' "MANDATORY REVIEW … Neckbeard AND Hoodie" → CONFLICT (see below). |
| `append-only` | core | Rule B2 — Git history is append-only | rule: `"append-only"` · ban table: `"git commit --amend"` · working tree: `"git clean"` · fix forward: `"NEW commit"` \| `"new commit"` · unpushed: `"looks unpushed"` \| `"even when the commit"` · chaining: `"Chain edit-then-commit"` · factual: `"factual claim"` · incident: `"Why this rule exists"` | cache: `### Rule B2 — Git history is APPEND-ONLY`. gpu: Rule B item 6 (one line + pointer to `../../../CLAUDE.md`). qbots/qctrl/materia: absent from RULES (materia AGENTS.md has a git section). containers: **contradicted** (see CONFLICTS). |
| `lifecycle` | core | Rule C — Plan lifecycle | move: `"completed/"` · mkdir: `"mkdir -p"` · partial stays: `"partially complete"` · abandoned: `"abandoned/"` · reason: `"stated reason"` \| `"recorded reason"` · active check: `"Currently Active"` \| `"deferred/blocked"` · hypothesis: `"hypothesis, not a contract"` | `### Rule C — Move completed plans to \`completed/\`` (cache, qbots, containers, gpu); `## Completed Plans` section (all, often duplicated alongside Rule C → propose merging). materia: no lifecycle *rule letter* — its `Rule C` is `evidence`+parity. qbots/gpu have `abandoned/` dirs; qbots SERIES documents it. |
| `harvest` | core | Rule D — Harvest the knowledge | rule: `"Harvest"` · as you go: `"as you go"` · bytes: `"unless the bytes are"` · multi-attempt: `"more than one attempt"` | cache: `### Rule D — Harvest the knowledge before you close the plan`. gpu's `Rule D` is **measurement** (→ measurement pack), not harvest. Others: absent (root AGENTS.md "Knowledge Management" bullets carry the idea). |
| `evidence` | core | Rule E — Evidence over assertion | oracle: `"proves nothing"` · premise: `"premise is a claim"` · below bar: `"Below the bar"` · inconclusive: `"Inconclusive"` · cite: `"path:line"` \| `"file:line"` | materia: `### Rule C — Done means matched, and matched is measured` items 2–4 (the tier table is the **parity pack**). gpu: Rule D item 6 ("Inconclusive is a valid, recordable outcome"). |
| `project-rules` | local | Project Rules | section: `"Project Rules"` \| `"Project-Specific"` | containers: `## Project-Specific Conventions` (has a CONFLICTING git workflow inside). gpu `Rule D/E`, materia `Rule C` tiers → suggest *relocating* under Project Rules only if the user wants; never required (letters are cited by old plans). |
| `series-rules` | core | Series-Scoped Rules | rule: `"_RULES.md"` · red flag: `"copy from X"` | materia `REUSE_RULES.md` is a live instance (series-specific file); no RULES section anywhere. |
| `style` | core | Content Style | section: `"Content Style"` · dates: `"date +%F"` · languages: `"language"` | `## Content Style` (all) ✓. |
| `templates` | core | Templates & History | tracker template: `"NN_example_tracker.md"` · history: `"completed/"` | `## Canonical Template` (all). qctrl + materia say "Plans 60–67 are the most recent" — stale in qctrl (12 plans) → flag. |

## `context/plans/SERIES.md`

| id | kind | Canonical heading | Probes | Legacy aliases / notes |
|---|---|---|---|---|
| `series-header` | core | (H1 + intro) | stamp: `"hands-plan:v"` · update trigger: `"Update"` · hypothesis: `"hypothes"` | materia: "should be treated as a hypothesis, not a contract" ✓. |
| `next-free` | core | **Next free plan number** | `"Next free plan number"` | cache only. Others: missing → ADD with computed max+1 (hp-scan `NEXT`). |
| `north-star` | seeded | North star | `"North star"` | qbots ("user directive, 2026-07-09"), gpu ✓. cache/qctrl/containers/materia: missing (materia has an arc statement — accept it as the north star). |
| `active` | seeded | Currently Active | `"Currently active"` | materia ✓ (in blockquote); qbots "Active set"; others missing. |
| `plans` | local | Plans table | table with a Status column | All have one (`## Plan Status`, `## Plan Sequence`, `## Status`). Never rewritten. |
| `abandoned` | core | Abandoned / Superseded | `"Abandoned"` \| `"Superseded"` · reason col: `"Reason"` | gpu ✓ table; qbots in-row notes + footer; containers `## Retired`. |
| `constraints` | seeded | Standing Constraints | `"Standing constraint"` | containers ✓. Optional elsewhere — report missing only as info. |
| `compaction` | core | (footer) | `"SERIES_ARCHIVE"` | none. Flag if SERIES > 30 KB (materia 103 KB, qbots 42 KB). |

## `context/plans/NN_example.md` and `NN_example_tracker.md`

| id | kind | Probes | Legacy notes |
|---|---|---|---|
| `example-plan` | core | file: `file:context/plans/NN_example.md` · stamp: `"hands-plan:v"` · fill-every: `"EVERY section"` · scope: `"## Scope"` \| `"SCOPE CONSTRAINT"` · bug evidence: `"verbatim"` \| `"State the evidence"` · key facts: `"How confirmed"` · rejected: `"Rejected Claims"` · verify: `"**Verify**"` · expected: `"Expected observation"` · commit: `"**Commit**"` · checklist evidence: `"only against evidence"` · closers: `"completed/"` | Missing entirely in qbots/qctrl (qctrl AGENTS.md references it → dead ref). cache/gpu/containers/materia have one; `{{CODE_LANG}}` in the template becomes the project's language. |
| `example-tracker` | core | file: `file:context/plans/NN_example_tracker.md` · resume: `"Resume Instructions"` · row same commit: `"row in the same commit"` · deviations: `"Deviations"` · follow-ups: `"Follow-ups"` · evidence: `"## Evidence"` | No legacy project has a tracker template file (format lived in RULES only). → ADD. |

## `context/AGENTS.md` (+ `CLAUDE.md` symlink)

| id | kind | Probes | Legacy notes |
|---|---|---|---|
| `context-agents` | core | file: `file:context/AGENTS.md` \| `file:context/CLAUDE.md` · stamp: `"hands-plan:v"` · authority: `"authoritative"` · honesty: `"unless the bytes are"` | No legacy project has one; the content lives in root AGENTS.md "Knowledge Management"/"Context Is Mandatory". → ADD, routing rows built from the project's **actual** files (materia: `impl-bugs.md`, `materia-learnings.md`; qbots: `distilled/`, `brain_notes.md`). |
| `context-map`, `context-routing` | seeded | table rows exist | Never reworded; check every path they name exists. |
| `context-provenance` | core | `"never upgrade a tag"` | cache distilled ("Do not upgrade a tag without doing the work"), gpu ("Promote only after actually seeing it") — same idea. |
| `context-style` | core | `"40 KB"` · `"No full source"` | slop root CLAUDE.md `./context` rules. |

## Root `AGENTS.md` / `CLAUDE.md`

| id | kind | Probes | Notes |
|---|---|---|---|
| `root-block` | core | `"hands-plan:begin v"` and `"hands-plan:end"` | Version in the begin marker vs skill VERSION. Content compared with `templates/root/agents_block.md` snippet 2 — UPDATE shows the new block. Legacy: no markers; the equivalent prose is spread across "Development Workflow" sections → propose the block + de-dup list (snippet 3). |
| `root-tree` | heuristic | a fenced tree containing `context/` and `plans/` | Never marked (inside a code fence). Check it names files that exist. |
| `root-refs` | heuristic | every `context/…` path mentioned exists | qctrl: `NN_example.md` missing. |
| `root-restated` | heuristic | restated rules agree with RULES | qbots AGENTS.md §5a duplicates Rule C; commit-format lines that differ from RULES `commit` param; "two-digit" numbering claims. |
| `root-stale` | heuristic | "Status"/"Phase"/"Next step"/"Getting Started" sections agree with SERIES | qctrl: "Phase: Planning / Scaffolding" with 12 plans shipped. |

---

## Never touch (resync never proposes edits to these)

- Any real plan or tracker (`NN_*.md` other than the two `NN_example*` files), anything in
  `completed/` or `abandoned/`.
- SERIES rows, narrative, milestones, backlog — only the structural items above.
- The *content* of `distilled.md`, `pitfalls.md`, `high_level.md` and every other knowledge file.
- `local` and `seeded` section text (shape checks only), `#### Project addendum` blocks.
- Rule **letters** — never renumber; old plans and commit bodies cite them. A missing core rule is
  added under the next free letter with its `hp:` marker.

## Known CONFLICT patterns (quote them; the human decides)

| Pattern | Seen in | Conflicts with |
|---|---|---|
| "Rebase to clean up history", "squash", "minor tweaks can be squashed into the original commit" | containers RULES §Git Workflow, Rule B.6 | `append-only` |
| "REVERT to last known good commit" / "Emergency Recovery" | containers RULES | `append-only` (no `git revert`, no resets) |
| A reviewer persona that must sign off before a task is done ("Neckbeard AND Hoodie") | containers RULES Rule B.6–7 | `commit` (commit before marking done) — ask whether it is real |
| "NEVER start a new plan without finishing the previous one" | containers, qbots, cache (Rule C.5 legacy) | `lifecycle` item 4 (parallel plans allowed when listed as active) — usually just outdated |
| "two-digit zero-padded" numbering in a project with plans ≥ 100 | materia RULES + AGENTS.md | `naming` |
| A commit format in the root file that differs from RULES Rule B | qbots/gpu AGENTS vs RULES (match today), materia | `commit` param |
| "Plans 60–67 are the most recent and reflect current conventions" | qctrl, materia RULES | `templates` (stale copy-paste) |
