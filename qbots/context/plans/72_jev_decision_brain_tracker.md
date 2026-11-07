# Plan 72 — Jev decision brain (`jev`) — Tracker

## Overview

- Status: 0% complete (0/16 tasks)
- Start date: — (set when T1 starts)
- Plan: `context/plans/72_jev_decision_brain.md`
- Evidence: `context/fixtures/jev/` (harvested live request/response pairs from `jev-probe`) and
  this tracker's **Evidence** table for every number. Live model runs are **human-run**:
  `cosmo.lan:8710` (model `h2oai/h2o-lightning-4b` Q8_0, `total_slots` 16, no auth). Offline
  tasks run against committed fixtures + a scripted `DecisionTransport` + a fixture-replaying
  `TcpListener`, never the box. **No Q2 server is assumed offline either.**
- Controls: `q3` (secondary) and **`jev_static`** (primary — all plumbing, `NullDecider`).
- Composition check (not a jev claim): `spawn-to-spawn --map q2dm1` effective **≥ 13/24**, no
  `max_speed > 320`. jev is inert while `ctx.goal_override.is_some()`, so that scenario exercises
  the inner `Q3Brain`, never the decision layer — keep it as proof the wrapper didn't break q3.

## Resume Instructions

1. Read `AGENTS.md` (root **and** `../CLAUDE.md`), `context/plans/RULES.md` **in full**,
   `context/plans/72_jev_decision_brain.md`, then `context/distilled/jev.md` (T1's output — the
   wire contract and the measured latency/token envelope; **if T1 has not landed, the plan's
   Context section is sufficient for T2–T4**). Read the Concurrency Model section of `AGENTS.md`
   again before T15 — that task only exists because of the T13 carve-out.
2. **Who runs what.** An **automated session runs the offline span T2 → T3 → T4 → T5 → [W1 gate] →
   T6 → T7 → T8 and then stops.** T9 is *entirely* live and is the handoff point, not a task to
   grind on. After a human runs T9 and records the W2 gate, the loop resumes at **T10 → T11**,
   stops again for the W3 gate, then T12/T14. T1 and T13 are human-only. **No task in the offline
   span may reach for the network**: if an offline test needs it, the test is wrong.
