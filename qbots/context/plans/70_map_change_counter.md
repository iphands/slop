# Plan 70 — Aggregated map-change count in the competition final report

> **Status**: in-progress
> **Created**: 2026-07-16
> **Depends on**: Plan 64 (servercount level-change detection), Plan 21/67 (competition report)
> **Goal**: Record and report how many map changes a competition run went through — one aggregated fleet-wide number (+ the map sequence), not per-bot.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: The competition FINAL report gains a `map changes` line — the count of level spawns the
run lived through, minus one — plus the chronological map sequence.

**Deliverables**:
1. `FleetStats` fleet-wide servercount→map log + `record_map_load`/`map_changes`/`map_sequence`.
2. `bot_task` records `(servercount, map)` at each map load (dedup by servercount collapses the
   36 bots into one count; reconnects to the same level don't inflate it).
3. A `map changes` summary line in the competition FINAL report and the fleet `log_final_stats`.

**Estimated effort**: Small (1–2 h)

## Context

Every bot in a competition sees the *same* rotation, and each detects a level change
independently (Plan 64: `servercount != map_servercount`, main.rs:1273). So the count must be
**deduplicated fleet-wide** — counting per-bot detections would multiply by the bot count.

The clean dedup key is the **servercount**: `SV_SpawnServer` bumps it on every level spawn (incl.
same-map restarts), so distinct servercounts = distinct levels the fleet played, and
`map_changes = distinct − 1`. A `BTreeMap<servercount, map>` keyed on the (monotonically
increasing) servercount also yields the map sequence in chronological order for free, and dedups
reconnects (re-recording the same servercount is idempotent).

`bot_task` already receives `&FleetStats` (used for kills/pickups), and the map-load block
(main.rs:1313) knows both the servercount and the resolved map name — so this needs **no new
parameter**, just a fleet-wide section on `FleetStats` (which stats.rs already frames as "fleet-wide
telemetry for the supervisor").

## Step-by-Step Tasks

### T1: `FleetStats` map log + recorders/queries

**File**: `crates/qbots/src/stats.rs`

Add `maps: Arc<Mutex<BTreeMap<i32, String>>>` (Default-derivable). Methods:
`record_map_load(servercount, map)` (insert if absent), `map_changes() -> usize`
(`len().saturating_sub(1)`), `map_sequence() -> Vec<String>` (values in key order). Unit tests:
same servercount from many "bots" counts once; three distinct levels → 2 changes; sequence is
chronological; empty → 0 changes.

### T2: record at map load + report the count

**Files**: `crates/qbots/src/main.rs`, `crates/qbots/src/supervisor.rs`

In `bot_task`'s map-load block (main.rs:1313, where `map` + `servercount` are both known), call
`stats.record_map_load(sc, &map)` when `servercount` is `Some`. In `run_competition`, after the
FINAL scoreboard, emit `tracing::info!(map_changes = …, maps = %seq.join(" → "), "…")`. Add the
same line to `log_final_stats` (the fleet path) for consistency.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/qbots/src/stats.rs` | fleet-wide map log + queries | P0 |
| `crates/qbots/src/main.rs` | record at map load | P0 |
| `crates/qbots/src/supervisor.rs` | FINAL + fleet report line | P0 |

## Open Questions / Risks

1. **Servercount monotonicity** — `SV_SpawnServer` increments it; a BTreeMap key gives
   chronological order. If a server ever reset it, the sequence order could skew, but the *count*
   (distinct keys − 1) stays correct.
2. **A revisited map in a rotation** (A→B→A) correctly counts as 2 changes — distinct servercounts,
   even though the map name repeats. Intended.
3. **Live verification needs a server + a rotation** — unit tests gate the dedup/count logic; a
   live rcon `map` flip is best-effort.

## Verification Checklist

- [ ] T1: dedup/count/sequence unit tests green. **Commit.**
- [ ] T2: `cargo build` + clippy clean; competition FINAL prints `map changes`; live rotation shows
  the right count (best-effort). **Commit.**
- [ ] `cargo fmt` + `cargo clippy -- -D warnings` + full `cargo test` before every commit.
- [ ] Docs (acceptance.md competition note), SERIES.md, move plan to `completed/`.
