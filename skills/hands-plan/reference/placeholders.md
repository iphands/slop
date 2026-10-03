# Reference — template placeholders

Used by `init` (fills every one) and `resync` (fills the ones in any text it proposes). Every
`{{…}}` must be gone from a project file; hp-scan reports leftovers as `PLACEHOLDER`.

Author-time placeholders in `NN_example*.md` — `NN`, `[Title]`, `YYYY-MM-DD`, `<…>`,
`path/to/file.ext` — **stay** in the templates; `new` fills them when it writes a real plan.

**Paths to a parent `context/`** are always **relative to the file being written**. Compute them,
never guess: `realpath --relative-to="<dir containing the target file>" "<parent>/context"` (e.g.
from `sub/context/AGENTS.md` it is `../../context`, from `sub/context/plans/RULES.md` it is
`../../../context`).

## Rule names — `{{R_*}}`

Templates never assume a rule letter. Every reference to a per-task rule is a placeholder:

| Placeholder | Section id | New project | Legacy project (resync) |
|---|---|---|---|
| `{{R_GATE}}` | `hp:gate` | `Rule A` | the project's letter for its verification gate (always `Rule A` so far) |
| `{{R_COMMIT}}` | `hp:commit` | `Rule B` | the project's commit rule (`Rule B` so far) |
| `{{R_APPEND}}` | `hp:append-only` | `Rule B2` | the project's letter, or — when missing — see *Assigning letters* |
| `{{R_LIFECYCLE}}` | `hp:lifecycle` | `Rule C` | e.g. qbots/cache `Rule C`; materia has none (its `Rule C` is evidence) |
| `{{R_HARVEST}}` | `hp:harvest` | `Rule D` | cache `Rule D`; gpu's `Rule D` is measurement, not harvest |
| `{{R_EVIDENCE}}` | `hp:evidence` | `Rule E` | materia `Rule C` |

**Assigning letters to missing core rules (resync).** Use the template's default letter if the
project doesn't use it; otherwise the next letter after the project's last rule, assigned in
template order (append-only, lifecycle, harvest, evidence). Then resolve every `{{R_*}}` in the
proposed text to the final mapping. Never re-letter an existing rule — old plans and commit bodies
cite it.

## Values

