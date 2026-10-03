# Pack — measurement discipline

For projects whose product is a **number**: performance work, profiling, benchmarks, tuning,
model evaluation, capacity tests. Origin: the slop `gpu` project, where a wrong number was the
equivalent of a failing test that reports success.

`init` (or `resync`, as a suggestion) copies each block below into the place its heading names.
Renumber `P<n>` to the next free Project Rule number. After that the text belongs to the project.

---

## → RULES.md, under `## Project Rules`

```markdown
### P<n> — Every number carries provenance and variance

This project's numbers are its product. An unsourced number is worse than no number, because it
gets cited.

1. **N ≥ 3 runs.** Report mean and spread. One run is an anecdote.
2. **A delta inside the spread is "within noise"** — write those words. Never report it as a win,
   a regression, or a trend.
3. **Name the source**: input/capture, build (git SHA or version string), environment and
   settings. It goes in the tracker's Evidence table.
4. **Change one thing at a time.** A different build *and* different settings in one comparison
   measures nothing.
5. **Check the environment first** (throttling, background load, warm vs cold caches). A run under
   a bad state is re-measured, not caveated.
6. **Inconclusive is a valid, recordable outcome.** Record it. Do not round it up.

### P<n+1> — Capture hygiene

1. Captures, traces and raw result files go in `captures/` (or the project's equivalent) and are
   **gitignored** — they are large and regenerable.
2. Name them so they are identifiable six months later: `<workload>_<YYYY-MM-DD>_<what-varies>.<ext>`.
3. The tracker's Evidence table says which capture backs which number.
4. **Restore the system after a session** — sysctls, env vars, swapped drivers/binaries, debug
   flags. The next session's baseline depends on a known state.
```

## → NN_example.md, in `## Context`

```markdown
### Prior Measurements

What earlier plans established, **with provenance** — cite the tracker row and capture, not a
recollection.

| Source | Metric | Result | Runs | Spread |
|--------|--------|--------|------|--------|
| Plan N tracker, `captures/<file>` | <metric> | <value> | 5 | ±<x> |
```

## → NN_example.md, in `## Open Questions / Risks` (seed lines)

```markdown
1. **Is the effect above noise?** What is the measured run-to-run spread for this workload, and
   is the expected effect larger? If not, this plan cannot succeed as written — say so now.
2. **Does it survive a restart?** Thermal state, caches, sysctls all reset. A one-session result
   is provisional.
```

## → NN_example_tracker.md

The **Evidence** table is mandatory for every plan (not just "when numbers are produced"). Add to
the Resume Instructions: "Restore the system state after the session (P<n+1>.4)."

## → .gitignore (suggest)

```gitignore
/captures/
```
