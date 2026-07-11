# Plan 56 — `--help` enumerates all brains and navmodes (competition value_enum lists)

> **Status**: done
> **Created**: 2026-07-11
> **Depends on**: N/A
> **Goal**: `qbots competition --help` lists every valid brain, navmode, and char (like
> `run` already does), by making `--brains`/`--navmodes`/`--chars` clap `value_enum` `Vec`s.
> **Agent**: sub-agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Convert competition's `--navmodes`/`--brains`/`--chars` from `Option<String>`
(hand-parsed comma lists) to clap `value_enum` `Vec`s with `value_delimiter = ','`, so clap
auto-renders `[possible values: …]` in `--help` and validates tokens.

**Deliverables**:
1. The three plural args are typed enums; clap lists all values in help.
2. Manual comma-split parsing + "unknown X" errors deleted; defaults + `runtester`
   rejection preserved.

**Estimated effort**: Small (1–2 h)

## Context

`run`'s `--brain`/`--navmode`/`--char` are `value_enum`, so `run --help` already shows a
`Possible values:` block. Competition's `--navmodes`/`--brains`/`--chars` are `Option<String>`
parsed by hand (`main.rs:2009-2082`), so `competition --help` shows only prose examples, not
the full valid set. clap can do both the enumeration and the parsing if the args are typed.

### Key Facts
- `--brains main,q3` must still work → use `value_enum` + `value_delimiter = ','` (also
  allows repeated flags). Omitted → empty `Vec` → handler applies the default.
- `runtester` is a valid `BrainKind` variant, so clap will list it as a possible brain even
  though competition rejects it; the command-level doc already notes "`runtester` … is
  rejected", and the runtime rejection stays.

## Step-by-Step Tasks

### T1: type the plural args + simplify the handler
**File**: `crates/qbots/src/main.rs`
- In `Cmd::Competition`, change:
  - `modes: Option<String>` → `#[arg(long = "navmodes", value_enum, value_delimiter = ',')] modes: Vec<NavMode>`
  - `brains: Option<String>` → `#[arg(long = "brains", value_enum, value_delimiter = ',')] brains: Vec<brain::BrainKind>`
  - `chars: Option<String>` → `#[arg(long = "chars", value_enum, value_delimiter = ',')] chars: Vec<brain::CharPreset>`
  - Trim the help prose (drop the "valid: …" hand-lists — clap now renders them).
- In the handler, replace the three parse loops with:
  - `let modes = if modes.is_empty() { NavMode::value_variants().to_vec() } else { modes };`
  - `let brains = if brains.is_empty() { vec![Main] } else { if brains.contains(&RunTester) { error+FAILURE } brains };`
  - `chars` used as-is (empty = no Q3 personalities).
- Remove the now-unused `ValueEnum::from_str` calls (keep the `ValueEnum` import for
  `value_variants`).

### T2: verify + close
`competition --help` shows `Possible values:` for all three; `--brains main,q3`,
`--navmodes astar,navmesh`, `--chars grunt,major` still parse; a bad token errors via clap;
`--brains runtester` still rejected. Workspace fmt/clippy/test green. Move plan+tracker to
`completed/`, mark SERIES.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/qbots/src/main.rs` | typed competition args + simplified handler | P0 |

## Open Questions / Risks
1. **`runtester` listed as a possible brain** though competition rejects it. Mitigation:
   command doc already says so; runtime rejection unchanged. Acceptable.
2. **Empty-list semantics.** Omitted flag → empty `Vec` → default applied; parity with old
   `None` branch. The old "was empty" error path disappears (clap requires a value when the
   flag is present) — acceptable/clearer.

## Verification Checklist
- [x] T1: `cargo build`/`clippy` clean; `competition --help` renders a `Possible values:`
      block for `--navmodes` (6), `--brains` (5), `--chars` (4).
- [x] T2: `--brains main,q3` / `--navmodes astar,navmesh` / `--chars grunt,major` parse;
      `--brains nope` → clap error listing `[possible values: main, sentry, runtester, q3,
      zb2]`; `--brains runtester` → runtime rejection; workspace fmt/clippy/test green.
