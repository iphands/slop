<!-- hands-plan:v1 YYYY-MM-DD -->
# [Topic] Rules — Plans NN–MM

> **MANDATORY for every agent implementing Plans NN–MM.** Read this before touching
> `[shared area]`. Linked from each of those plans' headers and from `SERIES.md`.
> Retire it (move to `completed/`) when the last plan in the range closes.

<!--
  Series-scoped rules: ONE decision rule that several plans share, written once instead of
  drifting through each plan. Typical uses: code reuse / extraction strategy across components,
  a format two sides must agree on, an ownership boundary. Delete these comments when filled.
-->

## The Problem

What keeps going wrong (or will) without a shared rule — e.g. plans saying "copy from X" with
no word on whether the code is extracted, kept, or used only as a reference.

## The Rule

Before writing any code that touches `[shared area]`, classify each piece:

1. **[Category one — e.g. pure logic, no runtime/device dependencies]** → **Extract** to
   `[shared home]`.
2. **[Category two — e.g. generic infrastructure helper]** → **Extract** to `[utils home]`.
3. **[Category three — e.g. application-specific orchestration]** → **Keep** where it is.
4. **[Category four — e.g. assets/shaders/schemas]** → **One shared source**, never duplicated.

If still unsure: is it used in more than one place, and does it have no app-specific
dependencies? Both yes → extract. Otherwise **ask** before implementing.

## Decision Map

| Component | Where it goes | Why |
|-----------|---------------|-----|
| `[thing]` | `[shared home]` / **KEEP** in `[place]` | [reason] |

## Per-Plan Instructions

### Plan NN — T[n]: [task]

**Plan currently says**: "[quote]"
**Correct approach**: [extract / use existing `[home]::[symbol]` / keep], and why.

## Red Flags (fix the plan before coding)

1. "Copy from X" / "port from X" without an extraction strategy.
2. The same helper, schema or asset appearing in two places.
3. Reimplementing something `[shared home]` already provides.

## Checklist (before marking any task in Plans NN–MM done)

- [ ] I classified every piece of reused code (extract / use existing / keep).
- [ ] I extracted shared logic instead of copying it.
- [ ] I used the existing shared component instead of reimplementing it.
- [ ] I updated the plan to state the strategy explicitly.
