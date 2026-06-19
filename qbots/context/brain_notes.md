# Brain development notes (running log)
# Started: 2026-06-18
#
# Append a dated section on EVERY brain plan (23–33) and any ad-hoc brain change.
# Format mirrors context/map_errors.notes.log.md: observed behavior, hypotheses,
# what was tried, outcome. Newest at the bottom. Keep entries dense, no fluff.

## 2026-06-18 — Plan 23: brain plugin core (trait Brain)
- Goal: introduce `trait Brain` + `BrainKind` factory; existing brain implements it; zero behavior change.
- Seam shape: `brain::brains::core` holds `trait Brain` + bundled I/O (`BrainContext<'a>`,
  `BrainOutput`, `BrainConfig`, `BrainMap`); `brains::mod` holds `BrainKind` enum +
  `build_brain(kind, skill, cfg) -> Box<dyn Brain + Send>` (mirrors `NavMode`/`build_navigator`).
- Seam shipped: `trait Brain` (set_map/tick/on_kill/on_death/heatmap_weights/status) in
  `brains::core`; `BrainKind::Main` + `build_brain` factory in `brains::mod`. Root `brain::Brain`
  export flipped from the concrete struct to the **trait**; the binary owns `Box<dyn Brain + Send>`.
- `tick` body is **byte-identical** to Plan 22 — the only change is the signature: it now
  destructures `BrainContext { view, nav, cm, dt, ticks }` / `set_map` destructures `BrainMap`.
  Pure adapter; no decision logic touched.
- Cosmetic change: periodic log used `brain.behavior()` (`Debug` of `BehaviorState`) → now
  `brain.status()` (`&str` label: roam/hunt/engage/flee/pickup). `behavior()` kept `#[cfg(test)]`
  for the typed-state unit test. Core stays decoupled from `BehaviorState` (main-specific).
- Surprise / process note: a trait extraction that changes signatures can't keep the binary
  green between "impl trait" and "update caller", so T3 folded `bot_task`'s 3 call sites into the
  same commit (inseparable). Used `use brain::brains::core::Brain as _` transiently in T3, then
  T5 made the root `Brain` the trait and switched construction to `build_brain`.
- Verification: `cargo build`/`clippy -D warnings`/`cargo test` (all 18 test binaries) green;
  `BrainConfig::default` combat-on/no-override + `build_brain(Main).status()=="roam"` asserted.
  **Live `connect-one`/`spawn-to-*` NOT run this session — server `noir40.lan` unreachable.**
  Behaviour-preserving by construction (adapter only); flag a live A/B once a server is up.

## 2026-06-18 — Plan 24: `main` brain plugin (relocate + prove the seam)
- `brain.rs` → `brains/main.rs`, struct `Brain` → `MainBrain` (git tracked as a rename; ~25 of
  ~454 lines touched — all the rename + doc, decision body verbatim). `pub mod brain` dropped;
  root exports now come from `brains::{core,main,sentry}`.
- Added `brains/sentry::SentryBrain` (~50 lines): stationary, combat-only, ignores nav — the
  proof that `trait Brain` is a real seam (a second impl sharing no state with MainBrain).
  `BrainKind::{Main,Sentry}`; `build_brain` dispatches both. `Sentry` is code/test-only until
  Plan 25 adds the `--brain` flag.
- `main` behaviour unchanged by construction (move+rename only). Verification: brain crate 106
  unit tests + sentry's 2 (constructs/labels; no-enemy tick → zero movement) green; workspace
  `build`/`clippy -D warnings`/`fmt` clean. **Live `connect-one`/`spawn-to-*` NOT run — server
  `noir40.lan` unreachable this session** (same as Plan 23). `scenario.rs` deliberately untouched
  (Plan 26 lifts it into `RuntesterBrain`; Plan 22 T4 stays open until then).

## 2026-06-18 — Plan 25: multibrain selection + --navmode rename
- `BrainKind` is now a `clap::ValueEnum` (added clap derive-only dep to the `brain` lib) +
  `brain_tag`. `--brain <main|sentry>` exposed on `connect-one` and `run` (fleet), threaded
  `bot_task`/`bot_supervisor_loop`/`run_single`/`run_fleet` → `build_brain`. Brain and navmode
  are independent: `build_navigator(navmode,…)` and `build_brain(brain,…)` are separate calls,
  no combination special-cased.
- Per-bot config: `[fleet].brain` (Option<String>, serde-default → main) with `Fleet::brain_kind()`
  parsing + warn-fallback; CLI `--brain` overrides config (like `--count`).
- `competition --brains main,sentry` spawns the `{navmodes}×{brains}` cross product; bots named
  `<group>_<i>` where group = `<mode>` (default single-main → board identical to before) or
  `<brain>-<mode>` when brains vary. Scoreboard regrouped by group tag (was mode-only). qport
  blocks are per-group disjoint; max_bots clamp now over `groups = navmodes×brains`.
- Rename: user-facing flag `--mode`→`--navmode`, `--modes`→`--navmodes` (clap `long=` override;
  internal field names `mode`/`modes` and the `NavMode` type/`build_navigator`/`mode_tag` kept).
  Updated CLI help, README, mode_perf.md. Clap gotcha: the value placeholder still renders as
  `<MODE>` (derived from the field name) — cosmetic; flag name is correct.
- DEVIATION: spawn-to-spawn/spawn-to-weapon did NOT get `--brain` (they got `--navmode`).
  `scenario.rs` uses raw nav/steer primitives, not a `Brain`, so a `--brain` there would be a
  no-op until Plan 26 migrates the scenario to `RuntesterBrain` — Plan 26 adds the functional
  spawn-to-* `--brain` (and flips its default to `runtester`). Avoided shipping a dead flag.
- Verification: 18 test binaries green; `--help` shows `--navmode`/`--navmodes`/`--brain`/`--brains`
  and no `--mode`; invalid brain/navmode rejected. Live matrix run pending a server.
