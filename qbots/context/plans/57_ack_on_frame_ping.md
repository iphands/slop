# Plan 57 — Ack-on-frame send re-phasing (drop bot ping to ≈RTT)

> **Status**: in-progress
> **Created**: 2026-07-11
> **Depends on**: Plan 09 (fleet) | Plan 22 (Brain seam) | Plan 51 (`EVT` instrumentation pattern)
> **Goal**: Send each `clc_move` the instant a `svc_frame` arrives (acking it), with the 10 Hz timer demoted to a keepalive fallback, so the server-measured ping collapses from ~50–80 ms to ≈ true RTT — without changing send rate or movement speed.
> **Agent**: sub-agent (interactive)

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Re-phase the outgoing `clc_move` from a free-running 10 Hz timer to *frame
arrival*, so the server measures the ack of frame N ~1 RTT after sending it instead of
RTT + up-to-100 ms of self-inflicted timer phase.

**Deliverables**:
1. `SendTiming` primitive measuring the self-inflicted frame-arrival→ack phase delay.
2. Hybrid send in the fleet loop: ack on frame arrival + 10 Hz keepalive fallback.
3. Same re-phasing in the `conn.rs` reference loop; scenario harness explicitly opted out.
4. Distilled/pitfalls notes on Q2 ping mechanics.

**Estimated effort**: Small–Medium (half day).

---

## Context

### Why the pings are high (root cause — confirmed from vendor source)

Quake 2's displayed "ping" is **not** network RTT. `SV_CalcPings` averages the last 16
per-frame latency samples, each computed as:

```
latency = (svs.realtime when the client's clc_move acking frame N arrives)
        − (frame[N].senttime, stamped when the server sent frame N)
```

- Sample: `vendor/yquake2/src/server/sv_user.c:686-696` (only when `lastframe != cl->lastframe`).
- `senttime` stamp: `vendor/yquake2/src/server/sv_entities.c:531-533`.
- Average: `vendor/yquake2/src/server/sv_main.c:131-164`, `LATENCY_COUNTS = 16`.

So the number **includes the client's own reply delay** — how long the bot sat on a
frame before its next outgoing packet; the server cannot subtract it.

**Our bots send `clc_move` only on a free-running 100 ms (10 Hz) timer and never reply
on frame arrival.** A freshly-arrived frame waits ~50 ms on average (0–100 ms uniform)
for the next tick — added on top of true RTT, exactly explaining the observed 50–80 ms.

- Send only in `ticker.tick()`: `crates/qbots/src/main.rs:904`, transmit `main.rs:1258`.
- Frame arrival only *stores* the frame, returns `None`: `crates/client/src/conn.rs:228-240`.
- Timer: `main.rs:824` (`interval(from_millis(100))`).

### Key facts that make the fix safe

1. **Server accepts commands as fast as sent** — no packet-rate limit; only the
   `commandMsec` real-time budget (`sv_user.c:606-618`), never exceeded by re-phasing.
2. **Server emits frames at fixed 10 Hz** (`sv_main.c:343-344`), so acking every frame
   is still ~10 sends/sec — **the same rate we send today**. Movement speed = `msec` ×
   sends/sec (`move_ctrl.rs:103-110`, sent 3× per packet); both stay constant. We
   re-phase, not speed up.
3. Real Q2 clients decouple decision/render fps from the packet frame (`cl_input.c:732-833`,
   gated `cl_main.c:899-901`). We mirror it: **decision stays 10 Hz, packet re-phased**.

### Design — re-phase the send, keep the decision on the timer

- **Decision (brain tick) stays in `ticker.tick()`**; cache the built `Usercmd` (+ its
  `msec`) so the recv arm can re-send it.
- **Send moves to frame arrival**: recv arm compares `conn.frame.serverframe` before/after
  `on_recv`; on a new frame while `Active`, transmit the cached cmd immediately.
- **Timer send becomes conditional** (dedupe): transmit only if no frame-triggered send
  in the last ~90 ms → ≈10 sends/sec from frames, timer fills stalls only.
- **`msec` unchanged in steady state** (frame-arrival send reuses the timer's cached
  `msec`); only the rare keepalive path uses wall-elapsed msec.

**Net**: ack of frame N leaves ~immediately (≈1 RTT) instead of RTT + up-to-100 ms;
scoreboard ping → true RTT; combat control loop tightens ~50 ms as a bonus.

### Why hybrid (not the alternatives)

Ack-on-frame alone risks freezing on a frame stall; raising the raw rate 5×'s packet
load across the fleet for a worse result. Hybrid = ping ≈ RTT **and** robust to stalls,
byte-identical movement in steady state.

---

## Step-by-Step Tasks

### T1: `SendTiming` instrumentation primitive (pure, unit-tested)

**File**: `crates/client/src/send_timing.rs` (new); export from `crates/client/src/lib.rs`.

**What to do**: Pure struct measuring the self-inflicted reply delay this plan removes,
so before/after is provable locally (independent of the server scoreboard read by
`crates/qbots/src/status.rs`).

- Ring of last ~16 frame-arrival `Instant`s keyed by `serverframe` (mirrors `LATENCY_COUNTS`).
- `on_frame(serverframe, now)` — stamp arrival.
- `on_ack_sent(acked_serverframe, now) -> Option<Duration>` — phase `now − arrival(acked)`
  if known; feeds EMA + max + count.
- `snapshot() -> SendTimingStats { ema_ms, max_ms, sends, late }`; `late` = phase > ~40 ms
  (starved-loop / "are we ever late" flag).
