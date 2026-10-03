# Pack — parity with an original (the oracle rule)

For projects that **port, reimplement, emulate, or must match** something that already exists: a
reimplementation of a binary, a protocol client that must be byte-exact, a port between languages,
a re-render that must match reference output. Origin: `materia-engine` (an FF7 reimplementation
measured against the original exe) and `qbots` (wire-format parity with a C server).

`init` (or `resync`, as a suggestion) copies each block below into the place its heading names,
after asking the user to **name the oracle and the tiers**. After that the text belongs to the
project.

---

## → RULES.md, under `## Project Rules`

```markdown
### P<n> — Done means matched, and matched is measured

A task that ports behavior of the original is done when it is **shown to match the original**,
not when it runs. This applies to outputs, timings, numbers and placement alike.

**The oracle** is {{ORACLE}}: the original itself, its own tables/specs, or its source read raw —
never our own output. A test that asserts our own values proves nothing about accuracy.

| Tier | Gate | Evidence |
|---|---|---|
| `stub` | produces something | its behavior read from the source, not compared with the original |
| `{{TIER_2}}` | {{TIER_2_GATE}} | {{TIER_2_EVIDENCE}} |
| `{{TIER_3}}` | {{TIER_3_GATE}} | {{TIER_3_EVIDENCE}} |
| `verified` | everything above, plus the final observable output compared | a recorded comparison, or a recorded human sign-off |

1. **Nothing is called "matched" — or "ported" without qualification — below `{{TIER_3}}`.**
   Each port's tier and its evidence are recorded (in the tracker, or a registry the project
   keeps).
2. **A plan premise is a claim.** When the original contradicts a plan, record the measurement,
   do what the original does, and mark the plan item `invalid` as written — in the tracker and
   the commit body.
3. **Below the bar is a row, not a silence.** Anything left below `{{TIER_3}}`, and any
   measurement that disagrees, gets a Follow-ups row in this plan or a named later one.
4. Cite the original as `path:line` / address / table name every time a value is ported.
```

## → NN_example.md, in `### Key Facts` (seed row)

```markdown
| Original's behavior for <X> | <value> | oracle: `<command / address / path:line>` on YYYY-MM-DD |
```

## → NN_example_tracker.md, Progress table

Add a **Tier** column after Status for port tasks: `stub` / … / `verified`.
