# Plan 55 — Capacity preflight: refuse to spawn more bots than the server can hold

> **Status**: in-progress
> **Created**: 2026-07-11
> **Depends on**: Plan 53 (reuses `loose_botcap`; this is the *early* gate to 53's *join-time* gate)
> **Goal**: Before spawning any bot, query the server's `maxclients` + current player count;
> if the roster won't fit, exit immediately with a clear error (unless `--loose-botcap`).
> **Agent**: sub-agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Add a capacity preflight to `run_competition` and `run_fleet` — after the total
bot count is known but before the spawn loop — that fails fast when `total > free slots`.

**Deliverables**:
1. `preflight_capacity(addr, total, loose_botcap)` in `supervisor.rs` + a pure,
   unit-tested `fits_capacity` helper.
2. Called in both fleet paths before spawning; strict = exit non-zero, loose = warn.

**Estimated effort**: Small (2 h)

## Context

Plan 53 made an *over-subscribed* run fail loudly, but only **after** it spawns bots and
the first one is refused mid-handshake. We already query the server up front:
`preflight_map` (`main.rs:686`) calls `query_status` (`main.rs:657`) in the competition/run
dispatch, and `StatusReport` carries `maxclients: Option<u32>` and `player_count()`
(`status.rs:19,29`). So we can know the free-slot count *before* spawning and exit
immediately when the roster can't fit — cheaper and clearer than 53's join-time abort.

If capacity is unknowable (status query fails, or the server reports no `maxclients`), we
do **not** block — "exit only if we *know* it won't fit" — and rely on Plan 53 as backstop.

## Step-by-Step Tasks

### T1: capacity preflight helper + wire into both fleet paths
**Files**: `crates/qbots/src/main.rs`, `crates/qbots/src/supervisor.rs`
- Make `query_status` `pub(crate)` in `main.rs` so the supervisor can call it.
- Add a pure `fn fits_capacity(total: usize, maxclients: u32, players: usize) -> bool`
  (`total <= maxclients.saturating_sub(players)`) + unit tests.
- Add `async fn preflight_capacity(addr, total, loose_botcap) -> std::io::Result<()>`:
  query status; on query failure or missing `maxclients`, `warn!` + `Ok(())`; if it fits,
  `info!` the numbers + `Ok(())`; if not, `Err(io::Error::other(...))` (strict) or `warn!`
  + `Ok(())` (loose). The error names want/free/in-use counts and the three fixes.
- Call it in `run_competition` right after `let total = num_groups * per_group_count;`
  (before building `shared`/spawning) and in `run_fleet` after the `max_bots` clamp +
  `count == 0` guard (before `NavCache::new()`), both `?`-propagating so the dispatch's
  existing `Err → ExitCode::FAILURE` fires.

### T2: verify + close
Live: run a roster that exceeds free slots → immediate exit non-zero with the capacity
error, **no bots spawned**; then `--loose-botcap` → warn + proceed. Under-capacity run
unchanged. Workspace fmt/clippy/test green. Move plan+tracker to `completed/`, mark SERIES.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/qbots/src/supervisor.rs` | `preflight_capacity` + `fits_capacity` + 2 call sites | P0 |
| `crates/qbots/src/main.rs` | `query_status` → `pub(crate)` | P0 |

## Open Questions / Risks
1. **Flaky status query blocking a valid run.** Mitigation: query failure → warn + proceed,
   never a hard block (Plan 53 still catches real over-subscription at join time).
2. **Double status query in competition** (preflight_map + preflight_capacity). Negligible
   (~tens of ms, 2 s timeout); keeps total-count logic in one place. Acceptable.
3. **Race**: players can join between preflight and spawn. Preflight is best-effort; Plan 53
   remains the authoritative backstop.

## Verification Checklist
- [ ] T1: `fits_capacity` unit tests pass; `cargo build`/`clippy` clean.
- [ ] T2: over-capacity strict run exits non-zero **before** spawning (no `competitor
      entering` lines); `--loose-botcap` warns + proceeds; under-capacity run unchanged;
      workspace fmt/clippy/test green.
