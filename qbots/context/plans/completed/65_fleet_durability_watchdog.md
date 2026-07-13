# Plan 65 — Fleet Durability: Active-State Frame-Stall Watchdog

> **Status**: done
> **Created**: 2026-07-13
> **Depends on**: Plan 64
> **Goal**: Bots survive many hours of map rotations — every bot that loses its slot detects it and reconnects, so the fleet never shrinks.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Add a frame-stall watchdog that runs while a bot is `Active`, so a bot whose server slot silently died (missed `svc_reconnect` during a hard map change) returns a retryable error instead of hanging forever; plus reset the supervisor's reconnect budget after each successful session.

**Deliverables**:
1. `stall_timeout_ms` fleet config knob + watchdog in `bot_task` (default 10 s of no new server frames while `Active` → `ConnectionReset`).
2. `bot_supervisor_loop` resets `attempts`/`backoff_ms` after a completed session.
3. Verified live: a `competition` fleet on a 2-minute-timelimit server keeps its full roster across ≥4 consecutive map cycles.

**Estimated effort**: Small (2 h)

## Context

### Pre-Identified Bug/Issue

Observed by the user: `competition --brains main,q3,xon --count 2 --navmodes nm,sg …` on a
10-minute-timelimit server dropped from full roster to **16 bots after ~1 hour** (several
map rotations). Bots must stay connected for many hours across many map changes.

Code audit findings:

1. **No watchdog while `Active`** — `crates/qbots/src/main.rs:1062` explicitly gates the
   only deadline to `state != Active`. Plan 64 made the *soft* map change (reliable
   `stufftext "changing"/"reconnect"`) and the observed *hard* change (`svc_reconnect`)
   re-arm the deadline, but both depend on the bot **receiving** the state-change message.
   On a hard change `SV_FinalMessage` sends `svc_reconnect` copies **unreliably**; a bot
   that misses every copy stays `Active`, sends `clc_move` into a recycled slot the server
   now ignores (no netchan match ⇒ zero bytes back), and `bot_task` never returns. The
   supervisor's retry loop never runs. Each rotation is an independent small loss chance
   → exactly the observed gradual attrition. (Plan 64's pitfalls notes already document
   the burst-loss conditions around rotations: reliable-buffer overflow kicks, stale
   FinalMessage copies.)
2. **Reconnect budget never resets** — `bot_supervisor_loop`
   (`crates/qbots/src/supervisor.rs:842-923`): `attempts` and `backoff_ms` are
   incremented per retry and never reset after a successful session. With
   `max_reconnects > 0`, hours of rotations exhaust the budget and bots are dropped
   permanently ("giving up after max reconnects"); backoff also stays at the 15 s cap
   forever. Inactive with the current `max_reconnects: 0` config but a real durability
   bug.

Fleet-wide fatal paths (`fatal!` on missing BSP, strict initial-join failure firing
shutdown) kill **all** bots at once and cannot explain a partial 16-survivor state; out
of scope here.

### Why a frame-stall watchdog (and why 10 s)

An `Active` Q2 client receives `svc_frame` at 10 Hz continuously — including during
intermission. Ten full seconds without a single new serverframe while we believe we are
in-game means the slot is dead (or the server is gone), and the correct move is a full
re-handshake, which the supervisor already provides. 10 s is ~100 missed frames — far
beyond any load hiccup — and matches `connect_timeout_ms`'s order of magnitude. The
watchdog returns `ConnectionReset` (retryable, same classification Plan 64 uses for
post-session failures), so the existing backoff/rejoin machinery (fresh socket, fresh
handshake, anti-herd jitter) does the recovery.

## Step-by-Step Tasks

### T1: Active-state frame-stall watchdog

**Files**: `crates/qbots/src/config.rs`, `crates/qbots/src/main.rs`

**What to do**:
1. Add `stall_timeout_ms: u64` to `Fleet` (default `10_000`), doc-commented, plus the
   `config.example.yaml` entry.
2. In `bot_task`: track `last_frame_seen: time::Instant`.
   - Recv arm: inside the `state == Active` new-serverframe branch
     (`if Some(sf) != prev_sf`), refresh `last_frame_seen`.
   - Ticker arm, next to the Plan 53 deadline check: while `state != Active`, keep
     resetting `last_frame_seen` (the connect deadline owns non-Active hangs); while
     `Active`, if `now >= last_frame_seen + stall_timeout` → log an error and return
     `std::io::Error::new(ConnectionReset, "server frames stalled while Active")`.
3. `cargo build` / `clippy` / `test` / `fmt` clean. **Commit** `task(T1): …`.

