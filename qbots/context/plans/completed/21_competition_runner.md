# Plan 21 — Competition Runner

> **Status**: done
> **Created**: 2026-06-18
> **Depends on**: Plan 09 (fleet supervisor), Plan 20 (hybrid nav modes)
> **Goal**: A `qbots competition` subcommand that spawns N bots per nav `--mode` in one process (shared nav cache), one distinct skin per mode, and prints a per-mode frag scoreboard.
> **Agent**: sub-agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: One in-process runner that connects `modes × N` bots to a live server, each mode in a
distinct skin, and reports which mode frags best.

**Deliverables**:
1. `mode` threaded **per-bot** through the fleet supervisor (small refactor).
2. `skins::distinct` — one distinct skin per competitor.
3. `run_competition` over a single shared `NavCache` + a per-mode K/D scoreboard.
4. `qbots competition [--count N] [--modes ...]` subcommand.

**Estimated effort**: Small–Medium (half day)

## Context

We have 6 nav backends and a movement-only perf comparison (`context/mode_perf.md`). A live
competition is the natural next lens: all modes fighting at once, tellable apart by skin, scored
by frags. The fleet supervisor (`run_fleet`) already does count, qport spacing, staggered
connects, reconnect, shutdown, a shared `NavCache`, per-bot skins, and a kill/death tally
(`FleetStats`). The only structural gap is that **`mode` is fleet-wide** (`FleetShared.mode`);
the competition needs it **per-bot**.

**Why in-process (not Bash spawning 6 `run`s):** `run_fleet` builds its own `NavCache`
(`supervisor.rs:272`), so 6 processes rebuild the 12 890-node graph + navmesh 6× and hold 6
copies in RAM. One process shares a single `Arc<NavCache>`. It's also idiomatic (Rust helpers,
not scripts) and gives a unified scoreboard from `FleetStats::snapshot()`.

## Step-by-Step Tasks

### T1: `mode` per-bot
**File**: `crates/qbots/src/supervisor.rs`. Remove `mode` from `FleetShared`; add `mode:
crate::NavMode` to `bot_supervisor_loop`, pass to `bot_task` instead of `shared.mode`; `run_fleet`
passes its single `mode` to each spawn. Behaviour-preserving.

### T2: `skins::distinct`
**File**: `crates/qbots/src/skins.rs`. `distinct(baseq2, n, rng) -> Vec<Option<String>>` returns
`n` distinct `model/skin` from the available pool (shuffle+take; repeats only if pool < n, with a
warning). Reuse the pool enumeration already used by the random-skin path. Unit-test no-dupes.

### T3: `run_competition` + scoreboard
**File**: `crates/qbots/src/supervisor.rs`. New `run_competition(cfg, addr, modes, per_mode_count,
qport_base_override, skins_per_mode)`: one shared `NavCache`/`Shutdown`/`FleetStats`/signal; spawn
the mode×count cross-product (`name = "{tag}_{i+1}"`, `qport = base + mi*count + i`, per-mode
skin, per-bot mode); clamp by `max_bots`. Add `mode_tag(NavMode)` and
`log_competition_scoreboard(&FleetStats, &[NavMode])` (group `snapshot()` by name prefix → per-mode
kills/deaths/K-D table) used by the heartbeat + at shutdown. Unit-test the grouping.

### T4: `competition` subcommand
**File**: `crates/qbots/src/main.rs`. `Cmd::Competition { addr, count, modes, qport_base }`;
`--count` = bots per mode (default 8), `--modes` = comma list (default all 6). Handler resolves
addr, parses modes, builds `skins_per_mode`, warns if `modes·count` crowds `maxclients`, calls
`run_competition`.

### T5: Docs + close
Note the subcommand in `context/distilled.md`; move this plan + tracker to `completed/`; mark
`SERIES.md` done.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/qbots/src/supervisor.rs` | per-bot mode; `run_competition`; `mode_tag`; scoreboard | P0 |
| `crates/qbots/src/main.rs` | `Competition` subcommand + handler | P0 |
| `crates/qbots/src/skins.rs` | `distinct` helper | P0 |
| `context/distilled.md`, `SERIES.md` | docs | P1 |

## Open Questions / Risks
1. **Server capacity** — 6×N must respect `maxclients=64`; clamp by `max_bots` and warn. Default
   N=8 (48 bots).
2. **qport collisions** — contiguous per-mode blocks (`base + mi*count + i`) are disjoint; pin via
   `--qport-base` for reproducibility.
3. **Scoreboard fairness** — random spawns + skill variance make a single run noisy; the scoreboard
   is indicative, not definitive (note it in output).

## Verification Checklist
- [ ] T1: `cargo test` — existing `run` fleet behaviour unchanged.
- [ ] T2: unit test: `distinct` returns n entries, no dupes when pool ≥ n.
- [ ] T3: unit test: scoreboard grouping sums per mode by name prefix.
- [ ] T4: `qbots competition --count 4` connects 6×4 bots; `loading nav graph` logged **once**.
- [ ] Live: 24 bots in `qbots status`, 6 distinct skins, Ctrl-C prints per-mode scoreboard, clean disconnect.
- [ ] All: `cargo fmt` + `clippy -D warnings` + `cargo test` green before each commit.
