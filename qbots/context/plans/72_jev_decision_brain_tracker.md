# Plan 72 — Jev decision brain (`jev`) — Tracker

## Overview

- Status: 19% complete (3/16 tasks)
- Start date: 2026-11-06 (`date +%F`, local; the plan's `2026-11-07` dates are UTC)
- Plan: `context/plans/72_jev_decision_brain.md`
- Evidence: `context/fixtures/jev/` (harvested live request/response pairs from `qbots jev-probe`)
  and this tracker's **Evidence** table for every number. Live model runs are **human-run**:
  `cosmo.lan:8710` (model `h2oai/h2o-lightning-4b` Q8_0, `total_slots` 16, no auth). On the dev
  box `cosmo.lan` is an `/etc/hosts` entry for **127.0.0.1** — the dev box *is* the model host, so
  every latency here is **loopback**. Offline tasks run against committed fixtures + a scripted
  `DecisionTransport` + a fixture-replaying `TcpListener`, never the box. **No Q2 server is assumed
  offline either.**
- Controls: `q3` (secondary) and **`jev_static`** (CLI `jvs`, alias `jev_static`; primary — all
  plumbing, `NullDecider`).
- Composition check (not a jev claim): `spawn-to-spawn --map q2dm1` effective **≥ 13/24**, no
  `max_speed > 320`. jev is inert while `ctx.goal_override.is_some()` (T7's flag — posture and
  asks included), so that scenario exercises the inner q3 brain, never the decision layer — keep
  it as proof the wrapper didn't break q3.
- Effort: ~5–6 days total (W0–W3 ≈ 3 days, W4/W5/T16 ≈ 2–3 days). RULES' scale tops out at 3 days;
  if W3 slips, W4/W5 move to sub-plans `72_1`/`72_2` rather than being compressed.

## Resume Instructions

1. Read `AGENTS.md` (root **and** `../CLAUDE.md`), `context/plans/RULES.md` **in full**,
   `context/plans/72_jev_decision_brain.md`, then `context/distilled/jev.md` (T1's output — the
   wire contract and the measured latency/token envelope; **if T1 has not landed, the plan's
   Context section is sufficient for T2–T4**). Read the Concurrency Model section of `AGENTS.md`
   again before T15 — that task only exists because of the T13 carve-out.
2. **Who runs what.** An **automated session runs the offline span T6 → T2 → T3 → T4 → T5 →
   [W1 gate] → T7 → T8 and then stops.** (T6 is independent of T2–T5; W1 gates T7, the first task
   that needs the hub's types.) T9's code and its offline tests are the agent's, but its gate is
   *entirely* live and is the handoff point, not a task to grind on. After a human runs the W2 gate
   (including `calibrate`, which **freezes** the gate thresholds), the loop resumes at **T10 →
   T11**, stops again for the W3 gate, then two independent tracks: **T12** (→ W4) and **T14 → T15**
   (→ W5; T15 also needs T13). T1 and T13 are human-only. **No task in the offline span may reach
   for the network**: if an offline test needs it, the test is wrong.
3. **Live checks are wave-gate items, never task items** (this is the plan's main execution
   hazard). A task is `done` when its offline Verify passes **and is committed**; any line marked
   **Wave-gate check (live, human — Wn)** in the plan belongs to gate Wn and sits as a
   `_(pending live)_` row in the Evidence table until a human runs it. Statuses are RULES' set only:
   `pending` | `in-progress` | `done` | `blocked` | `skipped` (the last two always carry a reason).
   Never mark a gate passed on unit tests alone, and never mark a task `blocked` because its gate's
   live check hasn't run.
4. Environment: the offline span needs no box and no server. `crates/brain/Cargo.toml` must be
   **byte-unchanged** (`git diff --exit-code`); `crates/tools/Cargo.toml` is not touched (the probe
   is a `qbots` subcommand); `Cargo.lock` must gain **no new `name = "…"` package**, including no
   new workspace crate. `arc-swap`, `http`, `hyper`, `reqwest`, `rand` are all confirmed absent,
   hence banned. Rule A (fmt, `clippy -D warnings`, test) before every commit.
5. Ordering: **waves are gated.** T6 (the q3 seam) is the best first task for a fresh session —
   **first commit the pre-seam golden, then the refactor**; the golden must not change by a byte.
   It touches live play, so it gets its own commits and its own inertness proof. W2's gate is live
   and human-run; T10 cannot start before it is recorded. W3's gate is a **measurement**: if
   `gate_reject_rate` on event-triggered asks is ≥ 85 % the state does not discriminate the
   question — fix the state or delete it, and **if `admission_drop_rate` is non-trivial, raise
   `min_dwell_s` / `event_floor_s` and re-run instead; never loosen the (frozen) gate thresholds to
   produce a number.**
6. Two rules that outweigh convenience: **no `.await` and no HTTP inside the ticker arm**
   (`main.rs:946` is `time::interval(100ms)` with **no `MissedTickBehavior` call anywhere**, so
   tokio's `Burst` default is live, and `main.rs:1180-1186` is Plan 57's ack-on-arrival with a
   90 ms keepalive gap — a stall there perturbs reported ping for all 36 bots), and **all
   deadlines live in the hub on `Instant`** while the brain's own time stays `self.time += dt`
   (the one brain-side timer, `posture_max_age_s`, counts brain time). A bot must never be unable
   to play because the model is slow: *"jev never owns the absence of behaviour."*
7. Commit per task as `task(TN): …` (RULES Rule B), with **this tracker's row in the same commit**.

## Open Unknowns

- ~~`confidence` semantics~~ **resolved 2026-11-07 (review)**: `(p1 − 1/n)·n/(n−1)`, exact on three
  live answers (0.3263, 0.7604, 0.1620); the 24-option flat case fits within rounding. A function
  of `p1` alone — logged, never gated on. T1 records it and asks whether it is contractual.
- Where will the fleet run, and how does it resolve `cosmo.lan`? On the dev box: `/etc/hosts` →
  `127.0.0.1` (fleet and model share the host). Any other host must record its resolver path
  (`.lan` is not mDNS) and re-run `bench`. T1.
- Is `/v1/systemone` a stable API or a stopgap? The 501 wording reads purpose-built. T1.
- **Gate thresholds**: the margins `0.50` (binary) / `0.30` (3-way) are provisional; `calibrate`
  at W2 sets them from **pre-registered** obvious/ambiguous/flat sets and they are frozen from then
  on. If no threshold serves a question, the question is deleted.
- `noul` semantics — canary-only until validated against the captured flat fixtures, and fourth in
  the cut order.
- The exact `AGENTS.md` wording for the fleet-coach carve-out, and whether the human approves it
  at all (T13). **If declined, T15 is cut (T14's per-bot region answers stay log-only) and T16
  runs on the objective + posture layers, with the region layer deferred.**
- Response cache on or off: caching guarantees identical answers for similar bots (maximum herd);
  persona-differentiated prompts defeat the cache. **Off by default**; decided in T15 with
  reasoning. It is first in the cut order — it is a herding hazard with no consumer.
- Does `deadline_ms = 350` survive a loaded box? The 8-concurrent probe's **mean was ≈346 ms**, so
  350 is not comfortably above it. T9's bench must answer this; if p95 exceeds it, **raise dwell
  before raising deadline.**

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Record the contract + measurements, settle the open questions | `context/distilled/jev.md` | pending | **human-only, parallel with W1** — never blocks T2; label the numbers loopback; `confidence` is already resolved; strike unanswered questions, never delete |
| 2 | T2: Closed `DecisionState` + builder + exhaustive renderer | `crates/brain/src/jev/{state,question}.rs`, `crates/qbots/src/jev/{mod,render}.rs` | done | 4 brain + 5 qbots tests; byte-exact state **and** request goldens; 100× determinism; budget drop names the field. Deviations: `MapContext` instead of `&BrainMap`, `render_state(s, max)` returns all dropped keys, `MapId` closes the map name (see Notes) |
| 3 | T3: Margin gate + `Decision` + ticketed `DecisionSource` + stubs | `crates/brain/src/jev/{decision,source,gate}.rs` | done | gate = margin `p1−p2` (n ≤ 3), `n_eff` only for n ≥ 4; `submit → TicketId`, `latest → Option<AppliedDecision>` (`Copy`), `ack`; vectors **constructed in Rust** incl. the `{0.80,0.20}` regression row. 13 tests; objective travels as an index into its ticket's list (see Notes) |
| 4 | T4: Hand-rolled HTTP/1.1 POST + wire types | `crates/qbots/src/jev/{net,wire}.rs`, `crates/qbots/tests/jev_net.rs` | pending | `Connection: close`, chunked **and close-delimited**, timeout **wraps connect**, 1 MiB cap, extra-key + model-mismatch cases; **real clock, never `start_paused`** |
| 5 | T5: Hub — pull workers, bucket, tickets, pacing, generation, tiered breaker | `crates/qbots/src/jev/{hub,transport,mod}.rs`, `crates/qbots/Cargo.toml` | pending | **schedule risk**: 7 mechanisms + a test each; **no semaphore, no `permit_wait_ms`** — workers pick oldest event, else oldest refresh (≤ 2 in flight); hub runs the gate, slot stores only `AppliedDecision` |
| 6 | T6: q3 generic over `GoalPolicy` (behaviour-preserving) | `crates/brain/src/brains/q3/{policy,mod}.rs`, `q3/testdata/parity_200.txt` | done | golden `41f42b336` (pre-seam), seam in the T6 commit after it; golden byte-identical after the refactor; 12 old q3 tests unedited; 4 new seam tests; both mutation checks fail as they should (Evidence). Live no-op control → **W2** |
| 7 | T7: `JevBrain` / `JevStatic` + `jev:` config | `brains/jev/{mod,policy}.rs`, `brains/mod.rs`, `config.rs`, `config.example.yaml` (root), `roster.rs`, `supervisor.rs` | pending | `JevBrain { inner: Q3Brain<JevPolicy> }` intercepts `set_map`, sets the `goal_override` inert flag; `#[value(name="jvs", alias="jev_static")]`; `brain_code` `jev`/`jvs`; dead-endpoint test uses `127.0.0.1:1` |
| 8 | T8: Hub wiring + `on_map_change` + `goal_override` guard | `crates/qbots/src/main.rs`, `supervisor.rs`, `brains/core.rs` | pending | **start the hub at BOTH `supervisor.rs:323` and `:682`** or `competition` runs jev with no decider; triggers are flags, submit on the next tick; apply tests live in `brain`; `map_servercount = servercount` stays last |
| 9 | T9: `qbots jev-probe` | `crates/qbots/src/jev/probe.rs`, `main.rs` (`Cmd::JevProbe`) | pending | **not `crates/tools`** (qbots is bin-only); offline argv + calibration-search tests are the task; the live checks are the **W2 gate**; 501 needs a well-formed chat body |
| 10 | T10: `objective` question + candidates + triggers | `crates/brain/src/jev/objective.rs`, `brains/jev/policy.rs` | pending | candidates in `JevPolicy`; ≤3 seek/hold moves; resolve against the answer's **own ticket**; **held until released** (no lapse to roam); injection test through the real pipeline (in `qbots`) |
| 11 | T11: Telemetry — `applied/asked` + the two rates + herd | `crates/qbots/src/jev/telemetry.rs`, `main.rs`, `supervisor.rs` | pending | may move to right after T7; `applied` counted from `ack` only; herd = same-goal until T15; zero-applied ⇒ report first, then non-zero exit |
| 12 | T12: binary `posture` question, add-caution only | `crates/brain/src/jev/posture.rs`, `brains/jev/policy.rs` | pending | `disengage`/`hold`; `posture_max_age_s` 6.0 brain time; the high-aggression disengage test is the regression guard for the FSM loop |
| 13 | T13: `AGENTS.md` fleet-coach carve-out | `AGENTS.md` | pending | **human approval of the exact diff required before T15** |
| 14 | T14: Per-map region tables + per-bot region question + re-price | `crates/qbots/src/jev/regions.rs`, `config.example.yaml`, `state.rs`, `render.rs` | pending | answers **logged, not applied**; `STATE_SCHEMA_VERSION` → 2; re-price here: 420 tok, `min_dwell_s` 20, `ttl_s` 30 |
| 15 | T15: Code-side capped region assignment + herd control | `crates/qbots/src/jev/{assign,hub,telemetry}.rs`, `brains/jev/policy.rs` | pending | **no fleet-level model question**; greedy assignment under `region_cap`; objective > region > roam |
| 16 | T16: Acceptance A/B + docs + brain notes + pitfalls | `docs/BRAINS.md`, `context/brain_notes.md`, `context/pitfalls.md` | pending | N ≥ 5/arm, paired by `(map, servercount)`, include the predicted-no-difference metric; 10 named pitfalls |

**Status values** (RULES): `pending` | `in-progress` | `done` | `blocked` | `skipped`. `blocked` and
`skipped` always carry a reason in the Notes cell. Live checks are tracked in Wave Gates and
Evidence, not in task status (Resume step 3).

## Wave Gates

Recorded here (not just in prose) before the gated work may start.

| Gate | Before | Requires | Result |
|---|---|---|---|
| W1 | T7 | build/clippy/test green offline; **no new packages in `Cargo.lock`**; `crates/brain/Cargo.toml` byte-unchanged (T6 may land before or after — it does not wait on W1) | — |
| W2 | T10 | live `jev-probe`: slug + 501 fingerprint; **`calibrate` thresholds recorded and frozen** (or the question deleted); **both** sensitivity pairs move; anti-correlation; flatness rejected; bench p50/p95/p99 + tok/s vs both figures + p95-vs-`deadline_ms` verdict; T6's `q3,jev_static` no-op control; T8's `connect-one` smoke + refuse-to-start; with **`require: false`** and the endpoint unreachable, jev ≡ jev_static | — |
| W3 | T12, T14 | run `cargo run -p qbots -- competition --brains q3,jev_static,jev --count 12 --map q2dm1` **≥ 2×** on a parked q2dm1 (at 12 bots even the event worst case, 12/5 s = 2.4 req/s ⇒ 840 tok/s, is under the 1 000 knob, so the bucket is provably not the limiter). Publish per arm **and per question key**: `asked`, `admitted`, `applied`, `applied/asked`, `gate_reject_rate`, `admission_drop_rate`, the herd line. Decision rule: **`gate_reject_rate ≥ 85 %` on event-triggered asks ⇒ the state does not discriminate — fix the state or delete the question.** Non-trivial `admission_drop_rate` is **not** part of the judgement: raise `min_dwell_s` / `event_floor_s` and re-run. Never loosen the thresholds to move a number. | — |
| W4 | T16 | no frozen-bot scenario; `decisions_that_changed_per_bot_per_min` published (**> 2/bot/min = investigation**); composition check ≥ 13/24 labelled as a composition check | — |
| W5 | T16 | T13 human-approved; region `flatness` + `calibrate` pass; live 12-bot region histogram non-degenerate; re-priced budget asserted (T14) — or the region track recorded as cut because T13 was declined | — |

## Cut Order (if W2 slips)

None of these breaks the A/B — the experiment is `jev_static` vs `jev`, and every cut is off the
default path.

| Order | Cut | Why safe |
|---|---|---|
| 1 | T5's optional **response cache** | off by default, a herding hazard, no consumer until T15 decides |
| 2 | T9's **`matrix --live`** (40 scenarios) | keep `health`/`calibrate`/`sensitivity`/`anticorrelation`/`flatness`/`bench` — exactly what the W2 gate names. **Keep `--replay` if cheap**: it is the risk-6 detector |
| 3 | T11's **`debug`/`debug_sample` prompt dumps** | keep `st_hash` — the hash and the changed-fields `debug!` are the debugging path |
| 4 | T12's **`noul` canary** | explicitly not load-bearing, validated against the flat fixtures first |

**Never cut**: `jev_static`, the zero-applied hard-fail, the changed-fields `debug!`, the herd
metric, the `EVT jev_fleet` line, `calibrate`.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-07 | `curl` probes vs `cosmo.lan:8710/v1/systemone` (pre-plan characterisation; **loopback** — the dev box is the model host) | 1 noul 12–19 ms (82 tok); 3 q 58–147 ms (387 tok); 12 q 109–112 ms; 1621 tok 147–150 ms; 24 options 57 ms | 12 | 8 concurrent: 154→501 ms (**mean ≈346 ms**) ⇒ 6.2k tok/s hammered, **≈3.9k single-stream** |
| 2026-11-07 | 24-option `choice` over meaningless labels | top `p` 0.051, `confidence` 0.0096 ⇒ **flat** | 1 | the canonical "our gates must reject" fixture |
| 2026-11-07 | review probes: `POST /v1/chat/completions` with body `{}`, then with a well-formed chat body | `{}` ⇒ **400**; well-formed ⇒ **501** `not_supported_error` "use /v1/systemone" | 2 | the health fingerprint must send a valid body |
| 2026-11-07 | review probe: posture, state `hp:18(band:crit) armor:0 weapons:1/8 low:yes` / `enemy n:2 near:180 vis:yes`, criteria engage/trade/disengage | `{engage 0.182, trade 0.377, disengage 0.441}`, confidence 0.1620, 127 input tok | 1 | margin 0.065 — 3-way posture splits mass; motivated the binary question |
| 2026-11-07 | same state, criteria disengage/stay | `{disengage 0.663, stay 0.337}`, confidence 0.3263, 116 tok | 1 | first-draft `n_eff` gate: reject (1.89 > 1.5) |
| 2026-11-07 | extreme state (hp 5, blaster only, 3 enemies, "cannot win this fight"), disengage/stay | `{disengage 0.880, stay 0.120}`, confidence 0.7604, 142 tok | 1 | only this cleared the first-draft gate; `confidence = (p1−1/n)·n/(n−1)` exact on all three |
| 2026-11-06 | T6: `cargo test -p brain --lib brains::q3` after the generic seam | 22 q3-module tests pass incl. `q3_parity_golden`; `git diff --exit-code q3/testdata/` empty — golden **byte-identical** to the pre-seam recording (`41f42b336`) | 1 | golden covers seek-ltg/fight/chase/seek-nbg/retreat/battle-nbg, the `:506` veto, the `:926` roam arm, 2 recovery back-offs |
| 2026-11-06 | T6 mutation 1: `wants_chase = h_chase && post.disengage` (temporary, reverted) | `q3_parity_golden` **fails at tick 55** (chase phase) | 1 | the golden has teeth |
| 2026-11-06 | T6 mutation 2: first-draft merge `wants_chase = h_chase` with a forced disengage (temporary, reverted) | debug: the new `debug_assert` panics; `--release`: node ends **`"chase"`, expected `"retreat"`** | 1 | empirical confirmation of the review's FSM-loop bug; `disengage_on_a_chase_state_…` is its regression guard |
| 2026-11-06 | gate after T6 (`just all`) | fmt-check, clippy `-D warnings`, **583** tests, build — green; `crates/brain/Cargo.toml` unchanged | 1 | |
| _(pending live)_ | W2: `jev-probe calibrate` per question shape | | | thresholds recorded here, then frozen |
| _(pending live)_ | W2: T6 inertness — `competition --brains q3,jev_static` | | | `--brains q3,q3` is **not** the proof unless `matrix_specs` is shown to field distinguishable arms |
| _(pending live)_ | W2: T8 smoke — `connect-one` with `jev:`, then with the box unreachable | | | expect: refuse to start under `require: true` |
| _(pending live)_ | W2: T9 `bench` (config-derived rate) | | | p95 vs `deadline_ms = 350` |
| _(pending live)_ | W3 gate: `competition --brains q3,jev_static,jev --count 12 --map q2dm1` | | | three ratios per arm and per key |
| _(pending live)_ | W4: posture flips/bot/min + `spawn-to-spawn --map q2dm1` on the jev build | | | composition check only |
| _(pending live)_ | W5: region `flatness`/`calibrate` + 12-bot region histogram | | | |
| _(pending live)_ | T16 A/B (N ≥ 5/arm) | | | |

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
  region layer is not built on an unproven decision channel. The honest estimate (~5–6 days)
  exceeds RULES' 3-day scale; sub-plans `72_1`/`72_2` are the fallback if W3 slips.
- `Brain::status()` **can** carry a dynamic tag with zero trait change (stored `String` +
  `&self.status_tag`); an earlier draft claimed it needed a signature change, which was wrong and
  is corrected in open question 9. The deferral stands on its real reason: the decision surface
  already lives in `EVT jev_decision` and the report, and a `status()` prefix would break T12's
  assertion and the `status()` pattern in `crates/brain/tests/q3_brain.rs`.
- Budget table corrected pre-commit: one-sided ×[1.0,1.3] dwell jitter; `ttl_s` 24 with its
  inequality; "≈26 % of the honest single-stream 3.9k"; W5's ~420 tok asks need `min_dwell_s` 20.
- Both reviewers' pre-commit catches that changed the design, recorded so nobody re-introduces
  them: the `GoalPolicy` trait with `&mut self` methods behind a shared `Arc` made the policy
  **unreachable** from the wrapper; the first posture merge used `unwrap_or_else` on
  `wants_chase`, which let the model **add** aggression; `start_beacon`'s **second** call site at
  `supervisor.rs:682` means one hub spawn point would have left every `competition` run with no
  decider; `arc-swap` is not in the lockfile; a "Cargo.lock byte-unchanged" gate was unsatisfiable,
  so it is "no new packages"; and a single `applied/(applied+NoSignal)` ratio makes a
  budget-starved fleet indistinguishable from an under-discriminating state, hence the three-ratio
  split.
- **Post-commit review (2026-11-07), applied in a follow-up commit — nine design defects:**
  1. **Gate**: `n_eff ≤ 1.5` needs p1 ≥ 0.86–0.90 for n ≤ 3 (every question we ask), so its
     odds-ratio/p1 ≥ 0.75 companions never bound and almost nothing would apply (see Evidence).
     → margin gate, provisional 0.50/0.30, calibrated and frozen at W2.
  2. **Posture merge**: `retreat = h_r || model`, `chase = h_c && model` could make both true;
     `run_fsm` loops within a tick (Fight→Retreat→Chase→…) until `MAX_NODESWITCHES = 50` leaves
     the bot in a **Chase** that never calls `combat_drive`. → disengage also clears chase,
     `debug_assert`, binary `disengage`/`hold` question (engage could never act; trade mapped to
     nothing). Posture is computed once per tick before `select_enemy`; its `:506` effect (skip a
     distant unaware enemy) is now stated as the one way it touches acquisition.
  3. **Parity test**: `new()` delegated to `new_with_policy(NullPolicy)`, so the test compared the
     refactor with itself. → pre-seam golden committed first.
  4. **Duty cycle**: `max_age_at_apply_s = 8` vs a 15–19.5 s ask cycle lapsed every objective to
     roam for 7–11 s per cycle. → objectives held until released; posture gets its own 6 s.
  5. **Regions**: T14 asked per bot, T15 "one fleet-level question" — and one `choice` cannot
     yield N assignments. → per-bot preference (T14, log-only) + code-side capped assignment (T15).
  6. **Ownership**: `GoalPolicy::set_map(&mut self)` was uncallable through the shared `Arc`. →
     `Q3Brain<P: GoalPolicy = NullPolicy>`; `JevBrain` intercepts `set_map`.
  7. **Probe location**: `qbots` is bin-only, so `crates/tools` could not reuse its client; and
     `cargo run -p tools -- jev-probe` cannot select a binary. → `qbots jev-probe` subcommand.
  8. **Tickets**: `submit` returned nothing, so an answer could not be matched to the candidate
     list it answers. → `submit → TicketId`, a 4-ticket ring, `ack` for the `applied` count.
  9. **W2 gate**: "endpoint unreachable ⇒ jev ≡ jev_static" contradicted `require: true`. → run
     with `require: false`.
  Also corrected: the semaphore + worker-pool double model → one pull model (`permit_wait_ms`
  deleted; the "FIFO starves" claim was backwards); event demand bounded by `event_floor_s`;
  `ttl_s` inequality re-derived; the symmetric-jitter argument restated (the mean is unchanged, the
  provable floor is not); `confidence` identified; loopback labelling; `.lan` is not mDNS;
  `BrainKind::Quake3` (not `Q3`) and the short-code `ValueEnum` convention; `trait Brain` has no
  `note_*`; `config.example.yaml` lives at the qbots root and the config is YAML (`jev:`, not
  `[jev]`); default-method params named `_q` (clippy); no `start_paused` with real sockets; a
  close-delimited body case; `node:84`/`region:` removed from the pre-T14 state; W1 gates T7 not
  T6; the W4/W5 ordering made consistent; RULES statuses only, live checks moved to gates.
- **2026-11-06, before T6:** the project gate (`just all`) was already **red** — clippy 0.1.98
  (rustc 1.98.1) `needless_late_init` at `crates/brain/src/traverse.rs:261`, outside this plan.
  Fixed behaviour-preservingly in its own commit `8acd93bf4` (not a task commit); gate after:
  fmt-check, clippy `-D warnings`, 578 tests, build — all green. The T6 golden was recorded on top
  of that commit.
- **T2 deviations (2026-11-06):** (1) the builder takes `MapContext { map: MapId, nav_graph }`
  rather than `&BrainMap` — `BrainMap` is not `Clone` and the policy keeps field copies (T7), and
  the map **name** was itself an unlisted wire-text vector, now closed as `MapId` (unknown names
  ⇒ `Other`); (2) `render_state(state, max_tokens)` returns **every** dropped key
  (`Vec<FieldKey>`), not one — a single drop need not fit the budget; (3) question text is typed
  as `&'static str` / `Piece::{Lit, Num}`, so criteria (T10) cannot carry wire text either;
  (4) the request JSON is hand-written in declared order — `serde_json` in this lockfile has no
  `preserve_order`, so its `Map` would sort the offered options alphabetically; (5) the state's
  `loc` line is `exits` + `moving` (no region, no `floor`), matching the plan's no-opaque-index
  rule. `qbots::jev` carries a temporary `#![allow(dead_code)]` until T8 wires it (Follow-ups).
- **T3 deviations (2026-11-06):** (1) `AppliedDecision.objective` is an **index** (`u8`) into
  the answering ticket's offered list, not a label — the hub maps the validated wire label to its
  offered position, so no wire string reaches the brain and the slot stays `Copy`;
  `OffTicketLabel` = index outside that ticket's list; (2) non-200 statuses are
  `TransportErr::Http(u16)` (→ `err=http_501`), not `Malformed`; (3) `NoSignalReason` also has
  `TooFewOptions { n }` (a one-option question cannot discriminate); (4) `gate()` itself returns
  `Malformed(BadProbabilities)` for non-finite / negative / mis-summed vectors, so an invalid
  vector can never be gated as `Flat`; (5) `StubDecider` uses `std::sync::Mutex` — it is a test
  double; the production slot is still the hub's, in `qbots`.
- **T6 harvest:** the FSM-loop pitfall (one of T16's ten named entries) is already in
  `context/pitfalls.md` — confirmed by mutation in T6, so it was written then rather than at T16.
- `--brains` and `BrainKind` exist (`main.rs:249`, `:2634-2646`, `brains/mod.rs:35`). Any Verify
  naming a CLI command must be checked to exist and to field distinguishable arms before it is
  used as proof.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| `on_map_change` hook added separately from `on_death` | `main.rs:1398` fires `on_death` on level change *and* `:1592` on respawn; strategy resets are not death resets | done in T8, revisit if other brains want it |
| Dynamic `status()` tag (`"jev:hold-dock"`) | costs a stored `String` + one `format!` per applied decision; **no trait change needed** — deferred as nice-to-have, not blocked | open question 9; separate plan if operators ask |
| `jev-probe matrix --replay` promoted into T9 | cheapest detection of a server restart/settings change silently changing the readout | T9 (cut order position 2 — keep if cheap) |
| BSP-derived region partitioning | hand-authored tables are per-map labour; clustering/room partitioning is nav-graph-adjacent with the "if we say it's unreachable we're wrong" risk | separate plan |
| Re-run `jev-probe flatness` + `calibrate` + `matrix --live` after **any** server-side change | determinism is per-process; a restarted box with different slot/parallel settings may not reproduce, and frozen thresholds were calibrated against the old one | recurring, note in `context/pitfalls.md` (T16) |
| Per-trigger split of the herd/oscillation metrics | W3's gate needs it; W4/W5 want it trended across runs | T11, then T16's report shape |
| Re-run `bench` from the fleet host if the fleet ever leaves the model host | every latency here is loopback | T1 decision; recurring |
| Remove `#![allow(dead_code)]` from `crates/qbots/src/jev/mod.rs` | `qbots` is bin-only, so T2–T5's module has no caller until the hub is wired; anything still unused after T8 is genuinely dead | **T8** |