3. **Live-half rule** (this is the plan's main execution hazard): a task whose Verify has a live
   half — **T6, T8, T9 (all of it), T11, T12, T14, T15** — is marked **`done (offline)`** when the
   offline assertions pass, and a `_(pending live)_` row is added to the Evidence table below.
   **A live half is a wave-gate condition, never a task-completion condition.** Never mark such a
   task `done` on unit tests alone (overclaiming), and never mark it `blocked` and halt.
4. Environment: the offline span needs no box and no server. `crates/brain/Cargo.toml` must be
   **byte-unchanged** (`git diff --exit-code`); `Cargo.lock` must gain **no new `name = "…"`
   package** (adding tokio's `test-util` feature in T5 and `clap`/`serde_json` in T9 edits the
   lockfile's per-package dependency lists and that is allowed — a new crate is not). `arc-swap`,
   `http`, `hyper`, `reqwest`, `rand` are all confirmed absent, hence banned. Rule A (fmt,
   `clippy -D warnings`, test) before every commit.
5. Ordering: **waves are gated.** T6 (the q3 seam) is independent of T2–T5 and is the best first
   task for a fresh session — verifiable against the **12** tests already in `q3/mod.rs`, with
   zero new scaffolding — but it touches live play, so it gets its own commit and its own
   inertness proof. W2's gate is live and human-run; T10 cannot start before it is recorded. W3's
   gate is a **measurement**: if `gate_reject_rate` on event-triggered asks is ≥ 85 % the state
   does not discriminate the question — fix the state or delete it, and **if `admission_drop_rate`
   is non-trivial, raise `min_dwell_s` and re-run instead; never loosen the gate thresholds to
   produce a number.**
6. Two rules that outweigh convenience: **no `.await` and no HTTP inside the ticker arm**
   (`main.rs:946` is `time::interval(100ms)` with **no `MissedTickBehavior` call anywhere**, so
   tokio's `Burst` default is live, and `main.rs:1180-1186` is Plan 57's ack-on-arrival with a
   90 ms keepalive gap — a stall there perturbs reported ping for all 36 bots), and **all
   deadlines live in the hub on `Instant`** while the brain's own time stays `self.time += dt`.
   A bot must never be unable to play because the model is slow: *"jev never owns the absence of
   behaviour."*
7. Commit per task as `task(TN): …` (RULES Rule B), with **this tracker's row in the same commit**.

## Open Unknowns

- `confidence` semantics: is it exactly `top1 − 1/n`? (Flat 24-option case computed 0.0093 vs
  reported 0.0096.) We compute our own gate statistic either way; T1 resolves the wording.
- Does the box resolve `cosmo.lan` from wherever qbots will run? `getaddrinfo` does not resolve
  mDNS; a `/etc/hosts` entry on the dev box is not a contract. T1.
- Is `/v1/systemone` a stable API or a stopgap? The 501 wording reads purpose-built. T1.
- `noul` semantics — canary-only until validated against the captured flat fixtures, and fourth in
  the cut order.
- The exact `AGENTS.md` wording for the fleet-coach carve-out, and whether the human approves it
  at all (T13). **If declined, T14/T15 are cut and the plan closes after T12 with the region layer
  deferred.**
- Response cache on or off: caching guarantees identical answers for similar bots (maximum herd);
  persona-differentiated prompts defeat the cache. **Off by default**; decided in T15 with
  reasoning. It is first in the cut order — it is a herding hazard with no consumer.
- Does `deadline_ms = 350` survive a loaded box? The 8-concurrent probe's **mean was ≈346 ms**, so
  350 is not comfortably above it. T9's bench must answer this; if p95 exceeds it, **raise dwell
  before raising deadline.**

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Record the contract + measurements, settle 3 open questions | `context/distilled/jev.md` | pending | **human-only, parallel with W1** — never blocks T2; strike unanswered questions, never delete |
| 2 | T2: Closed `DecisionState` + builder + exhaustive renderer | `crates/brain/src/jev/state.rs`, `crates/qbots/src/jev/render.rs` | pending | injection test is **not** here (renderer can't see a `Worldview`); it lands in T10/T14 |
| 3 | T3: Gate algebra + `Decision` + `DecisionSource` + stubs | `crates/brain/src/jev/{decision,source,gate}.rs` | pending | table tests **constructed in Rust** — brain has no `[dev-dependencies]`/serde, so no brain test may read a JSON fixture |
| 4 | T4: Hand-rolled HTTP/1.1 POST + wire types | `crates/qbots/src/jev/{net,wire}.rs`, `crates/qbots/tests/jev_net.rs` | pending | `Connection: close`, chunked, timeout **wraps connect**, 1 MiB cap, extra-key + model-mismatch cases, assert the `Decision` variant not "no panic" |
| 5 | T5: Hub — bucket, transport seam, slots, generation, tiered breaker | `crates/qbots/src/jev/{hub,transport,mod}.rs`, `crates/qbots/Cargo.toml` | pending | **schedule risk**: 8 mechanisms + a test each; `Mutex<value>` slot (no `arc-swap`), `chars/4` reserve + true-up, timeouts don't open the breaker |
| 6 | T6: `GoalPolicy` seam in q3 (behaviour-preserving) | `crates/brain/src/brains/q3/{policy,mod}.rs` | pending | **independent of T2–T5, good first task**; `&self` methods + `Send + Sync`; posture = `\|\|` add-caution / `&&` remove-aggression; route `:506` too; parity test inside q3's own test module, no `tests/common` extraction |
| 7 | T7: `JevBrain` / `JevStatic` + `[jev]` config | `brains/jev/mod.rs`, `brains/mod.rs`, `config.rs`, `config.example.yaml`, `roster.rs`, `supervisor.rs` | pending | `build_brain_with_decider` sibling (7 existing call sites untouched); `status()` delegates verbatim; `brain_code` = `jev`/`jvs` for the 15-char `netname`; dead-endpoint test uses `127.0.0.1:1` |
| 8 | T8: Hub wiring + `on_map_change` + `goal_override` guard | `crates/qbots/src/main.rs`, `supervisor.rs`, `brains/core.rs` | pending | **start the hub at BOTH `supervisor.rs:323` and `:682`** or `competition` runs jev with no decider; no I/O in the ticker arm; `map_servercount = servercount` must stay last in the branch |
| 9 | T9: `jev-probe` | `crates/tools/src/bin/jev_probe.rs`, `crates/tools/Cargo.toml` | pending | **entirely live — the handoff point**; `--config` + `--json`; bench rate derived from config; harvests `context/fixtures/jev/`; *(W2 gate)* |
| 10 | T10: `objective` question + candidates + triggers | `crates/brain/src/jev/objective.rs`, `brains/jev/policy.rs` | pending | candidates generated **in `JevPolicy`** from its own `BrainMap` copy; ≤3 moves; **injection test through the real pipeline**; golden asserts the **total** request budget |
| 11 | T11: Telemetry — `applied/asked` + the two rates + herd | `crates/qbots/src/jev/telemetry.rs`, `main.rs`, `supervisor.rs` | pending | may move to right after T7; `EVT jev_fleet` every ~10 s; split by `(key × trigger)`; zero-applied ⇒ report first, then non-zero exit |
| 12 | T12: `posture` question, conservative-only bias | `crates/brain/src/jev/posture.rs`, `brains/jev/policy.rs` | pending | both sites never disagree; pinned posture still reaches `SeekLtg` (`:576/:598-599/:607/:614`); flips > 2/bot/min = investigation |
| 13 | T13: `AGENTS.md` fleet-coach carve-out | `AGENTS.md` | pending | **human approval of the exact diff required before T15** |
| 14 | T14: Per-map region tables + move generation | `crates/qbots/src/jev/regions.rs`, `config.example.yaml` | pending | 4–6 regions, anchors validated at load, legends name **contents**; `flatness` step is **live/human** |
| 15 | T15: Fleet-coach assignment + herd control + re-price | `crates/qbots/src/jev/{hub,telemetry}.rs` | pending | budget re-price is mandatory here (`tokens_per_req` 420, `min_dwell_s` 20) and re-check the `ttl_s` inequality; non-degenerate histogram |
| 16 | T16: Acceptance A/B + docs + brain notes + pitfalls | `docs/BRAINS.md`, `context/brain_notes.md`, `context/pitfalls.md` | pending | N ≥ 5/arm, paired by `(map, servercount)`, include the predicted-no-difference metric |

**Status values**: `pending` | `in-progress` | **`done (offline)`** | `done` | `blocked` |
`skipped` | `invalid` — `done (offline)` is used by any task with a pending live half (rule 3
above) and always has a matching `_(pending live)_` row in Evidence. The last three always carry a
reason in the Notes cell.

## Wave Gates

Recorded here (not just in prose) before the next wave may start.

| Gate | Before | Requires | Result |
|---|---|---|---|
| W1 | T6 | build/clippy/test green offline; **no new packages in `Cargo.lock`**; `crates/brain/Cargo.toml` byte-unchanged | — |
| W2 | T10 | live `jev-probe`: slug + 501 fingerprint, **both** sensitivity pairs move, anti-correlation, flatness rejected, bench p50/p95/p99 + tok/s vs both figures + p95-vs-`deadline_ms` verdict; endpoint unreachable ⇒ jev ≡ jev_static | — |
| W3 | T12 | run `cargo run -p qbots -- competition --brains q3,jev_static,jev --count 12 --map q2dm1` **≥ 2×** on a parked q2dm1 (`--count 12` keeps demand ≈ 1/3 of the fleet budget so the bucket is provably not the limiter). Publish per arm **and per question key**: `asked`, `admitted`, `applied`, `applied/asked`, `gate_reject_rate`, `admission_drop_rate`, the herd line. Decision rule: **`gate_reject_rate ≥ 85 %` on event-triggered asks ⇒ the state does not discriminate — fix the state or delete the question.** Non-trivial `admission_drop_rate` is **not** part of the judgement: raise `min_dwell_s` and re-run. Never loosen the thresholds to move a number. | — |
| W4 | T14/T15 | no frozen-bot scenario; `decisions_that_changed_per_bot_per_min` published (**> 2/bot/min = investigation**); composition check ≥ 13/24 labelled as a composition check | — |
| W5 | T16 | T13 human-approved; live 12-bot region histogram non-degenerate; re-priced budget asserted | — |

## Cut Order (if W2 slips)

None of these breaks the A/B — the experiment is `jev_static` vs `jev`, and every cut is off the
default path.

| Order | Cut | Why safe |
|---|---|---|
| 1 | T5's optional **response cache** | off by default, a herding hazard, no consumer until T15 decides |
| 2 | T9's **`matrix --live`** (40 scenarios) | keep `health`/`sensitivity`/`anticorrelation`/`flatness`/`bench` — exactly what the W2 gate names. **Keep `--replay` if cheap**: it is the risk-6 detector |
| 3 | T11's **`debug`/`debug_sample` prompt dumps** | keep `st_hash` — the hash and the changed-fields `debug!` are the debugging path |
| 4 | T12's **`noul` canary** | explicitly not load-bearing, validated against the flat fixtures first |

**Never cut**: `jev_static`, the zero-applied hard-fail, the changed-fields `debug!`, the herd
metric, the `EVT jev_fleet` line.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-07 | `curl` probes vs `cosmo.lan:8710/v1/systemone` (pre-plan characterisation) | 1 noul 12–19 ms (82 tok); 3 q 58–147 ms (387 tok); 12 q 109–112 ms; 1621 tok 147–150 ms; 24 options 57 ms | 12 | 8 concurrent: 154→501 ms (**mean ≈346 ms**) ⇒ 6.2k tok/s hammered, **≈3.9k single-stream** |
| 2026-11-07 | 24-option `choice` over meaningless labels | top `p` 0.051, `confidence` 0.0096 ⇒ **flat** | 1 | the canonical "our gates must reject" fixture; `reported ≈ top1 − 1/n` |
| _(pending live)_ | T6 inertness: `competition --brains q3,jev_static` | | | `--brains q3,q3` is **not** the proof unless `matrix_specs` is shown to field distinguishable arms |
| _(pending live)_ | T8 smoke: `connect-one` with `[jev]`, then with the box unreachable | | | expect: refuse to start under `require=true` |
| _(pending live)_ | T9 `bench` (config-derived rate) | | | p95 vs `deadline_ms = 350` |
| _(pending live)_ | W3 gate: `competition --brains q3,jev_static,jev --count 12 --map q2dm1` | | | three ratios per arm and per key |
| _(pending live)_ | T16 A/B (N ≥ 5/arm) | | | |
| _(pending live)_ | `spawn-to-spawn --map q2dm1` on the jev build | | | composition check only |

## Notes / Deviations

- Plan 71 (`brain_bugfixes`) is **still open** (T1/T2 shipped, T3/T4/T5/T6 not) and is marked
  **deferred in `SERIES.md`** so Rule C.5 does not block this plan. If a jev run shows ghost
  chasing, attribute it to plan 71 T5 rather than fixing it here.
- The brief said "a brain **copied** from q3". Both reviewers argued against the fork (the whole
  q3 module duplicated, and its **12** copied tests staying permanently green against the
  heuristic path while the model path has zero coverage) and the user chose **wrap q3 + a
  `GoalPolicy` seam**. T6/T7 implement that. A fork was therefore **not** built.
- The original design assumed a one-token **logit readout** over `/v1/chat/completions` with letter
  re-normalisation and prompt-prefix-cache games. **Probing the endpoint disproved it** (501:
  "This model is a decision model without text generation, use /v1/systemone"), and that whole
  sub-design — including the option-letter-tokenisation and `noul`-unobtainable objections — is
  deleted, not deferred.
- The user kept area control **inside plan 72** against both reviewers' "split to a follow-up"
  advice. Mitigation is the wave gates above; W3's gate is a triple measurement precisely so the
  region layer is not built on an unproven decision channel.
- `Brain::status()` **can** carry a dynamic tag with zero trait change (stored `String` +
  `&self.status_tag`); an earlier draft claimed it needed a signature change, which was wrong and
  is corrected in open question 9. The deferral stands on its real reason: the decision surface
  already lives in `EVT jev_decision` and the report, and a `status()` prefix would break T12's
  assertion and the `status()` pattern in `crates/brain/tests/q3_brain.rs`.
- Budget table corrected pre-commit: symmetric ±30 % dwell jitter **violated the token budget it
  was sized to protect** (1 200 tok/s demand on a 1 000 knob) → one-sided ×[1.0,1.3]; `ttl_s`
  20.0 was **below** the nominal worst path (20.05 s) → 24 with the inequality stated;
  "≈16 % of 6.2k tok/s" → **≈26 % of the honest single-stream 3.9k**; W5's ~420 tok asks need
  `min_dwell_s` 20, added to T15.
- Both reviewers' pre-commit catches that changed the design, recorded so nobody re-introduces
  them: the `GoalPolicy` trait with `&mut self` methods made the policy **unreachable** from the
  wrapper (`Rc<RefCell<_>>` would have broken `Brain: Send`); the first posture merge used
  `unwrap_or_else` on `wants_chase`, which let the model **add** aggression; `start_beacon`'s
  **second** call site at `supervisor.rs:682` means one hub spawn point would have left every
  `competition` run with no decider; `arc-swap` is not in the lockfile; a "Cargo.lock
  byte-unchanged" gate is unsatisfiable once tokio `test-util` and tools' clap are needed, so it is
  now "no new packages"; and a single `applied/(applied+NoSignal)` ratio makes a budget-starved
  fleet indistinguishable from an under-discriminating state, hence the three-ratio split.
- `--brains` and `BrainKind` exist (`main.rs:249`, `:2634-2646`, `brains/mod.rs:35`). Any Verify
  naming a CLI command must be checked to exist and to field distinguishable arms before it is
  used as proof.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| `on_map_change` hook added separately from `on_death` | `main.rs:1398` fires `on_death` on level change *and* respawn; strategy resets are not death resets | done in T8, revisit if other brains want it |
| Dynamic `status()` tag (`"jev:hold-dock"`) | costs a stored `String` + one `format!` per applied decision; **no trait change needed** — deferred as nice-to-have, not blocked | open question 9; separate plan if operators ask |
| `jev-probe matrix --replay` promoted into T9 | cheapest detection of a server restart/settings change silently changing the readout | T9 (cut order position 2 — keep if cheap) |
| BSP-derived region partitioning | hand-authored tables are per-map labour; clustering/room partitioning is nav-graph-adjacent with the "if we say it's unreachable we're wrong" risk | separate plan |
| Re-run `jev-probe flatness` + `matrix --live` after **any** server-side change | determinism is per-process; a restarted box with different slot/parallel settings may not reproduce | recurring, note in `context/pitfalls.md` (T16) |
| Per-trigger split of the herd/oscillation metrics | W3's gate needs it; W4/W5 want it trended across runs | T11, then T16's report shape |
