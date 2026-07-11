# Plan 53 — Fail hard when the bot roster can't fully join

> **Status**: done
> **Created**: 2026-07-11
> **Depends on**: Plan 52 (base64 spawn/teleporter work; unrelated but latest in SERIES)
> **Goal**: qbots exits non-zero with a clear error when any fleet/competition bot is
> rejected (server full) or times out joining; `--loose-botcap` opts back into
> proceed-with-warnings.
> **Agent**: sub-agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Make a too-large fleet a loud failure instead of a silent short-count, by
classifying handshake rejections in the connection FSM and propagating them to a
fleet-level fatal signal.

**Deliverables**:
1. `ConnState::Rejected` + `reject_reason` in the FSM; OOB `print` while `Connecting`
   is classified as a server rejection (not swallowed).
2. A connect-phase timeout in `bot_task` (config `connect_timeout_ms`, default 10 s).
3. Strict-by-default fleet behavior: any join failure tears the fleet down and returns a
   non-zero exit; `--loose-botcap` downgrades it to per-bot warnings.

**Estimated effort**: Small–Medium (half day)

## Context

Running `competition` on `base64`, the bot count scaled 6/12/18 then hard-capped at 18 —
further bots silently vanished. Diagnosis: **not** a qbots roster bug (total =
`navmodes × brains × chars × count`, `supervisor.rs:393-415`; `[fleet].max_bots` defaults
to 0 = uncapped). The wall is the **server's `maxclients`**: when full it sends a
connectionless `print\nServer is full.\n` (`vendor/yquake2/src/server/sv_conless.c:264`).

### Pre-Identified Bug/Issue

The rejection is silently swallowed: the OOB `"print"` arm in the FSM is a no-op
(`crates/client/src/conn.rs:148-150`), so a rejected bot hangs in `Connecting` forever —
no log, no timeout — and never returns, so the supervisor's backoff/"giving up" logic
(`supervisor.rs:628-631`) never fires. The user just sees fewer bots and no error.

### Why classify OOB-print-while-Connecting as a rejection

The server rejects at `SVC_DirectConnect`, **before** `client_connect` — i.e. before a
netchan exists. So any OOB `print` received while `state == Connecting` is unambiguously a
handshake rejection (`Server is full.` / `Bad challenge.` / `Connection refused.` /
protocol / password). In-band `svc_print` (MOTD/chat) arrives after Active and is handled
separately in `on_payload`, so it is unaffected.

## Step-by-Step Tasks

### T1: FSM classify handshake rejections

**File**: `crates/client/src/conn.rs`

**What to do**: Add `ConnState::Rejected` (unit variant, stays `Copy`). Add
`pub reject_reason: Option<String>` to `Conn` (init `None`). In `on_oob`'s `Some("print")`
arm, if `self.state == ConnState::Connecting`, capture the message
(`line.strip_prefix("print").unwrap_or(line).trim()`), store in `reject_reason`, set
`state = Rejected`; otherwise keep the informational no-op. Add unit tests for
`Server is full.`, `Bad challenge.`, `Connection refused.`.

### T2: `connect_timeout_ms` config

**File**: `crates/qbots/src/config.rs`

**What to do**: Add `pub connect_timeout_ms: u64` to `Fleet` with a doc comment; default
`10_000` in the `Default` impl.

### T3: `bot_task` surface reject + connect-phase timeout

**File**: `crates/qbots/src/main.rs`

**What to do**: Before the loop, `let connect_deadline = tokio::time::Instant::now() +
Duration::from_millis(cfg.fleet.connect_timeout_ms);`. In the recv arm, if
`conn.state() == ConnState::Rejected`, log + `return Err(io::Error::new(
ErrorKind::ConnectionRefused, format!("join rejected: {reason}")))`. In the ticker arm, if
`conn.state() != ConnState::Active && Instant::now() >= connect_deadline`, log + `return
Err(io::Error::new(ErrorKind::TimedOut, "connect handshake timed out"))`.

### T4: Fleet fatal signal + hard fail

**File**: `crates/qbots/src/supervisor.rs`

**What to do**: Add `join_failure: Arc<Mutex<Option<String>>>` + `loose_botcap: bool` to
`FleetShared`. In `bot_supervisor_loop`, when `bot_task` returns `Err(e)` with
`e.kind()` in `{ConnectionRefused, TimedOut}`: strict → set `join_failure` (first wins) +
`shared.shutdown.fire()` + return; loose → `warn!` + return. Other errors unchanged. In
`run_competition` and `run_fleet`, construct `FleetShared` with the failure slot +
`loose_botcap`, and after the task-join loop return `Err` if the slot is set.

### T5: `--loose-botcap` CLI flag

**File**: `crates/qbots/src/main.rs`

**What to do**: Add `#[arg(long)] loose_botcap: bool` to `Cmd::Competition` and `Cmd::Run`;
thread into `run_competition`/`run_fleet` as a trailing param. Competition dispatch already
maps `Err` → `ExitCode::FAILURE` (`main.rs:2065-2071`).

### T6: Verification + knowledge capture

Full `cargo fmt && cargo clippy -- -D warnings && cargo test`. Add a `context/pitfalls.md`
note. Move plan + tracker to `completed/`, mark SERIES done.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/client/src/conn.rs` | `ConnState::Rejected`, `reject_reason`, OOB reject parse | P0 |
| `crates/qbots/src/config.rs` | `Fleet.connect_timeout_ms` | P0 |
| `crates/qbots/src/main.rs` | `bot_task` reject/timeout returns; `--loose-botcap` flag | P0 |
| `crates/qbots/src/supervisor.rs` | fleet fatal signal + strict/loose behavior | P0 |

## Open Questions / Risks

1. **`Cmd::Run` default flip.** Making `run_fleet` strict-by-default also changes `run`
   behavior. Mitigation: same `--loose-botcap` escape hatch; document in the flag help.
2. **Connect timeout too tight.** Nav build happens after Active, so 10 s to reach the
   handshake milestone is generous. Mitigation: it's config-tunable.
3. **False positives on transient net loss during join.** A dropped challenge packet could
   time out a bot that would otherwise connect. Mitigation: 10 s covers several 100 ms
   ticks; strict mode is the intended loud-failure, loose mode is the escape.

## Verification Checklist

- [x] T1: `cargo test -p client` — reject-classification tests pass (full/bad-challenge/refused).
- [x] T2: `cargo build` clean with the new config field defaulted.
- [x] T3: `bot_task` returns `ConnectionRefused`/`TimedOut` on reject/timeout (compile + logic review).
- [x] T4: strict run exits non-zero with a `fleet join failed` error (live-verified via
      `connect_timeout_ms: 0` against the real server → `EXIT=1`).
- [x] T5: `--loose-botcap` run logs warns + keeps going (live-verified → `EXIT=0`,
      `dropping this bot`).
- [x] T6: no-regression live run (24 bots joined & fought under `maxclients=64`);
      `cargo fmt/clippy/test` all green (214+ tests); pitfalls note added.