### T2: Reset reconnect budget after a successful session

**File**: `crates/qbots/src/supervisor.rs`

**What to do**: In `bot_supervisor_loop`, on `Ok(())` (and on any error after
`had_session`) reset `attempts = 0; backoff_ms = 1000;` — the budget guards *consecutive*
failed attempts, not lifetime reconnects. Simplest form: reset both in the `Ok(())` arm.
Build gates + **commit** `task(T2): …`.

### T4: Stale `roam_idx` panics the brain on map change (found live during T3)

**Files**: `crates/brain/src/brains/q3/mod.rs`, `crates/brain/src/brains/main.rs`,
`crates/brain/src/brains/xon/mod.rs`

**What to do**: First T3 run caught it in the act — rotation q2dm1→q2dm3:
`panicked at crates/brain/src/brains/q3/mod.rs:220:39: index out of bounds: the len is
6279 but the index is 8501`. `set_map` replaces `roam_nodes` with the new map's (smaller)
list but keeps the old `roam_idx`; `roam_goal` only re-modulos the index every 50th tick,
so any other tick indexes straight out of bounds. `q3`, `main`, and `xon` all share the
pattern (`zb2` uses `.get()` and is safe). Fix: reset `self.roam_idx = 0` in each
`set_map`; add a regression test (set_map big roster → advance → set_map small roster →
`roam_goal` must not panic). Build gates + **commit** `task(T4): …`.

### T5: A brain panic must not permanently kill the bot

**File**: `crates/qbots/src/supervisor.rs`

**What to do**: `bot_supervisor_loop` awaits `bot_task` directly, so a panic anywhere in
the brain unwinds the supervisor loop itself — the bot is gone for good with no retry (the
exact attrition shape observed). Spawn `bot_task` as its own tokio task and await the
`JoinHandle`: a `JoinError` (panic) becomes a warned, retryable outcome that falls through
to the normal backoff/reconnect path. Build gates + **commit** `task(T5): …`.

### T3: Live multi-cycle durability run

**What to do**: Against the user's 2-minute-timelimit server, run
`cargo run --release --bin qbots -- competition --brains main,q3,xon --count 2 --navmodes nm,sg --chars grunt,major,sarge,camper --xonchars rus,shp,trt,nob`
in the background. Record the initial roster size, then poll `qbots status` about every
30 s for ≥4 map cycles (map name changes in the status report mark cycles; ≥10 min).
Pass = player count equals the initial roster at every post-settle poll (a dip during
the rotation window itself is fine; it must recover before the next poll or two) and any
watchdog trips visibly recover in the logs. Note results in the tracker, update
pitfalls/distilled if anything new surfaces, then move the plan to `completed/`.
**Commit** `task(T3): …` (tracker/docs).

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/qbots/src/main.rs` | frame-stall watchdog in `bot_task` | P0 |
| `crates/qbots/src/config.rs` | `Fleet.stall_timeout_ms` | P0 |
| `crates/qbots/src/supervisor.rs` | reset reconnect budget on success | P1 |
| `config.example.yaml` | document the new knob | P2 |

## Open Questions / Risks

1. **False trips on a genuinely slow server pause** — 10 s of zero frames is far outside
   normal operation, and a false trip costs one clean reconnect, not a lost bot.
   Mitigation: configurable knob.
2. **The stall might not be the only attrition path** — T3's live run is the arbiter; if
   bots still vanish, capture which supervisor/exit path fired (per-bot spans make this
   greppable) and extend the plan.
3. **Verification duration** — 4 cycles ≈ 10–12 min with a 2 min timelimit; the original
   failure took ~1 h at 10 min timelimit (≈6 rotations), so 4+ cycles exercises more
   rotations than the failing run.

## Verification Checklist

- [x] T1: `cargo build`/`clippy`/`test` clean; watchdog code path returns `ConnectionReset` while `Active` with stalled frames; committed (262679d95)
- [x] T2: budget reset on successful session; build gates clean; committed (ffb71086a)
- [x] T4: `roam_idx` reset in q3/main/xon `set_map` + regression test; committed (6f13cf044)
- [x] T5: brain panic → `JoinError` → retryable session end; committed (37531802c)
- [x] T3: live run — 36/36 at every settled poll across **9 rotations / ~18 min** (run 2, fixed binary); every rotation dip (incl. a 1-player hard-change trough at 11:14:42) recovered to 36/36 within one 30 s poll; **0 panics / 0 stalls / 0 give-ups** (run 1, pre-fix binary: 4 panics, 36→32, arithmetic exact); plan moved to `completed/`