| Placeholder | Where | Value |
|---|---|---|
| `{{DATE}}` | stamps, SERIES north star, TOPIC_RULES | `date +%F` output |
| `{{PROJECT_NAME}}` | SERIES, context files | the name from the description, else the directory name |
| `{{PLAN_GATE_ROWS}}` | RULES `plan-gate` | 2–4 rows `\| <risky change type in this project> \| **Yes.** <why> \|`, one per line |
| `{{PROJECT_GATE}}` | RULES `gate-project` | the approved gate (see `guides/rule_a.md`): numbered steps, fenced commands, mechanical pass conditions, blind spots, prerequisites, the init baseline result |
| `{{COMMIT_FORMAT}}` | RULES commit rule, item 3 | the format in backticks, one sentence on its parts, the outside-plan form, the bookkeeping forms, and two example sub-bullets — wrapped at ~100 columns. Default: `` `[P<n>][T<n>][<topic>] <imperative summary>` `` — plan number (not zero-padded), task number, short area tag. Outside a plan: `[<topic>] <summary>`. Plan bookkeeping replaces the task tag: `[P<n>][plan] add <title>`, `[P<n>][revise] …`, `[P<n>][close] <title>: <outcome>`. Sub-project in a monorepo (recommended): prefix the scope — `[<sub>][P<n>][T<n>][<topic>]` — so plan numbers don't collide between sub-projects. |
| | | **Legacy project (resync):** keep the project's own format text verbatim; forms it doesn't document (outside-plan, bookkeeping) become an INFO question with the hp-scan `COMMITFMT` tally — never invented. A format documented in the root agent file counts as the project's. |
| `{{COMMIT_FORMAT_SHORT}}` | root block, tracker template | the bare format, e.g. `[P<n>][T<n>][<topic>] <summary>` |
| `{{COMMIT_EXAMPLE}}` | NN_example T1 | the format instantiated with `<n>` and `T1`, e.g. `` `[P<n>][T1][<topic>] <short description>` `` |
| `{{USER_DOC}}` | RULES commit rule, item 6 | `` `README.md` `` (or the real user-facing doc) |
| `{{PARENT_GIT_RULE}}` | RULES, after the append-only incident | sub-project whose parent agent file has a git-discipline rule → `> Full rule: [\`<rel>/CLAUDE.md\`](<rel>/CLAUDE.md) § Git discipline.` (`<rel>` relative to `context/plans/`); otherwise delete the line |
| `{{PROJECT_RULES}}` | RULES `project-rules` | pack rule blocks, or `_None yet — add **P1** when a project-specific rule earns its place._` |
| `{{CODE_LANG}}` | RULES style, NN_example | the main language fence: `rust`, `python`, `ts`, `go`, `bash`, `nginx`, … |
| `{{NEXT_FREE}}` | SERIES | `01`, or hp-scan `NEXT computed=` in adopt mode |
| `{{NORTH_STAR}}` | SERIES | one or two sentences from the description — the outcome, not the tasks |
| `{{ORDERING_PRINCIPLE}}` | SERIES | one line if the description implies one ("measure before optimizing"), else `TBD — set when the second plan is written.` |
| `{{ACTIVE}}` | SERIES | `None yet — start with \`/hands-plan new <first piece of work>\`.` (adopt mode: the in-progress plans) |
| `{{PLAN_ROWS}}` | SERIES | adopt mode: one row per existing plan; otherwise **delete the line** |
| `{{CONSTRAINTS}}` | SERIES | standing constraints from the description/survey (missing tools, "this workstation is not the server"), else `None recorded yet.` |
| `{{VERIFY_CMD}}` | NN_example T1 | the gate's single-line commands chained with `&&`; a multi-line behavioral step becomes a comment pointing to it (`# + project gate step 5 (behavior)`) — the task-specific check sits above it |
| `{{CONTEXT_MAP_ROWS}}` | context/AGENTS.md map | one row per knowledge file that exists after init (real names, paths relative to `context/`), plus a row for the parent context (path computed as above) when one exists |
| `{{ROUTING_ROWS}}` | context/AGENTS.md routing | rows pointing at the **actual** files: confirmed fact → `distilled.md` (or its equivalent), bug/gotcha → `pitfalls.md`, dependency choice → `high_level.md` (or the parent's, if that is where the project already records them), project-specific logs found in the survey; for a new project also `<lib_name>.md` / `algo.md` / `patterns.md` (create on first use) |
| `{{PARENT_CONTEXT}}` | context/AGENTS.md routing | sub-project with a parent `context/` → `\| Anything that generalizes beyond this project \| also \`<rel>/pitfalls.md\` (or a \`<lib>.md\` / \`high_level.md\` there) \| Same shape; keep project-specific detail here \|`; else delete the line |
| `{{PROVENANCE}}` | context/AGENTS.md | the default tag set (`[SOURCE]` `[DOC]` `[REPO]` `[LIVE YYYY-MM-DD]`), or the project's own evidence vocabulary if it has one (e.g. rig / exe table / decomp / capture) |
| `{{VENDOR_LINE}}` | context/AGENTS.md style | `vendor/` exists → `- \`vendor/\` holds third-party source clones: **read-only** reference; never write docs into it — distill into \`context/\`.`; else delete the line |
| `{{PARENT_PITFALLS}}` | pitfalls.md | sub-project with a parent context → `Cross-cutting entries also go up to \`<rel>/pitfalls.md\`; project-specific ones stay here.`; else delete the line |
| `{{ORACLE}}` | parity pack | the original, named concretely (e.g. "GNU coreutils `wc` 9.x on this host", "the retail `ff7.exe` under Wine") — asked at init |
| `{{TIER_2}}`, `{{TIER_2_GATE}}`, `{{TIER_2_EVIDENCE}}`, `{{TIER_3}}`, `{{TIER_3_GATE}}`, `{{TIER_3_EVIDENCE}}` | parity pack | defaults: `behaves` / "the same results as the oracle on the same inputs" / "a recorded run of both on a shared input set"; `exact` / "value-, byte- or frame-exact on the reference set" / "a diff against the oracle's output that comes back empty". Offer them; the user may rename. |

## Symlink direction (`context/CLAUDE.md` ↔ `context/AGENTS.md`, and a new root pair)

Mirror the project's own root files: a pair (`CLAUDE.md -> AGENTS.md` or the reverse) → the same
direction; a lone real root file → that name is the real file in `context/` too, and the other
links to it; no root agent file → the nearest ancestor pair's direction (hp-scan `PARENTFILE`);
nothing anywhere → real `AGENTS.md`, `CLAUDE.md -> AGENTS.md`.
