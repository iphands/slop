# Plan 18 — Ahead-of-Time Map Cache (`generate-map-cache`)

> **Status**: pending
> **Created**: 2026-06-16
> **Depends on**: Plan 17 (STEP fix must land first so cached graphs bake in the corrected
> constant), Plan 09 (fleet / `NavCache`)
> **Goal**: Move BSP-parse + collision-model + nav-graph-generate out of every bot's connect
> path into a `generate-map-cache` command that writes a versioned binary cache to
> `./data/mapcache/`, and teach the fleet + scenario runner to load it.
> **Agent**: implementation agent (ralph-loop)

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Today every bot — including every one of N bots in `spawn-to-spawn --count N` —
independently runs `Bsp::load` → `CollisionModel::from_bsp` → `NavGraph::generate` →
`seed_spawns` → `detect_jump_edges` from scratch (`scenario.rs:77-99`,
`supervisor.rs::build_map_nav`). `NavCache` (`supervisor.rs`) only caches *within* one fleet
process's lifetime, in memory, keyed by map name — there is no cross-process, cross-run, on-disk
cache. The existing `world::learn` module has a `save_graph`/`load_graph` stub, but it's
nodes-only (no edges) — built for an abandoned observation-learning experiment
(`context/plans/abandoned/16_observation_learning.md`), not usable as a real cache.

This plan adds a real one: a new `qbots generate-map-cache` subcommand that runs the full
generation pipeline once per map and writes a fingerprinted binary file to
`./data/mapcache/<map>.qnav`; `NavCache::get_or_build` and the scenario runner's setup both
check the cache first, falling back to live generation (never a hard failure) if it's missing
or stale.

**Deliverables**:
1. `world::generate_map_nav(baseq2, map) -> Result<(Bsp, CollisionModel, NavGraph), String>` —
   one function replacing the duplicated build sequence in `scenario.rs` and `supervisor.rs`.
2. A binary cache format + `world::mapcache::{save, load}`, fingerprinted against the BSP file
   bytes and the generation constants (grid spacing, `STEP`, jump-edge spacing) so any future
   constant change or map edit auto-invalidates stale caches.
3. `qbots generate-map-cache [--map <name-or-glob>] [--jobs N] [--out-dir ./data/mapcache]`,
   supporting a single map (`q2dm1`) or a glob (`q2dm*`), parallelized.
4. `NavCache`/scenario setup load from `./data/mapcache/` first, with a clear log line either way.
5. `.gitignore` entry for `/data/mapcache/`.

**Estimated effort**: Medium (1 day).

---

## Context

### Why hand-rolled binary, not serde/bincode