- `Instant` injected (pass `now`) so the core is deterministic + unit-tested (no wall clock
  in tests). Logging shape mirrors Plan 51 `EVT wall_press` (`main.rs:1226-1239`); the log
  line lives in main.rs (T2), the math here.

**Commit**: `task(T1): SendTiming primitive — measure frame-arrival→ack phase delay`

### T2: Hybrid ack-on-frame send in the fleet loop

**File**: `crates/qbots/src/main.rs` (`bot_task`, ~`866-1281`).

1. Pre-loop state: `last_send: Instant`, `last_cmd: Option<Usercmd>`, `send_timing: SendTiming`.
2. Recv arm (`main.rs:880`): snapshot `prev_sf` before `on_recv`; after, if `Active` and
   `conn.frame.serverframe` changed → `send_timing.on_frame`, and if `last_cmd` is set,
   `transmit_cmd` it, `send_timing.on_ack_sent`, `last_send = now`.
3. Timer arm (`main.rs:904`): keep the whole decision body; after `build_cmd`, store
   `last_cmd = Some(cmd)`; replace the unconditional transmit (`main.rs:1258`) with a
   keepalive-only send gated on `now − last_send >= 90 ms` (wall-elapsed msec on that path).
4. Reuse the `ticks.is_multiple_of(10)` heartbeat (`main.rs:1262`) to log
   `EVT send_timing ema=… max=… late=…`.

**Guardrail**: frame-arrival send reuses the timer's cached `msec`; the 90 ms dedupe
prevents a double-send (which would 2× `msec` integration → 2× move speed).

**Commit**: `task(T2): ack clc_move on frame arrival; demote 10Hz timer to keepalive`

### T3: Reference loop parity (`conn.rs::run`) — scoped

**File**: `crates/client/src/conn.rs` (`run`, `331-379`).

Re-phase the reference loop: new-frame detect in the recv arm → `keepalive()` when
`Active`; gate the timer `keepalive` on the same 90 ms dedupe (no new Conn API —
`keepalive()` already emits a real `clc_move` when `Active`).

**Out of scope**: `crates/qbots/src/scenario.rs` (`spawn-to-*`, timer `scenario.rs:404`) —
its Plan 10–14 movement baselines were recorded against the 10 Hz free-running send;
re-phasing could shift `mean_speed`/elapsed. Leave as-is + a one-line comment citing this
plan.

**Commit**: `task(T3): re-phase conn.rs reference loop; document scenario opt-out`

### T4: Knowledge capture + close-out

**Files**: `context/distilled.md`, `context/pitfalls.md`, `context/plans/SERIES.md`.

- `distilled.md`: dense "Q2 ping = reply-phase + RTT" note (formula, `sv_user.c:686-696`
  sample, why 10 Hz free-running inflates ~50 ms, the ack-on-frame fix; cite vendor lines).
- `pitfalls.md`: "High bot ping is self-inflicted send phase, not the network / not 10 Hz"
  → problem → fix (ack on frame arrival, dedupe timer, keep msec/rate constant) → source.
- `SERIES.md`: Plan 57 row (done) + "After 57" milestone.
- Move plan + tracker to `completed/`; mark SERIES done.

**Commit**: `task(T4): distill Q2 ping mechanics; close Plan 57`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/qbots/src/main.rs` | Hybrid ack-on-frame send; cache `last_cmd`; dedupe timer; `EVT send_timing` | P0 |
| `crates/client/src/send_timing.rs` (new) | `SendTiming` phase-delay primitive | P0 |
| `crates/client/src/lib.rs` | export `SendTiming` | P0 |
| `crates/client/src/conn.rs` | Re-phase reference `run` loop | P1 |
| `crates/qbots/src/scenario.rs` | One comment: opt-out + rationale (no behavior change) | P2 |
| `context/distilled.md`, `context/pitfalls.md`, `SERIES.md` | Knowledge capture + close-out | P1 |

---

## Open Questions / Risks

1. **`msec` double-count → 2× move speed.** Biggest risk. Mitigation: 90 ms dedupe +
   reuse cached `msec`. Verify: `spawn-to-spawn` log `mean_speed`/`max_speed` unchanged
   (a `max_speed > ~320` flags the bug).
2. **First-frame race**: no `last_cmd` yet → skip the frame-arrival send until the timer
   builds one.
3. **New-frame detect**: compare `conn.frame.serverframe` before/after `on_recv` (public
   field `conn.rs:59`); no Conn API change.
4. **Delta state**: `build_clc_move` deltas the 3 usercmds within one packet (self-contained);
   re-phasing doesn't corrupt it. Confirm `build_cmd` has no per-send mutation beyond the
   local `self.last_cmd` echo.
5. **LAN RTT ~0** → ping may read 0–1 ms after the fix (correct). `EVT send_timing` is the
   primary before/after signal; scoreboard confirms.

---

## Verification Checklist

- [ ] T1: `cargo test -p client send_timing` green (phase/ring/EMA/late).
- [ ] T2: `cargo build && cargo clippy -- -D warnings && cargo fmt --check` clean; `cargo test` green.
- [ ] T2 (live): fleet run → `qbots status` pings in true-RTT range (LAN ≈ 0–5 ms); logs show `EVT send_timing ema=<single-digit ms>`.
- [ ] T2 (regression): `spawn-to-spawn --map q2dm1` still `reached=1`, `mean_speed`/`max_speed` unchanged.
- [ ] T3: `conn.rs::run` re-phased; `scenario.rs` opt-out comment present, baseline untouched.
- [ ] T4: `distilled.md` + `pitfalls.md` notes on disk; `SERIES.md` row + milestone; plan moved to `completed/`.
- [ ] All tasks committed `task(TN): …`; warnings/clippy/tests clean before each commit.
