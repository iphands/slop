# Plan 01 — Workspace Scaffold

> **Status**: pending
> **Created**: 2026-06-14
> **Depends on**: N/A
> **Goal**: Stand up a compiling Cargo workspace with all member crates stubbed, a
> `.gitignore` that keeps build output out of git, a `justfile` with build gates, and
> green `fmt`/`clippy`/`test` — ready for Plan 02 to fill `q2proto`.
> **Agent**: implementation agent (ralph-loop) | sub-agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Create the qbots Rust workspace skeleton — six crates, gitignore, justfile,
toolchain pin — all compiling and linting clean with no business logic yet.

**Deliverables**:
1. Root `Cargo.toml` workspace manifest + six member crates (`q2proto`, `world`,
   `client`, `brain`, `qbots`, `tools`), each a minimal stub that compiles.
2. Project `.gitignore` covering `/target*/`, `/vendor/`, editor cruft (per AGENTS.md §Constraints #5).
3. `justfile` recipes: `fmt`, `fmt-check`, `clippy`, `test`, `build`, `all`.
4. `rust-toolchain.toml` pinning a stable channel + edition.

**Estimated effort**: Small (2 h)

---

## Context

### Why this first

Everything in Plans 02–07 depends on a workspace that compiles and lints clean. Plan 02
fills `q2proto`; Plan 03 wires `client` → `q2proto`. This plan creates the empty shell and
the verification gates so every later task starts from a known-green baseline (RULES.md
Rule A demands zero warnings — that only holds if the gate exists from day one).

### Key Facts

- **Crates** (from `AGENTS.md` §Architecture): `q2proto`, `world`, `client`, `brain`
  are libraries; `qbots` is the main binary; `tools` is a binary crate for reusable
  utilities (packet capture, BSP dumper). No cross-crate dependencies are wired yet —
  each is a standalone stub. Wiring (`client` → `q2proto`) happens in later plans.
- **Edition**: 2021 (stable, matches `../qctrl`). Edition 2024 is an option if we want
  stricter lints later — flagged as an open question.
- **`.gitignore`** must include `/target/`, `/target-*/` (qctrl cross-compiles to
  `target-host/`; we may too), `/vendor/` (cloned, not authored), `**/*.rs.bk`. Keep
  `Cargo.lock` **committed** (qbots ships binaries). See AGENTS.md §Constraints #5.
- **Justfile pattern**: mirror `../qctrl/justfile` (Rust-only, so no `fe-*` recipes).

---

## Step-by-Step Tasks

> **RULES.md Rule A/B apply to every task**: zero build warnings, zero clippy warnings,
> `cargo fmt` applied, tests green — **then commit** as `task(TN): <desc>` at each boundary.

### T1: Create the workspace manifest

**File**: `Cargo.toml`

**What to do**: Create the root workspace manifest. Members are the six crates under
`crates/`. No `[workspace.dependencies]` yet (added as deps are introduced). Set
`resolver = "2"`.

**After**:
```toml
[workspace]
resolver = "2"
members = [
    "crates/q2proto",
    "crates/world",
    "crates/client",
    "crates/brain",
    "crates/qbots",
    "crates/tools",
]

[workspace.package]
edition = "2021"
# version = "0.1.0"   # set per-crate
```

### T2: Generate the six member crates

**Files**: `crates/{q2proto,world,client,brain}/src/lib.rs`, `crates/{qbots,tools}/src/main.rs`
plus each `crates/<name>/Cargo.toml`.

**What to do**: For each library crate, a `lib.rs` with a module-level doc comment and a
sanity `#[test]`; for each binary crate, a `main.rs` printing nothing on success (or a
trivial `--help`). Each `Cargo.toml` sets `name`, `edition.workspace = true`. Use
`name = "qbots"` for the main bin and `name = "tools"` for the utility bin.

**Commit**: `task(T2): scaffold six workspace member crates`

### T3: Add the project `.gitignore`

**File**: `.gitignore`

**What to do**: Keep all regeneratable/build output out of git (AGENTS.md §Constraints #5).

**After**:
```gitignore
# Rust build output
/target/
/target-*/

# Cargo
**/*.rs.bk

# Vendored reference trees (cloned, not authored)
/vendor/

# Editor / OS cruft
/.idea/
/.vscode/
*.swp
.DS_Store
# NOTE: Cargo.lock stays COMMITTED — qbots ships binaries.
```

**Commit**: `task(T3): add .gitignore excluding target, vendor, build artifacts`

### T4: Add the `justfile` build gates

**File**: `justfile`

**What to do**: Define recipes mirroring `../qctrl/justfile` but Rust-only. `all` is the
CI gate that every task must pass (RULES.md Rule A).

**After**:
```make
default: all

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --all-targets --all-features -- -D warnings

test:
    cargo test --all-features

build:
    cargo build --all-targets

# CI / pre-commit gate: fmt-check + clippy + test + build, all must pass
all: fmt-check clippy test build
```

**Commit**: `task(T4): add justfile with fmt/clippy/test/build gates`

### T5: Pin the toolchain

**File**: `rust-toolchain.toml`

**What to do**: Pin a stable channel so all contributors/agents build identically. Do not
pin a specific patch version unless reproducibility demands it — channel `stable` is fine.

**After**:
```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

**Commit**: `task(T5): pin stable toolchain with rustfmt and clippy`

### T6: Verify the whole workspace is green

**What to do**: Run the gate end-to-end from a clean state. Confirm `git status` shows no
`target/` leakage.

```bash
just all           # fmt-check + clippy + test + build, all pass
git status --porcelain | grep -E 'target|vendor' && echo "LEAK" || echo "clean"
```

**Commit**: `task(T6): verify clean workspace gates green` (or fold into T5 if nothing changed).

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `Cargo.toml` | New workspace manifest | P0 |
| `crates/*/Cargo.toml`, `crates/*/src/{lib,main}.rs` | Six stub crates | P0 |
| `.gitignore` | Exclude build artifacts + vendor | P0 |
| `justfile` | Build gates | P0 |
| `rust-toolchain.toml` | Stable toolchain pin | P1 |

---

## Open Questions / Risks

1. **Edition 2021 vs 2024.** 2021 is safe and matches qctrl; 2024 brings stricter lints.
   *Mitigation*: ship 2021 now; revisit in Plan 02 if a 2024 lint would catch codec bugs.
2. **Cross-compile target dir (`target-host/`).** qctrl uses it for Podman image builds.
   If qbots needs no cross-compile, the `/target-*/` line is harmless dead weight.
   *Mitigation*: keep the glob — costs nothing, future-proofs.
3. **Single workspace vs per-crate `Cargo.lock`.** One lock at the root is correct for a
   workspace. *Mitigation*: confirmed standard; no action.

---

## Verification Checklist

- [ ] T1: `cargo metadata` resolves the workspace with 6 members.
- [ ] T2: `cargo build --workspace` compiles every crate with zero warnings.
- [ ] T3: `git status` shows no `target/` or `vendor/` tracked.
- [ ] T4: `just all` exits 0 (fmt-check + clippy + test + build).
- [ ] T5: `rust-toolchain.toml` honored (`rustc --version` stable).
- [ ] T6: Clean clone → `just all` passes with no setup beyond toolchain.
