# Plan NN — [Title]

> **Status**: pending
> **Created**: YYYY-MM-DD
> **Depends on**: Plan N | N/A
> **Goal**: One-sentence deliverable description.
> **Hardware**: A310 / DG2-G11 / `xe` — note here if the plan assumes otherwise
> (a patched Mesa, an `i915` boot, a different resolution).

---

> **Before starting, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: One sentence describing what is being done.

**Deliverables**:
1. Concrete output one
2. Concrete output two

**Estimated effort**: Small (2 h) | Small–Medium (half day) | Medium (1 day) | Large (3 days)

---

## Context

Why this plan exists. What prompted it, what problem it addresses, what the intended
outcome is.

### Prior Measurements

What earlier plans established, **with provenance**. Cite the capture and the tracker
row, not a recollection.

| Source | Metric | Result | Runs | Spread |
|--------|--------|--------|------|--------|
| Plan N tracker, `captures/palworld_2026-07-30_baseline.gfxr` | frame time p50 | 14.2 ms | 5 | ±0.3 ms |

### Why [Approach]

Justification for the design choice. What alternatives were considered and why they lost.

### Key Facts

Tool syntax, hardware limits, driver behavior confirmed while planning. Anything durable
here should also be appended to `context/distilled.md` — this section is the plan's
working copy, `distilled.md` is the permanent record.

---

## Step-by-Step Tasks

### T1: [Task title]

**File**: `path/to/file.rs`

**What to do**: Detailed instructions.

**Before**:
```rust
// old code
```

**After**:
```rust
// new code
```

**Expected observation**: *(measurement tasks only — write this BEFORE running anything)*
- **Confirms the hypothesis if**: …
- **Refutes it if**: …
- **Is within noise if**: the delta is under X, where X came from the replay-stability
  measurement in Plan N.

**Commit**: `task(T1): <description>` — Rule B, commit before marking done.

---

### T2: [Task title]

…

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `analyze/src/foo.rs` | Description of change | P0 |
| `scripts/record.sh` | Description of change | P1 |

---

## Open Questions / Risks

1. **[Risk name]** — description. *Mitigation:* …
2. **Is the effect above noise?** What is the measured run-to-run spread for this
   workload, and is the expected effect larger than it? If not, this plan cannot
   succeed as written — say so now, not after the work.
3. **Does it survive a reboot?** Thermal state, ASLR, shader cache warmth, and
   `dev.xe.observation_paranoid` all reset. Anything measured in one session only is provisional.
4. **Does it hold on the other kernel driver?** If the finding is `xe`-specific, say so;
   if it should port to `i915`, that is a claim requiring evidence.

---

## Verification Checklist

Each item is a testable assertion. "It looks faster" does not qualify.

- [ ] T1: `cargo test` passes; `cargo clippy -- -D warnings` clean
- [ ] T2: [specific, checkable observation with a threshold]
- [ ] T3: measurement recorded in the tracker's Measurements table with capture name,
      driver build, N runs, and spread
- [ ] Plan: system restored to a known state (Rule E.4) — no stray sysctls, env vars,
      or swapped ICDs left behind