`world`'s `Cargo.toml` currently has zero dependencies beyond `q2proto`. The project's own
stated codec style (AGENTS.md: "hand-rolled little-endian readers/writers over `bytes::BytesMut`.
The Q2 message API is tiny") already gives us the pattern to follow — `q2proto::Reader`/`Writer`
are right there. Reuse that style for the cache format instead of pulling in `serde`+`bincode`
as a new dependency pair for a single internal file format. (If `q2proto::Reader`/`Writer` are
awkward to depend on from `world` due to crate layering, a small hand-rolled
`Vec<u8>`-append/`&[u8]`-cursor pair in a new `world/src/mapcache.rs` is fine — keep it tiny.)

### Why a fingerprint, not just a version byte

A cache built before Plan 17's `STEP` fix lands would silently keep stale (wrong) edges forever
if we only checked a format version. The fingerprint must include: the BSP file's byte length +
a cheap content hash (e.g. FNV/crc32 over the bytes — already have a CRC table in
`q2proto::crc`, consider reusing it), plus the grid spacing, `STEP`, and jump-edge spacing
constants baked in at generation time. Any mismatch on load = treat as a cache miss, regenerate,
overwrite.

### Parallelism strategy

- **Within one map**: `NavGraph::generate`'s grid sampling does one independent
  `floor_waypoint` collision trace per `(x, y)` grid column — embarrassingly parallel. Add
  `rayon` as a new dependency to `world` (justified — single well-known crate, not a project
  convention violation) and parallelize the column sampling with `par_iter`/`par_bridge`. Edge
  connectivity (8-neighbor hull traces) can similarly be parallelized per-node after all nodes
  exist, since edge checks between two fixed nodes don't mutate shared state until merged.
- **N maps at once**: independent maps share nothing — use a `rayon::scope` or a bounded
  `std::thread` pool sized by `--jobs` (default: `std::thread::available_parallelism()`), one
  map fully generated per worker.
- Record the chosen approach (and why the other wasn't picked) in `context/high_level.md` per
  the slop project's library-choice convention.

### Where the command lives

`qbots generate-map-cache`, not a `crates/tools/` binary — AGENTS.md reserves `tools/` for
one-off debug helpers (packet capture, bsp dumper); this is a first-class, user-facing
operational command that `qbots` itself depends on at runtime (the cache it writes is read back
by `qbots run`/`spawn-to-spawn`/`spawn-to-weapon`), so it belongs in the `qbots` binary's `Cmd`
enum alongside `ConnectOne`/`Run`/`SpawnToSpawn`.

---

## Step-by-Step Tasks

### T1: Consolidate the build sequence into `world::generate_map_nav`

**Files**: `crates/world/src/lib.rs` (or a new `crates/world/src/build.rs`),
`crates/qbots/src/scenario.rs`, `crates/qbots/src/supervisor.rs`

**What to do**: Extract the `Bsp::load` → `CollisionModel::from_bsp` → `NavGraph::generate` →
`seed_spawns` → `detect_jump_edges` → `spawns_in_largest_component` sequence (currently
duplicated, with slightly different grid-spacing literals, in both call sites — unify on one
constant) into a single `world` function returning everything callers need. Both `scenario.rs`
and `supervisor.rs` call this instead of repeating the sequence. This alone removes the
spacing-literal drift risk (24.0 in both today, but nothing enforced that).

**Commit**: `task(T1): consolidate map-nav build sequence into world::generate_map_nav`

---

### T2: Binary cache format + save/load

**File**: new `crates/world/src/mapcache.rs`

**What to do**: Define a fingerprinted binary format (magic `b"QBNAVC2"`, version `u32`, BSP
byte-length + hash, generation-constant snapshot, then node positions + edges with
`EdgeKind`/jump-landing data — a faithful serialize of everything `NavGraph` needs to skip
regeneration entirely, not just nodes). Add `pub fn save(path, graph, fingerprint)` and
`pub fn load(path, expected_fingerprint) -> Option<NavGraph>` (returns `None` — not an error —
on any mismatch/corruption, so callers can fall back to live generation transparently). Add
round-trip unit tests.

**Commit**: `task(T2): nav graph binary cache format + save/load`

---

### T3: `generate-map-cache` CLI command

**File**: `crates/qbots/src/main.rs` (new `Cmd::GenerateMapCache` variant + handler)

**What to do**: Add the subcommand with `--map <name-or-glob>` (default: error, require
explicit), `--jobs <N>` (default: `available_parallelism()`), `--out-dir` (default
`./data/mapcache`). Glob handling: if `--map` contains `*`, enumerate available maps by listing
loose `.bsp` files in `<baseq2>/maps/` and `Pak::names()` entries matching `maps/*.bsp` across
`pak0..9`, dedupe, glob-filter (a simple `*`-only glob is fine — no need for a regex crate).
For each matched map, generate (using T1's consolidated function) + write the cache (T2) on a
worker from the `--jobs`-sized pool; log per-map timing.

**Commit**: `task(T3): generate-map-cache CLI command with glob + --jobs`

---

### T4: Parallelize node generation within one map

**File**: `crates/world/Cargo.toml` (add `rayon`), `crates/world/src/navgraph.rs`

**What to do**: Convert the grid-column sampling loop in `NavGraph::generate` to use
`rayon::prelude::*` (`par_iter` over grid columns, collecting `Option<[f32;3]>` results, then
sequentially building the node list + spatial index so indices stay deterministic — order
matters for reproducible node IDs, so collect-then-assign, don't push concurrently into a
shared `Vec`). Benchmark before/after on q2dm1 generation time; record in the tracker.

**Commit**: `task(T4): parallelize nav graph grid sampling with rayon`

---

### T5: Wire cache lookup into `NavCache` and scenario setup

**Files**: `crates/qbots/src/supervisor.rs`, `crates/qbots/src/scenario.rs`

**What to do**: Before calling `generate_map_nav` (T1), check `./data/mapcache/<map>.qnav`
(configurable dir, default matches T3) via `mapcache::load` with the current fingerprint. Hit →
use it, log `nav graph: loaded from cache (<n> nodes, <m> edges)`. Miss/stale → generate live
(unchanged behavior today) and log a one-line hint:
`nav graph: no fresh cache for '<map>' — run 'qbots generate-map-cache --map <map>' to speed up future runs`.
Never error out on a missing cache — this must stay a pure optimization, not a new failure mode.

**Commit**: `task(T5): load nav graph from disk cache when fresh, else generate live`

---

### T6: `.gitignore`

**File**: `qbots/.gitignore`

**What to do**: Add `/data/mapcache/` (regenerable build artifact, per AGENTS.md Constraint #5).

**Commit**: `task(T6): gitignore /data/mapcache/`

---

### T7: Live verification

**What to do**:
1. `cargo run -p qbots -- generate-map-cache --map 'q2dm*' --jobs 4` — confirm one `.qnav` file
   per matched map under `./data/mapcache/`, and that parallel `--jobs` actually overlaps
   (check wall-clock vs `--jobs 1`).
2. Time `cargo run -p qbots -- spawn-to-spawn --count 8 --max-secs 60` (Plan 19 adds
   `--max-secs`; until then, run with the existing 30s default) once with the cache present and
   once with `./data/mapcache/` removed — record both wall-clock-to-first-bot-moving numbers in
   the tracker.

**Commit**: none (verification only — update tracker).

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/world/src/mapcache.rs` (new) | T2: cache format | P0 |
| `crates/world/src/navgraph.rs` | T4: parallel generation | P1 |
| `crates/world/Cargo.toml` | T4: add `rayon` | P1 |
| `crates/qbots/src/main.rs` | T3: CLI command | P0 |
| `crates/qbots/src/supervisor.rs` | T1, T5: consolidate + cache lookup | P0 |
| `crates/qbots/src/scenario.rs` | T1, T5: consolidate + cache lookup | P0 |
| `qbots/.gitignore` | T6 | P2 |

---

## Open Questions / Risks

1. **Fingerprint over the whole BSP file vs. just the lumps we use**: hashing the whole file is
   simplest and correct (any edit invalidates); only revisit if BSP files are large enough that
   hashing becomes a measurable cost (unlikely — q2dm1 is a few hundred KB).
2. **rayon's determinism**: collect indexed results then build the node list/edges sequentially,
   so two runs of `generate-map-cache` on the same BSP produce byte-identical caches (useful for
   diffing/debugging, and avoids spurious cache "changes" from one parallel run to the next).
3. **What if `--out-dir` doesn't exist yet?**: `generate-map-cache` should `create_dir_all` it;
   loaders should treat a missing directory the same as a missing file (cache miss, not error).

---

## Verification Checklist

- [ ] T1: `cargo build` clean; both call sites use the consolidated function; no behavior change
- [ ] T2: round-trip save/load test passes; fingerprint mismatch correctly returns `None`
- [ ] T3: `generate-map-cache --map 'q2dm*'` produces one file per matched map
- [ ] T4: q2dm1 generation time improves measurably with `--jobs > 1` vs `--jobs 1`
- [ ] T5: `spawn-to-spawn` logs "loaded from cache" on a second run after T3; logs the fallback
  hint on a clean checkout with no cache
- [ ] T6: `git status` shows `/data/mapcache/` untracked-and-ignored after a generate run
- [ ] T7: wall-clock improvement for `--count 8` with vs without cache recorded in tracker
- [ ] `cargo build` + `cargo clippy -- -D warnings` + `cargo test` + `cargo fmt` clean throughout
