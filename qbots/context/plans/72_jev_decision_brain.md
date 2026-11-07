# Plan 72 — Jev decision brain (`jev`): model-driven high-level play over a q3 core

> **Status**: in-progress
> **Created**: 2026-11-07
> **Revised**: 2026-11-07 (pre-commit) — corrected after two artifact reviews: the `GoalPolicy`
> trait shape (ownership made the policy unreachable from the wrapper), the posture merge (it
> permitted *more* aggression, the one thing the design forbids), three budget-table arithmetic
> errors, an unsatisfiable `Cargo.lock` gate, a missing second hub spawn point, and a false
> justification for deferring the `status()` tag.
> **Revised**: 2026-11-07 (post-commit review; a follow-up commit, never an amend) — nine design
> defects corrected: (1) the `n_eff ≤ 1.5` gate demanded p1 ≥ 0.86–0.90 for every n ≤ 3 question
> and would have rejected nearly every answer — now a top-two **margin** gate with provisional
> thresholds **calibrated at W2**; (2) the posture merge could make `wants_retreat` **and**
> `wants_chase` true together, cycling `run_fsm` Fight→Retreat→Chase→… until the 50-switch guard
> left the bot in a **non-firing Chase** — a disengage now also clears chase, and posture is a
> binary question; (3) the T6 parity test compared the new code with itself — now a **pre-seam
> golden**; (4) `max_age_at_apply_s = 8` against a 15–19.5 s ask cycle left objectives lapsing to
> roam for 7–11 s of every cycle — objectives are now **held until released**; (5) T14/T15
> disagreed on where the region question lives — now **per-bot preference + code-side capped
> assignment**; (6) `GoalPolicy::set_map(&mut self)` was uncallable through a shared `Arc` — q3
> is now **generic** `Q3Brain<P: GoalPolicy = NullPolicy>`; (7) `jev_probe` in `crates/tools`
> could not reach the bin-only `qbots` crate — it is now a **`qbots jev-probe`** subcommand;
> (8) answers carried no **ticket** tying them to the candidate list they answer; (9) the W2
> unreachable-endpoint check contradicted `require: true`. Also: the semaphore + worker-pool
> double model is now one pull model, event demand is budgeted, `confidence` is identified, and
> the measurements are labelled as loopback.
> **Depends on**: Plan 24 (the `trait Brain` plugin seam), Plan 37/38 (`q3` + `q3char` — the
> posture functions this plan biases), Plan 66 (`beacon.rs` — the process-wide coalescing-hub
> precedent). Lightly: Plan 29 (`engage::EngageTracker`), Plan 57 (ack-on-frame timing that
> must not be perturbed).
> **Goal**: Add a `jev` brain that **wraps the real `Q3Brain`** and takes its high-level
> decisions — objective, engagement posture, and area control — from the hosted
> `POST /v1/systemone` decision model at `cosmo.lan:8710`, with the model never required
> frame-to-frame and every frame still producing valid q3 play.
> **Agent**: implementation agent (ralph-loop) for T2–T8 and T10–T11 (**offline span**); **T1, T9,
> T12's live half, T13 and T15's live check are human-run**, as are every Verify that names
> `competition` / `connect-one` / `spawn-to-*` — **no Q2 server is assumed offline**.

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.
> Rule A (zero warnings) and Rule B (commit per task) are not optional here.

---

## TL;DR

**What**: Add `BrainKind::Jev` — a thin brain that composes the existing q3 brain as
`Q3Brain<JevPolicy>` and, through a new `GoalPolicy` type parameter inside q3, substitutes the
model's answer for three *high-level* choices (which objective to pursue, whether to disengage,
which area to prefer).
All model I/O lives in one process-wide hub in `crates/qbots`; the brain only ever reads a
fresh, code-validated decision out of a slot, so a dead endpoint degrades to pure q3 per answer
and never stalls a bot's socket loop.

**Deliverables**:
1. `crates/brain/src/jev/` — closed-vocabulary `DecisionState` + its `Worldview`→state builder,
   the `Decision`/`NoSignalReason` gate type, `DecisionSource` trait (no `async fn`), `NullDecider`,
   `StubDecider`, `JevBrain`, `JevPolicy`. **`crates/brain/Cargo.toml` is not touched at all.**
2. `crates/brain/src/brains/q3/policy.rs` — the `GoalPolicy` trait (q3 becomes
   `Q3Brain<P: GoalPolicy = NullPolicy>`, the `HashMap<K, V, S = RandomState>` pattern) over q3's
   roam-fallback goal and its posture inputs, defaulting to today's code (**behaviour-preserving**,
   gated by a 200-tick golden trace recorded from the **pre-seam** code).
3. `crates/qbots/src/jev/` — hand-rolled HTTP/1.1 `POST` (~150 lines, **no new packages**), the
   `/v1/systemone` wire types, the closed-set JSON renderer, and the hub: newest-wins snapshot
   slots per bot, **one pull-model pool of `max_concurrent` workers** behind an injectable
   transport seam, a **fleet token bucket**, hub-issued **tickets**, a generation counter and a
   tiered circuit breaker. The hub runs the gate; a bot's slot only ever holds a gated,
   `Copy` decision.
4. `crates/qbots/src/jev/probe.rs` — the **`qbots jev-probe`** subcommand (`health` / `ask` /
   `bench` / `calibrate` / `sensitivity` / `anticorrelation` / `flatness` / `matrix`, +`--json`;
   reads the fleet's own `--config`), the only thing that talks to the live box during
   development, and the harvest tool for the offline fixtures. It lives in `qbots` because `qbots`
   is a bin-only crate — `crates/tools` cannot import its HTTP client, wire types or config.
5. One `objective` question (≤3 offered moves, event-triggered, **held until released**), one
   **binary** `posture` question (`disengage` / `hold` + `noul` canary, permitted **only to add
   caution**: a disengage sets retreat *and clears chase*), and a region layer of 4–6 regions asked
   per bot as ≤3 *moves*, with a **code-side, capacity-capped fleet-coach assignment** over the
   bots' answers.
6. Telemetry that can answer "did the model do anything at all?": `EVT jev_*` lines including
   `err=` and `keys=`, a continuous `EVT jev_fleet` line, `applied/asked` as the headline with
   `gate_reject_rate` and `admission_drop_rate` split out, decision-age p50/p95,
   `distinct_choices`, and a **herd metric** (max concurrent bots on the same objective goal; per region from T15). `competition`
   hard-fails if the jev arm applied 0 decisions — **after** printing the report.
7. Docs: `context/distilled/jev.md` (contract + measurements), `docs/BRAINS.md` §6,
   `config.example.yaml`, a dated `context/brain_notes.md` append, and `context/pitfalls.md`.

**Estimated effort**: Large (3 days) **for W0–W3 alone** (T1–T11), offline-green throughout; W4 +
W5 + acceptance (T12–T16) are **another ~2–3 days**, so **~5–6 days in total**. RULES' effort scale
stops at 3 days; the honest figure is stated rather than squeezed into it. The user kept area
control inside this plan; if W3 slips, the fallback is to move W4/W5 into sub-plans `72_1`
(posture) and `72_2` (regions) per RULES' `NN_N` naming, not to compress them. **T5 is the overrun
risk** (seven interacting mechanisms + a test each). The live gates are a human's half-day on the
box per wave. Cut order if W2 slips is in the tracker; none of the four cuts breaks the A/B.

---

## Context

### The endpoint, measured 2026-11-07

`cosmo.lan:8710` is **not** a chat server. It natively implements the TypeSafe **System One**
decision API; `/v1/chat/completions` answers `501 {"error":{"message":"This model is a
decision model without text generation, use /v1/systemone"}}` — that 501 is our capability
fingerprint, assert it in `jev-probe health`. **Send a well-formed chat body**
(`{"model":"x","messages":[{"role":"user","content":"hi"}]}`): an empty `{}` body is rejected with
a **400** before the route is reached (verified 2026-11-07), so a lazy probe would see the wrong
status and call the box broken.

```text
POST /v1/systemone
{"state": <string|object|array>,
 "questions": {key: {"type":"choice","instructions":…,"criteria":{label:desc}}
             | {"type":"score","instructions":…,"criteria":[lv0,lv1,…]}
             | {"type":"noul","instructions":…}}}

→ {"model":"h2oai/h2o-lightning-4b",
   "answers":{key:{"type":"choice","choice":L,"probabilities":{label:p},"confidence":c}
            | {"type":"score","score":f,"legend":{"0":…},"probabilities":{…},"confidence":c}
            | {"type":"noul","noul":p}},
   "usage":{"input_tokens":N,"output_tokens":0}}
```

Model `h2oai/h2o-lightning-4b` Q8_0, `n_ctx` 40960, `total_slots` 16, no auth, plaintext.

**All figures below are loopback.** The dev box *is* the model host: its `/etc/hosts` maps
`cosmo cosmo.lan` to `127.0.0.1`, so no network round-trip is in any number. That is the real
deployment if the fleet runs on the same box; a fleet on another host must re-run `jev-probe
bench` from there before trusting `deadline_ms`.

| Probe (dev box = model host, loopback) | Result |
|---|---|
| 1 tiny `noul` (82 tok) | **12–19 ms** |
| 3 questions, choice+score+noul (387 tok) | **58 / 96 / 98 / 142 / 142 / 147 ms** |
| 12 short `noul`s, one call | **109–112 ms** |
| 6 KB state (1621 tok), 1 question | **147–150 ms** |
| 24-option `choice` | **57 ms** |
| **8 concurrent** 3-question requests | **154, 293, 294, 294, 411, 411, 501, 501 ms** |
| 300 questions in one request | accepted |
| Repeat of identical request ×3 | **byte-identical** probabilities |
| Bad question `type` | `400 …"type" must be one of: choice, score, noul` |

Four facts we design on:

1. **Cost tracks input tokens, not question count.** 1 question at 1621 tok (148 ms) costs more
   than 3 questions at 387 tok (100 ms); marginal cost is ~8 ms/question and ~48 ms per 1 200
   extra state tokens. **State size is the budget; never split questions across calls.**
2. **Two throughput numbers, and the honest one is the lower.** The 8-concurrent probe moved
   8 × 387 = 3 096 tok in 501 ms ⇒ **≈6.2k tok/s**, but *only in the regime where per-request
   latency degraded 154→501 ms*. Single-stream, a 3-question request is 387 tok / ~100 ms ⇒
   **≈3.9k tok/s equivalent**. Quote **3.9k** as the headroom figure; 6.2k is what you get by
   hammering, which is precisely the regime we design away from.
3. **Fully deterministic**, `output_tokens` always 0, `probabilities` sum to 1.0. This is a
   **scoring oracle, not a generative call**: retries are safe, responses are cacheable, and
   there is no temperature to decorrelate anything (see *Determinism ⇒ herding*).
4. **A 24-option `choice` over semantically empty labels returns a flat distribution**
   (top `p` 0.051, `confidence` 0.0096). Wide option sets are *noise by construction*. This is
   a first-class design constraint, not an anecdote — see *Gates*.

**`confidence` is identified: `confidence = (p1 − 1/n) · n/(n−1)`**, the top-1 excess normalised
to `[0,1]` (verified 2026-11-07 on three live answers: binary `{0.6632, 0.3368}` → 0.3263, binary
`{0.8802, 0.1198}` → 0.7604, 3-way top 0.4413 → 0.1620 — all exact; the 24-option flat case,
top 0.051 → 0.0096, fits within the rounding of the quoted `p`). It is a function of `p1` alone,
so it is **not** an independent second opinion and we never gate on it. Log it, nothing more.

### Why wrap `q3` instead of forking it

The original brief said "a brain copied from q3". Both reviewers argued against the fork and the
user chose the wrapper. What a fork costs is not merely the module's bulk (`q3/mod.rs` +
`move.rs` + `aim.rs` — quote `wc -l` if you need the number) but its **12** `#[test]` functions
in `q3/mod.rs`, which would stay **permanently green while exercising the heuristic path**, so
the model path ships with zero coverage and every future q3 fix needs a twin commit that nobody
will make.

So: `JevBrain` **composes** a real q3 brain (`crates/brain/src/brains/jev/mod.rs`, ~120 lines)
and the only thing q3 changes is that its roam-fallback goal (`roam_goal`, `q3/mod.rs:211`,
called from the `:926` arm — `battle_goal` `:407` and `retreat_goal` `:421` stay untouched) and
its posture inputs (`crate::q3char::wants_to_retreat` / `wants_to_chase`, called at
`q3/mod.rs:890-891` **and again at `:506`** for the sneak-past veto) consult a `GoalPolicy`
whose default impl **is today's code**. `--brains q3,jev_static` then differs by one variable,
which is a better experiment than a forked file, and drift is structurally impossible.

**The policy is a type parameter, not a shared trait object.** q3 becomes
`Q3Brain<P: GoalPolicy = NullPolicy>`, with `new(ch)` defined only on `Q3Brain<NullPolicy>`
(exactly how `HashMap::new` is defined only on `HashMap<K, V, RandomState>`), so every existing
`Q3Brain::new(ch)` and every `&mut Q3Brain` in a signature keeps compiling **unedited**.
`JevBrain { inner: Q3Brain<JevPolicy> }` reaches its policy through `inner.policy_mut()`. That
gives `&mut self` trait methods with no `Arc`, no `Mutex`, no `Send + Sync` bound on the policy
and no dynamic dispatch, and q3's own build monomorphises to the same code it runs today. (The
first draft's `Arc<dyn GoalPolicy>` shared between q3 and the wrapper forced `&self` everywhere,
made its own `set_map(&mut self)` uncallable — `Arc::get_mut` fails with two clones — and pushed
every piece of `JevPolicy` state behind a lock.)

A wrapper alone is *not* enough, and the plan records why: q3 honours
`ctx.goal_override` (`brains/core.rs:56`, consumed at `q3/mod.rs:900`) by **always
path-following**, which suppresses its combat FSM. That is the right mechanism for an
*objective*, and the wrong one for an engagement *posture*. Hence both: wrapper + seam.

### The two-clocks rule

The brain keeps time by `self.time += dt` (`q3/mod.rs:880`), fed from serverframe deltas clamped
`0.02..0.3` and *carried* on ticks that bring no new frame. That is deliberately not a wall
clock, and it is what makes brain tests reproducible. **All deadlines, TTLs, dwell timers and
token buckets live in the hub on `time::Instant`.** The brain's own temporal judgement is
identity (the answer's `TicketId` vs the tickets in its ring and the one it last applied) plus,
for posture only, a brain-time freshness bound (`posture_max_age_s`); objectives are held until a
release condition, and their hard expiry is the hub's `ttl_s` on `Instant`.

Consequence for the hub: every ask gets a **`TicketId`** from a **hub-owned** `AtomicU64`,
returned by `submit()`, and the answer carries it back. Never a per-brain counter —
`build_brain` is re-invoked per connection session (`main.rs:972`, inside `bot_task` which starts
at `:877`; the supervisor re-spawns `bot_task` on reconnect), so a per-brain counter restarting at
0 makes stale-response rejection unsound across a reconnect. The ticket is also what makes a
label resolvable: the brain keeps a ring of its last **4** tickets, each holding the
`label → NavGoal` map it offered, and resolves an answer **only against the ticket it answers** —
newest-wins replacement means an in-flight answer usually refers to an *older* candidate list
than the one the brain would build now. An answer whose ticket has left the ring is rejected
(`UnknownTicket`), never resolved against the current list.

### What the model may and may not decide

The model **never names an entity**. Two independent reasons: `combat_drive`'s
`let n = self.enemy?; … .find(…)?` (`q3/mod.rs:648-649`) returns **before writing any movement
axis** for that tick, and the mechanism is reachable when `self.enemy` is `Some` but absent from
`view` (the 2-frame sight grace) — note `run_fsm` (`:894`) and the `nav.set_goal` calls
(`:908`/`:913`) both run *earlier in the same tick*, so the bot is not permanently trapped, but a
zero-intent combat frame is produced; and the flat 24-option measurement proves opaque
identifiers are unanswerable anyway. The application mapping is closed:

| Question | May only… | Must never touch |
|---|---|---|
| `objective` | replace the `self.roam_goal(…)` fallback arm in q3's node→goal match (`q3/mod.rs:904-930`, fallback at `:926`) with a goal drawn from a **code-proved candidate list** | `battle_goal` / `retreat_goal` (`:407`, `:421`), nav node selection, velocity, aim |
| `posture` | **add caution only**: a `disengage` sets `wants_retreat = true` **and** `wants_chase = false` at `:890-891`, and makes the `:506` sneak-past veto skip a *distant, unaware* enemy (declining to acquire — the one, conservative, way posture touches enemy acquisition) | choosing **among** acquired enemies, weapon selection, dodge, and ever *adding* aggression |
| `region` | express a preference among ≤3 code-generated **moves**; the hub's capped assignment (T15) turns preferences into at most one region per bot, resolving to an existing `NavGoal` | raw coordinates, entity numbers, configstring indices |

**Why a disengage must also clear chase.** q3's heuristics are mutually exclusive by
construction — `wants_to_retreat` is `bot_aggression < threshold`, `wants_to_chase` is `>` the
same threshold (`q3char.rs:380`, `:385`) — and `run_fsm` (`q3/mod.rs:534-632`) **loops within one
tick until no transition fires**. Forcing retreat on while the heuristic still says chase creates
a state q3 has never seen: with the enemy visible, Fight →`:585` Retreat →`:605` Chase →`:596`
Fight → … until `MAX_NODESWITCHES = 50` (`:71`) trips, warns, and leaves the bot in **Chase** (50
mod 3), which path-follows `battle_goal` **without calling `combat_drive`** — a "disengaging" bot
running *at* the enemy without firing, every tick. The merge therefore sets both, and a
`debug_assert!(!(wants_retreat && wants_chase))` guards the invariant.

**Why posture is binary.** Under add-caution-only, an `engage` answer can never change anything
and the first draft's `trade` was mapped to nothing; offering them only splits probability mass
between two behaviourally identical options, which the gate then reads as uncertainty. The
question is `disengage` vs `hold` ("leave it to q3").

Everything else stays q3's: FSM, aim, lead, weapon select, dodge, hazard, recovery, and the
stall/wall-block detector. **A decision is released when stuck recovery escalates** (q3's
`self.recovery.evaluate` returning `BackOffThenRepath` or `BoxedIn`, `q3/mod.rs:297-325`, reported
through `GoalPolicy::on_stall`), **on death, on a level change, when a newer answered ticket
supersedes it, and — for objectives — on arrival** (seek-type moves release; hold-type moves start
their `hold_s` timer). The FSM cannot be trapped by a pinned posture: `enemy_alive`
(`q3/mod.rs:350`) means "the entity is in the worldview at all", and the exits to `SeekLtg` are
at `:576`, `:598-599` (BattleChase), `:607` (BattleRetreat) and `:614` (BattleNbg).

**Goal precedence at the `:926` arm**: an applied **objective** > an assigned **region** (T15) >
`roam_goal`. The objective is event-triggered and specific ("get health, you just respawned at
20 hp"); the region is background strategy. Posture is FSM-level and composes with either.

**The invariant to point at in review** (hoodie): *jev never owns the absence of behaviour.*
`Brain::tick` always returns a valid `BrainOutput` computed from the heuristic; the decision
layer may only substitute a goal from a code-validated set while the decision is fresh. The
mechanical failure is not "looks broken" — it is that a frozen brain emits a zero-velocity `usercmd`
while the loop at `main.rs:1180-1186` cheerfully keeps re-acking frames. The server sees a
connected, motionless player and **nothing errors**. You would find it in the scoreboard, after
an A/B you'd misattribute to the model.

### Closed-set prompt rendering (this is the security boundary)

`crates/brain` defines `DecisionState` as **closed data only** — a `FieldKey` enum, bucket enums,
`u32`/`bool`, `QuestionSpec` — and **derives no `Serialize`**. `crates/qbots` renders it with an
exhaustive `match` over `FieldKey`. That buys three things at once:

- it is **impossible** for a wire string to reach the prompt. `player_name(cs, entity_number)` is
  right there at `perception.rs:682`, and player-chosen text is untrusted input.
- `crates/brain`'s dependency list grows by **nothing** (`brain/Cargo.toml` is
  `q2proto/client/world/glam/tracing/clap`, 14 lines, no serde, no tokio, **no
  `[dev-dependencies]`**).
- adding a state field is a **compile error in the renderer** until someone reviews what the
  prompt now says — the review gate we want, because prompt text changes move model behaviour.

**The vector this design itself introduces, and where it is tested:** region/`criteria` text
derived from **BSP item classnames** (`classify_item_classname`; `#w_railgun.md2` style strings
reaching the brain from configstrings). BSP strings are map-author-controlled — lower trust than
our code — and `criteria` text now has *direct influence over the choice*. Rule: a hardcoded
`EntityClass → slug` table (`ArmorHeavy → "heavy_armor"`), never the classname bytes. The
injection test therefore has to drive the **real `Worldview → DecisionState → criteria →
rendered request` pipeline**, which is why it lands in **T10 and T14** (where criteria and legends
are generated) and **not** in T2, whose renderer cannot even see a worldview. Test: a worldview
whose configstrings contain `ignore previous instructions` yields a **byte-identical** rendered
request.

### Gates: when an answer is not a decision

Their own `confidence` field is not usable as our gate (it is `(p1 − 1/n)·n/(n−1)`, a function of
`p1` alone — see *The endpoint*). We compute our statistics in code, **in the hub**, with pure
functions from `crates/brain/src/jev/gate.rs`:

- **margin** `m = p1 − p2` — the gate statistic for every question shape we send (n ≤ 3).
  Provisional thresholds: **`m ≥ 0.50` for n = 2** (p1 ≥ 0.75), **`m ≥ 0.30` for n = 3**.
- `n_eff = exp(−Σ p ln p)` — an **additional** reject only for n ≥ 4 (`n_eff ≤ max(1.5, 0.5·n)`),
  which we never offer but the captured 4-way fixture exercises; logged for every answer.
- `max(p) − 1/n` kept only as a cheap logged "said nothing at all" secondary.

**Why not `n_eff` for small n (the first draft's gate).** `n_eff ≤ 1.5` needs **p1 ≥ 0.860**
binary and **p1 ≥ 0.86–0.90** 3-way (0.860 if the remainder is concentrated, 0.896 if split
evenly), so the odds-ratio ≥ 2 and binary p1 ≥ 0.75 rules it was paired with could never bind.
Live answers (2026-11-07) show what that does:

| State (terse) | Question | p1 | margin | first-draft gate | provisional gate |
|---|---|---|---|---|---|
| hp 18, no armor, 2 enemies near + visible | engage/trade/disengage | 0.441 | 0.065 | reject | reject |
| same | disengage/stay | 0.663 | 0.326 | reject | reject |
| hp 5, blaster only, 3 enemies, "cannot win this fight" | disengage/stay | 0.880 | 0.760 | apply (barely) | apply |

Only a cartoonishly explicit state cleared the first draft's gate. The provisional margins are a
starting point, **not** a result: they are **calibrated once at W2** by `jev-probe calibrate`
(T9) against a **pre-registered**, human-labelled set of ≥10 *obvious* and ≥10 *ambiguous*
scenarios per question — the lowest threshold that applies **≥ 80 %** of the obvious set,
**≤ 10 %** of the ambiguous set, and **0 %** of the flatness set. If no threshold satisfies all
three, the question fails W2 exactly as a sensitivity failure does: it is **deleted, not tuned**.
The calibrated values are recorded in the tracker and **frozen** for W3 onward — "never loosen
thresholds to move a number" applies from that point. They live in config (`jev.gate_margin_binary`,
`jev.gate_margin_multi`) so calibration is not a code change.

**Worked example from our own captured fixture** (the case that killed the simplest gate):
`objective` came back `{hunt 0.3164, hold_area 0.1040, roam 0.4911, seek_item 0.0885}` —
`n_eff = 3.20` of 4, margin **0.175**, odds ratio 1.55. A `max(p) − 1/n` gate (0.241) would have
**applied** it. Both the margin and the n ≥ 4 `n_eff` rule reject it. Commit that response as
`fixtures/jev_objective_flat.json`; it is the most valuable fixture we own (it is 4-way, a shape we
no longer send, which is exactly why it also exercises the n ≥ 4 branch).

Additional rules: **no `none_of_the_above` option** (it parks mass and turns the gate into a third
outcome to calibrate — let the gate produce "no decision ⇒ q3"); ask about **moves, not places**
(`hold_rail_bridge_wait_respawns`, not `region_7`), because actions describe themselves;
**cap options at 3 offered out of 4–6 known**; treat the answer as *not actionable* in the type,
not via a magic float:

```rust
pub enum Decision<T> { Apply(T), NoSignal { reason: NoSignalReason, stats: DistStats },
                       Malformed(MalformedKind), Transport(TransportErr), CircuitOpen }
```

Every arm is one telemetry line and one counter. This enum **is** the observability design — and
it is the **hub's** classification. The error payloads are `Copy` enums (they become the `err=`
field), not `String`s. The hub runs the gate and stores into a bot's slot only what the brain can
act on: a `Copy` `AppliedDecision` holding the ticket, generation, map name id, and one
`Option<…>` per question key (`None` = that question was gated off). `Transport`/`Malformed`/
`CircuitOpen` never reach the brain; they are counted where they happen. The brain's side of the
ledger — whether an applicable answer was actually **applied** — comes back through
`DecisionSource::ack(ticket, ApplyOutcome)`, which is the only source of the `applied` count.

**Denominators — three, and they answer different questions.** Publishing one ratio conflates two
failures:
- `applied / asked` — the **headline**. Everything we wanted to ask and could not is counted
  against us, which is honest. (A ratio whose denominator excludes `Transport`/`Malformed`/
  `CircuitOpen` **passes at 100 % on a hung box that answered four times, confidently**.)
- `gate_reject_rate = NoSignal / admitted` — "when we got an answer, did the state discriminate?"
- `admission_drop_rate = (asked − admitted) / asked` — "were the bucket or the worker capacity the limiter?"
  **This one is never part of the W3 judgement.**

### Event-triggered asking, not timer asking

A timer asks about every state, including the ~90 % where nothing is distinguishable, which
*guarantees* flat distributions and a low applied-rate. A bot that just respawned at 20 hp with
no armor has a state that genuinely discriminates. Triggers: `death/respawn`,
`loadout_gap_became_critical`, `damage_above_threshold`, `area_heat_crossed_threshold`,
`arrived_in_region` — each requiring the condition to **persist ≥2 s** (enemy LOS flickers at
10 Hz; `took_damage` is a per-frame health delta), plus a per-bot **event floor**:
`event_floor_s = 5.0`, one-sided jitter ×[1.0, 1.3], the minimum interval between **any** two
asks from one bot, so 36 bots never arrive at the workers in phase. **An event ask resets that
bot's refresh timer**, so events *replace* refreshes rather than adding to them. Background
refresh at `min_dwell_s` remains, at lower priority. Invalidation means "don't apply; run pure
q3", never "clear the plan".

**Event demand, bounded.** Per bot the ask rate is at most `1/event_floor_s`. Worst case for 36
bots: 36/5 = **7.2 req/s ⇒ 2 520 tok/s at 350 tok**, far over the 1 000 knob — so under an event
storm the bucket **is** the limiter, by design, and its drops are visible as
`admission_drop_rate` split by trigger. Refresh-only demand (2.4 req/s, below) is the floor, not
the ceiling. At the W3 gate's `--count 12`, even the event worst case is 12/5 = 2.4 req/s ⇒
840 tok/s, **under** the knob — which is what lets W3 claim the bucket is not the limiter.
The `trig=` field on every `EVT jev_decision` line is what makes the per-trigger split in *Wave
gates* computable.

### Determinism ⇒ herding (and the metric that detects it)

Byte-identical answers to identical states make the jev arm a **deterministic policy**. N jev
bots in *similar* states make the *identical* decision *simultaneously*, every time. On a DM map
with no coordination that is a herd: they converge on one region, contest each other, and their
failures correlate — the exact opposite of what area control is for, and it would show up as
"jev lost" with nobody knowing why. `/v1/systemone` has **no sampling** to decorrelate it.

Mitigations, all in-plan: decorrelate at the **input** (the offered candidate set already depends
on each bot's position; `Q3CharPreset` persona goes in `state`), jitter the ask phase per bot
(the same technique as `rejoin_jitter`, `main.rs:960`), and a **herd metric in the A/B report:
max concurrent jev bots per region, plus the region-occupancy distribution.**
Decide deliberately whether to enable the response cache (keyed
`hash(STATE_SCHEMA_VERSION ‖ rendered_state)`): caching *guarantees* identical answers for
similar bots, persona-differentiated prompts *defeat* the cache. The cache is **off by default and
is the first thing cut** if W2 slips (see the tracker's cut order) — it is a herding hazard with
no consumer until T15 decides.

### Region / area control needs a fleet carve-out — **T13 is a human decision**

"Is holding the quad area a good decision?" depends on whether 11 other bots already hold it —
which a fleet-blind bot cannot know. `AGENTS.md` is explicit: *"Mutability is local to a bot. No
shared mutable world state across bots — they cannot see each other except through server
frames."* `beacon.rs` only survives that rule because it is **write-only telemetry no bot ever
reads**.

The user chose the **fleet-coach carve-out**: a shared *decider* (an external coach, not a bot's
eyes) sees the fleet's region preferences and hands each bot at most one region under a per-region
cap. **The model does not do the assignment, code does.** A single `choice` question returns one
label, so "one fleet-level question" (the first draft's T15) cannot produce N distinct
assignments. Instead each bot's *own* request carries a `region` question over its ≤3 offered
moves (T14 — fleet-blind, no carve-out needed), and the hub (T15) runs a greedy capacity-capped
assignment over the latest fresh answers: sort `(bot, region, p)` by `p` descending, assign when
the bot is unassigned and the region is under `region_cap`; a bot whose preferred regions are all
full gets none and falls through to objective/roam (never idles). That is the only version that
is **structurally incapable of producing a herd**, and it costs zero extra requests. This is a
**written exception to
AGENTS.md's concurrency model**, so **T13 amends `AGENTS.md` and requires explicit human
approval of the exact wording before T15 may be written.** It also means `--brains jev` is not
strictly comparable to fleet-blind brains; the control arm `JevStatic` (all plumbing, `NullDecider`)
exists to absorb that confound.

Region representation: **hand-authored per-map tables, 4–6 regions**, each a name slug + anchor
node + item classes, resolved to existing `roam_nodes`/`MapItem` origins. **No BSP room
partitioning** (that is nav-graph work with the AGENTS.md "if we say it's unreachable we're wrong"
risk). Legends must **name regions by their contents** (`r4 = dock + mega armor`), never bare
indices — the flat-distribution measurement says an unnameable option set cannot be answered.

### Failure policy: loud at startup, silent per answer

Two distinct fallbacks, deliberately different:

- **Configured-but-unreachable endpoint ⇒ refuse to start.** `jev.require: true` (default when
  the `jev:` block is present in `config.yaml`) with an actionable message:
  `brain 'jev' needs a reachable jev endpoint at cosmo.lan:8710; try: cargo run -p qbots -- jev-probe health`.
  (`cargo run -p tools -- …` cannot work: `crates/tools` has five `[[bin]]`s and no
  `default-run`, and `qbots` is bin-only, so the probe is a `qbots` subcommand.)
  Beacon's "degrade to disabled with one `error!`" is right for telemetry and **wrong for a
  brain**: a fleet that silently runs q3 for 40 minutes while you believe you are measuring a
  model is a **voided experiment**, not a degraded one.
- **An individual answer that is late, flat, unresolvable, or malformed ⇒ silent q3, counted.**

Circuit breaker, **tiered by cause**: `refused` / 4xx / 5xx / model-slug-mismatch open it after N
consecutive; **timeouts get their own, higher threshold** — a busy-but-alive box must not trip a
5-second feature outage on an ordinary burst, and every open carries `err=` naming the cause.
Open ⇒ admit nothing for 5 s, keep serving last-good-or-none, `error!` **once**.
Name-resolution failure of `cosmo.lan` is a **startup refusal** under `require`, never a retry
storm. `.lan` is not mDNS (that is `.local`): it resolves through whatever the host's NSS
provides — on the dev box an `/etc/hosts` line mapping it to `127.0.0.1`, elsewhere typically the
router's DNS (dnsmasq). A name that resolves on one machine because of a local hosts entry is
not a contract for another; T1 records what the fleet host actually uses.

### Budget defaults (arithmetic shown; corrected in review)

| Knob | Default | Why |
|---|---|---|
| `max_input_tokens_per_s` | **1000** | **The primary knob.** A fleet bucket survives 8→36 bots without retuning; a per-bot rate silently rescales GPU load with `--count`. It is **≈26 % of the honest single-stream headroom (≈3.9k tok/s)** — ≈16 % of the hammered 8-concurrent ceiling, and that larger number is not the one to quote. |
| `tokens_per_req_target` | **350** (W5: **420**) | Sibling knob — the *rate* is derived, `fleet_rate = budget ÷ tokens_per_req` = 1000/350 ≈ **2.86 req/s** of capacity. |
| implied refresh demand | **≤ 2.4 req/s** at 36 bots, dwell ≥ 15 s | Refresh-only demand < capacity ⇒ the bucket is **not the limiter for refresh traffic**. 350 tok × 4/min × 36 = 50.4k/min = **840 tok/s** (worst case; the mean dwell of 17.25 s gives 2.09 req/s ⇒ 730 tok/s). Event traffic is bounded separately — see *Event demand, bounded*. |
| `min_dwell_s` | **15.0**, jitter **×[1.0, 1.3]** ⇒ 15.0–19.5 s | **One-sided, deliberately**, and the reason is the *guarantee*, not the mean: symmetric ±30 % jitter leaves the **mean** rate unchanged (2.4 req/s) but lowers the **floor** to 10.5 s, so the bound we can *prove* rises to 36/10.5 = 3.43 req/s ⇒ 1 200 tok/s — over the knob. One-sided keeps the provable worst case at 36/15 = 2.4 req/s and still decorrelates phase (the actual purpose, exactly as `rejoin_jitter` does). |
| `event_floor_s` | **5.0**, jitter **×[1.0, 1.3]** | Minimum interval between any two asks from one bot; an event ask resets the refresh timer. Bounds per-bot rate at 0.2 req/s. |
| `ttl_s` | **24.0** (W5: **30.0**) | Hub-side expiry of a bot's stored decision, on `Instant` from answer time. Must satisfy **`ttl_s ≥ dwell_max + snapshot_ttl + deadline + 1 s`** = 19.5 + 1.5 + 0.35 + 1 = 22.35 ⇒ 24 (a pending snapshot can wait at most `snapshot_ttl` for a worker, then at most `deadline` in flight). With the inequality honoured, a decision that expires before its successor arrives means asking stopped — non-zero `jev_stale` **is** a health/config error. W5 re-price: 26 + 1.5 + 0.35 + 1 = 28.85 ⇒ **30**. |
| objective lifetime | **held until released** | An applied objective steers until arrival (seek) / `hold_s` elapsed (hold), stuck-recovery escalation, death, level change, a newer answered ticket (an answer whose objective was gated off **releases** to roam), or hub `ttl_s` expiry. The first draft's brain-side `max_age_at_apply_s = 8.0` against a 15–19.5 s ask cycle applied each objective for 8 s and then **lapsed to roam for 7–11 s of every cycle** — goal flapping that would have been charged to the model. |
| `posture_max_age_s` | **6.0**, brain-side, counted in brain time from the tick the ticket is first seen | Posture is about *this* fight; asks are event-triggered (damage, contact), so a lapse returns to q3's own heuristic — not a gap, the default. Brain time, not `Instant`: the two-clocks rule. |
| `deadline_ms` | **350**, hard client-side cancel, **wrapping connect** | Single-request p95 is ≤150 ms (loopback). Caveat recorded honestly: the 8-concurrent probe's **mean was ≈346 ms**, so 350 is *not* comfortably above a loaded box — T9's bench must report whether p95 at the real demand rate stays under it, and if it does not, **raise dwell before raising deadline**. |
| `max_concurrent` | **3** workers | In-flight L = λ·W = 2.4 × 0.35 ≈ **0.84** using the *measured* mean (not an optimistic 0.15), so 3 is ≈3.6× headroom, not "a 7× burst". Overflow waits in the bot's newest-wins slot and is **shed** when it passes `snapshot_ttl_ms` (`skipped_no_capacity`), never queued. Serialisation is already visible *at* 8; **measured, not `total_slots`.** |
| worker policy | **refresh ≤ 2 in flight; events may use all 3** | A free worker takes the oldest pending **event** first, else the oldest pending refresh if fewer than 2 refreshes are in flight. Refresh traffic can never starve a respawn ask. |
| `snapshot_ttl_ms` | **1500**, from snapshot creation, checked **when a worker picks it** | A **different knob from `deadline_ms`**: the deadline protects the worker from a hung socket; the TTL stops us spending a scarce worker asking about a dead world. |
| queue depth per bot | **1, newest wins** | A queue lets 4 obsolete snapshots per bot consume the whole budget on the past. **MUST NOT queue >1.** |
| `max_state_tokens` | **512 hard, 250–350 target** | Overflow drops the lowest-priority bucket **and logs which**. **This bounds the state only** — criteria templates + three region legends ride on top, so T10/T14 also assert a **total rendered-request** token budget, not just the state's. |
| questions per request | **all of them, one call** | ~8 ms marginal/question vs ~48 ms per 1 200 state tokens. The region `state` block is therefore **a section of the same request**, included when the region question is included — never a second call. |
| bucket accounting | **reserve `chars/4` at admission, true-up with `usage.input_tokens`, bucket may go negative** | The real count only exists after the response; charging post-hoc means `max_concurrent` × state can admit above 1000 tok/s before the first true-up. The fake-clock test must exercise *this* algorithm, not a different one. |

**W5 re-pricing (mandatory, at T14 — the task that puts the region block into requests):** with
the region block a full ask is ~420 tok, so 36 × 420 ÷ 15 s = **1 008 tok/s — over the knob**.
`min_dwell_s` becomes **20** (36/20 = 1.8 req/s ⇒ 756 tok/s ✓), `ttl_s` becomes **30** (row
above), and `jev-probe bench`'s default rate must be **derived from the live config**, not a
hardcoded 2.8.

**One concurrency model: pull.** The first draft mixed two — bots' asks contending on a
`tokio::sync::Semaphore` with a bounded `permit_wait_ms`, *and* a worker pool pulling a priority
queue. In a pull model there is nothing to wait on: exactly `max_concurrent` worker tasks loop
"pick → admit → send → classify → store", and a worker only picks when it is free. The staleness
bug the semaphore design needed two fixes for (a snapshot that ages for seconds waiting on a
permit and is then answered "on time" about a dead world) cannot occur, because the age check
happens **at pick time** and the send starts immediately after. Fairness is structural: pick
order is **oldest pending first** within each class (events, then refresh), so no bot can be
systematically starved by its spawn position. (The first draft's claim that a FIFO semaphore
starves losers was backwards — FIFO *is* the fair order; the real hazard was unbounded wait.)
`permit_wait_ms` is deleted.

### `state` shape, and keeping it diffable

Named buckets, **integers/bands only, no floats, never raw `Vec3`** (coordinates are ~15 tokens
each, high-entropy, and change every call, faking "the world changed"). Never: entity lists
(cap to `enemy n:2 near:180 vis:yes`), player names, configstrings, the nav graph, arrays of
history, the bot's own name, monotonic counters (`serverframe`, `ticks`, `self.time` — the
model has no clock), or **opaque indices** (nav node numbers, entity numbers, region indices —
the flat 24-option measurement says the model cannot read them, and a node id changes every ask
as the bot moves, faking "the world changed" exactly as a raw `Vec3` would).

```text
# (a) objective + posture — target ~300 tok (T2/T10; no region facts until T14 exists)
self  hp:38(band:lo) armor:12 weapons:2/8 ammo:pistol:12|sg:0|roc:0 low:yes
dmg   recent:74 from:unknown since:3s
enemy n:2 near:180 vis:yes moving:yes
loc   floor:hi exits:2
items quad:seen(300) mega:no ra:no health:seen(120)
map   q2dm1 phase:active gap:heavy_damage

# (b) region block — ~120 tok, a SECTION of the same request when the region question is asked
#     (T14 adds the `Here`/`Alts` FieldKeys — the renderer's exhaustive match makes that a
#      compile error until reviewed). Regions are named by contents, never by index.
here  dock_mega held:28s contested:yes foes:2 loot:quad:no mega:yes
alts  rail_bridge:foes:0 loot:rail  lower_ra:foes:1 loot:ra  — no more than 3
```

Determinism makes serialization order a **behavioural** property: reordering two fields is an
invisible change the A/B will blame on the model. Guards: a hand-written `write!` sequence in
declared field order (**never** `HashMap`, never `serde_json::to_string` of a struct where field
order is implicit luck), `const STATE_SCHEMA_VERSION: u32 = 1;` beside the builder, a test that
serializes one snapshot 100× and asserts byte-equality, and a golden-render test whose failure
message is *"state serialization changed — this changes model behaviour; bump
`STATE_SCHEMA_VERSION` and re-run the live matrix"*.

`st_hash` is computed over the **rendered string** with one `DefaultHasher` — the same hash the
optional cache key uses, so they can never disagree — and is logged (6 hex, greppable) with
`st_tok` on every decision line. The previous rendered field-set for the changed-fields `debug!`
lives **in the hub, per bot** (36 small `Vec`s, trivial) — **never in the brain**; this is only
buildable because `DecisionState` is a closed `Vec<(FieldKey, …)>` and would be impossible with a
struct. Use the server's own `usage.input_tokens` as the token count — feed it into the bucket,
log it, don't estimate.

### Offline CI vs live, and why the boundary sits where it does

**Offline (`cargo test`, always, exact values).** The unit under test is *our logic*, never the
model. Recorded raw response bodies as fixtures — non-negotiably including the **flat 24-option
response**, a `400 invalid_request_error`, a `501 not_supported_error`, a response with a key
missing from `answers`, **an extra key**, `probabilities` that don't sum to 1, an unknown response
`type`, an unknown extra field, `answers: {}`, and a close-with-zero-bytes. Plus: gate tests over
**probability vectors constructed in Rust, in `brain`** covering every `NoSignalReason` arm, and
brain-side apply tests `(AppliedDecision, ticket ring, brain state) → ApplyOutcome` covering every
`BrainReject` arm; the JSON→struct leg is tested in `crates/qbots`, which is where `serde_json`
actually lives; request-render golden tests; and transport framing tests against recorded bytes
(**chunked**, **close-delimited** (no `Content-Length`, not chunked — read to EOF), truncated,
non-200, refused, slow-body timeout, >1 MiB, **model-slug mismatch**) replayed by a ~40-line
`tokio::net::TcpListener` speaking minimal HTTP/1.1 — **no mock library**. Because the server is
deterministic we can commit exact request+response pairs and get byte-exact client tests for free.

**Live (never CI; `cargo run -p qbots -- jev-probe …`).** Asserts **shape and direction, never
values**: response shape; `output_tokens == 0`; `answers` keys **exactly** equal requested keys;
`model == expected_slug`; determinism ×3; **directional sensitivity** (same question, two states
differing in exactly one bucket ⇒ the distribution must move); **anti-correlation** (two
materially different states must **not** return identical distributions — the signature of state
that isn't reaching the model, invisible to every other test); and the flat case ⇒ probe reports
"would be rejected by our gates".

Live must never assert exact probabilities: it false-alarms on a model bump and — worse — it
*passes* on a model that has stopped reading our state.

**The sensitivity probe is what keeps this from becoming an elaborate telemetry generator.** A
question that fails it is **deleted, not tuned**. Run the suite once per model/server upgrade,
like a dependency upgrade.

### Dependency and lockfile gates (stated precisely, because the first draft's were unsatisfiable)

- **Hard gate: `git diff --exit-code crates/brain/Cargo.toml`.** Achievable: the margin, `n_eff`,
  `Arc` are all std (the policy is a type parameter, so `brain` needs no lock at all), and the
  JSON leg lives in qbots (T4).
- **`crates/tools/Cargo.toml` is not touched**: the probe is a `qbots` subcommand (T9).
- **`Cargo.lock`: no new *packages*.** Stated as "no new `name = "…"` entry" rather than
  "byte-unchanged" so the gate stays satisfiable if enabling tokio's `test-util` feature in
  `crates/qbots` (T5) rewrites a dependency list; a feature flag alone should not add a package,
  and **a new `name = "…"` entry is not allowed** — including a new workspace crate, which is why
  the probe is not a new library crate either.
- Confirmed absent from `Cargo.lock` (so unusable): `arc-swap`, `http`, `hyper`, `reqwest`,
  `rand`. Confirmed present and already direct deps of `crates/qbots`: `tokio`
  (`io-util`, `macros`, `net`, `rt-multi-thread`, `signal`, `sync`, `time`), `serde`,
  `serde_json`, `time`.

### Waves and exit gates

The user chose to keep the strategic (area-control) layer in this plan rather than split it, and
both reviewers flagged it as the over-scoped part. The compromise is hard gates: **a wave may not
start until the previous wave's gate is recorded in the tracker.** W3's gate is deliberately
three *numbers* rather than a pass/fail, because a system that declines to act 85 % of the time
and is right when it acts is fine, while one that declines 99 % of the time is a dependency on a
GPU box — and because a single ratio cannot tell those two cases apart from a budget-starved
fleet, which is a config problem pretending to be a science result.

### Baselines and non-regression

- **What the movement gate can and cannot prove**: because jev is inert while
  `ctx.goal_override.is_some()` (`JevBrain::tick` tells its policy before delegating, so neither
  the objective **nor the posture** nor any `submit` runs — posture is consulted *before* q3's
  `goal_override` branch, so q3's own structure would not make it inert) — which is exactly what
  makes the harnesses safe to run — a
  `spawn-to-*` run exercises **the inner `Q3Brain`, never the jev decision layer**. Keep
  `spawn-to-spawn` effective **≥ 13/24** and no `max_speed > 320` as a **composition check**
  (the wrapper didn't break q3) and say so in the report; a jev-caused *movement* regression is
  only detectable as a live DM-batch metric.
- K/D is noise-limited in this repo (a control arm has swung 1.00 → 0.86 → 2.60 on identical
  code), so a jev arm whose applied count is 0 is **indistinguishable from a null result** —
  hence the hard-fail on applied = 0, and the primary metrics are decision-sensitive
  (`time_in_target_region`, latency-to-named-item, region entropy, applied/asked) with frags
  secondary, **N ≥ 5** runs per arm, paired by `(map, servercount)`. Include at least one metric
  we predict shows **no difference**, so an accidentally-identical pair of arms is detectable.
- `--brains` (`main.rs:249`, defaulting + the `RunTester` guard at `:2634-2646`; `BrainKind` at
  `brains/mod.rs:35`) is the A/B surface; the matrix cross-product loop
  (`supervisor.rs:584-610`) **does not dedup**, so do not assume `--brains q3,q3` fields two
  distinguishable groups — verify it or use `q3,jev_static` as the live control.

---

## Step-by-Step Tasks

Grouped into waves. Each task ends with **Verify** (what proves it) and **Commit** (Rule B —
commit *before* marking the tracker row `done`, after `cargo fmt` + `cargo clippy -D warnings` +
`cargo test`). **Live checks are wave-gate items, never part of a task's Verify.** Where a task
lists a **Wave-gate check (live, human)** line, that check belongs to the named wave gate: the
task is `done` (RULES status set — no extra statuses) when its offline Verify passes and is
committed, and the live check is a `_(pending live)_` row in the tracker's Evidence table until a
human runs it at the gate. Offline span: **T6, T2–T8** (T6 is independent and may go first). Then
a human runs T9. Then **T10–T11**, then the W3 gate. T1 and T13 are human-only; **T9 is entirely
live** and is the handoff point, not a task to grind on.

### Wave 0 — live contract gate (no Rust, human)

### T1: Record the contract, the measurements, and settle three open questions

**File**: `context/distilled/jev.md` (new)

**What to do**: Write the `/v1/systemone` request/response contract and the measurement table
from *The endpoint, measured 2026-11-07* into `context/distilled/jev.md`, in the dense house
style (see `context/distilled/eraser.md`), labelled **loopback** (dev box = model host). Then
record, in writing, the three unknowns that change our design:

1. **`confidence` — resolved by the 2026-11-07 review**: `(p1 − 1/n)·n/(n−1)`, exact on three live
   answers (see *The endpoint*). Record the formula and the three vectors; confirm with one 5-option
   request. We never gate on it.
2. **Where does the fleet run, and how does it resolve `cosmo.lan`?** On the dev box it is an
   `/etc/hosts` loopback entry (`127.0.0.1 cosmo cosmo.lan`) — i.e. fleet and model share the host
   and every measured latency is loopback. If the fleet ever runs elsewhere, record that host's
   resolver path (DNS vs hosts file; `.lan` is not mDNS) and re-run `jev-probe bench` from it.
   Decide whether `config.example.yaml` ships `127.0.0.1:8710` with the hostname as a comment.
3. **Is `/v1/systemone` a stable API or a stopgap?** The 501 wording ("use /v1/systemone") reads
   purpose-built. A one-line answer decides whether we pin the slug hard or version-tolerantly.

Any question that cannot be answered today gets a `~~strike~~ UNANSWERED (T1): <what was tried>`
line — never a silent deletion.

**Verify**: `context/distilled/jev.md` exists and contains the request/response shapes verbatim,
the measurement table (labelled loopback), the `confidence` formula, and the other two answers
resolved or struck.
**Expected observation**: a fresh session can implement T2–T4 **from the plan alone if T1 has not
landed** — the Context section already carries the shapes. T1 is parallel with W1, never before it.
**Commit**: `task(T1): record the /v1/systemone contract and measured latency/token envelope`

---

### Wave 1 — the offline seam (everything here is `cargo test`-green with the box switched off)

### T2: Closed-vocabulary `DecisionState` + its builder in brain, exhaustive renderer in qbots

**File**: `crates/brain/src/jev/state.rs` (new), `crates/qbots/src/jev/render.rs` (new)

**What to do**: In `brain`, define the closed vocabulary — `DecisionState`, `FieldKey` (enum),
bucket enums (`HealthBand`, `DistBand`, `Phase`, …), `QuestionSpec` (choice/score/noul +
criteria), and `const STATE_SCHEMA_VERSION: u32 = 1;`. **No `Serialize` derive, no `String`
fields carrying wire data.** Also define the **builder** — `DecisionState::build(&Worldview,
&BrainMap, &SelfSummary) -> DecisionState` — because the `Worldview → state` step is where wire
text could enter, so it must exist to be tested (its injection test is T10's, when `criteria`
generation exists to attack). In `qbots`, write `render_state`/`render_request` as an
**exhaustive `match` over `FieldKey`** using a hand-written `write!` sequence in declared field
order. Enforce `max_state_tokens` by dropping the lowest-priority bucket and returning which one.
Also define the hardcoded `EntityClass → slug` table. **No region facts and no opaque indices**
(node / entity / region numbers) in the state — regions do not exist until T14, and indices are
unreadable to the model (see *`state` shape*).

**Before**: *(no `jev` module exists)*

**After**:
```rust
// crates/brain/src/jev/state.rs  — closed data, no Serialize
pub const STATE_SCHEMA_VERSION: u32 = 1;
pub enum FieldKey { Self_, Dmg, Enemy, Loc, Items, Map }   // T14 adds Here, Alts (+ a version bump)
impl DecisionState { pub fn build(view: &Worldview, map: &BrainMap, me: &SelfSummary) -> Self; … }

// crates/qbots/src/jev/render.rs — the compile-error gate for new fields
pub fn render_state(s: &DecisionState) -> (String, Option<FieldKey> /* dropped */) { … }
```

**Verify**: `cargo test -p qbots` — (a) golden render: a named golden `DecisionState` produces an
exact committed string, byte-for-byte; (b) determinism: the same snapshot rendered 100× is
byte-equal; (c) over-budget: an over-budget state drops the expected bucket and names it;
(d) `git diff --exit-code crates/brain/Cargo.toml` clean.
**Note**: the injection test is **not** here — `render_state` structurally cannot see a worldview.
It lands in T10/T14 where `criteria` and legends are generated.
**Commit**: `task(T2): closed decision-state vocabulary + builder, exhaustive renderer in qbots`

### T3: `Decision` gate type, `DecisionSource` trait, `NullDecider` / `StubDecider`

**File**: `crates/brain/src/jev/decision.rs` (new), `crates/brain/src/jev/source.rs` (new),
`crates/brain/src/jev/gate.rs` (new)

**What to do**: Implement `Decision<T>` (with `Copy` `MalformedKind` / `TransportErr` payloads),
`NoSignalReason`, `DistStats`, `GateThresholds` (provisional defaults from *Gates*:
`margin_binary = 0.50`, `margin_multi = 0.30`, `neff_frac = 0.5` for n ≥ 4) and the **pure** gate
functions `margin(&[f32]) -> f32`, `n_eff(&[f32]) -> f32` and
`gate(&[f32], &GateThresholds) -> Result<usize, NoSignalReason>`. The hub (T5) calls them; the
brain never does. Define the brain-facing types — `TicketId(u64)`, a `Copy` `AppliedDecision`
(ticket, generation, map id, one `Option<…>` per question key) and
`ApplyOutcome { Applied, Rejected(BrainReject) }` with
`BrainReject { UnknownTicket, WrongMap, OffTicketLabel, Expired, Inert, Superseded }` — and the
seam trait with **no `async fn`** (so `brain` stays tokio-free and `Brain: Send` + sync `tick` are
untouched). Each bot gets its *own* source handle, so no bot key is passed:

```rust
pub trait DecisionSource: Send + Sync {
    /// Newest gated decision for this bot. `Copy` — no allocation on the tick path.
    fn latest(&self) -> Option<AppliedDecision>;
    /// Newest-wins: replaces this bot's pending snapshot. Returns the hub-issued ticket.
    fn submit(&self, s: DecisionState, trig: Trigger) -> TicketId;
    /// The brain's verdict on a decision `latest()` returned — the only source of `applied`.
    fn ack(&self, ticket: TicketId, outcome: ApplyOutcome);
}
```

`NullDecider` (`latest` always `None`, `submit` returns a dummy id) is what `JevStatic` and every
offline test use. `StubDecider` replays a scripted list for brain tests and records `ack`s.
Validation (applied by the hub, specified here): `probabilities` sum ≈ 1 within 1e-3, `choice` ∈
the labels we sent, unknown response `type` ⇒ `Malformed` (**never** a silent default). `score` is
converted to a discrete level at parse time (`argmax(probabilities)`) and the float is discarded
except as a within-level tie-break.

**Before**: *(no gate logic exists)*

**Verify**: `cargo test -p brain -- jev_gate` — a table of constructed probability vectors →
`Ok(index) | Err(NoSignalReason)` covering **every** `NoSignalReason` arm, built **in Rust**.
`crates/brain` has **no `[dev-dependencies]` and no serde** (verified: 14-line `Cargo.toml`, zero
`serde_json` usages), so **no brain test may read a JSON fixture** — the JSON→`Answer` leg is
T4's, in `crates/qbots/tests/jev_net.rs`, where `serde_json` is already a direct dependency.
Required rows: the captured flat 4-way vector `{0.3164, 0.1040, 0.4911, 0.0885}` ⇒
`Flat { margin: 0.175, n_eff: 3.20 }`; the live binary `{0.880, 0.120}` ⇒ `Ok(0)`; the live binary
`{0.663, 0.337}` ⇒ `Flat` (margin 0.326 < 0.50); the live 3-way `{0.182, 0.377, 0.441}` ⇒ `Flat`;
and a **regression row for the first draft's bug**: binary `{0.80, 0.20}` (n_eff 1.65) ⇒ `Ok(0)` —
an `n_eff ≤ 1.5` gate would have rejected it.
**Commit**: `task(T3): margin gate algebra and the ticketed DecisionSource seam`

### T4: Hand-rolled HTTP/1.1 POST + `/v1/systemone` wire types (zero new packages)

**File**: `crates/qbots/src/jev/net.rs` (new), `crates/qbots/src/jev/wire.rs` (new),
`crates/qbots/tests/jev_net.rs` (new), `crates/qbots/tests/fixtures/*.json` (new)

**What to do**: Types in `wire.rs` mirror the contract; `#[serde(default)]` on structs,
`Option<T>` for advisory fields (`model`, `usage`, `legend`), **required** for `type`/`choice`/
`probabilities`; **no `deny_unknown_fields`** (the server is independently versioned);
`#[serde(tag = "type", rename_all = "lowercase")]` with unrecognised variants mapped to
`Malformed`. In `net.rs`, one `POST` over `tokio::net::TcpStream`: `Content-Length`,
**`Connection: close` always** (one connection per request — deletes the whole keep-alive-pool
bug class for ~zero cost on LAN), handle `Content-Length`, `Transfer-Encoding: chunked`, **and a
close-delimited body** (neither header — read to EOF, which `Connection: close` makes legal),
`write_all` under backpressure, cap the response at **1 MiB** → `Malformed`, and wrap
**connect + write + read** in one `tokio::time::timeout(deadline)`. Validate the response:
`answers` keys **exactly equal** requested keys (reject the *whole* response on a missing **or
extra** key — partial application is where silent corruption lives), and assert `model` against
config, failing closed to the breaker on mismatch.

**Before**: *(no HTTP exists anywhere in the tree — `Cargo.lock` has zero http/tls crates)*

**Verify**: `cargo test -p qbots -- jev` against a ~40-line fixture-replaying
`tokio::net::TcpListener`: chunked body, **close-delimited body**, truncated body, non-200, `400`,
`501`, malformed JSON, `answers: {}`, a **missing** answer key, an **extra** answer key,
probabilities not summing to 1, an **unknown response `type`**, a **model-slug mismatch**,
connect-refused, a **close-with-zero-bytes** response, and a slow-body timeout. Each asserts the
specific `Decision` variant (fallback class + reason), not "didn't panic" — a panicking test
already fails, so that phrase proves nothing. **These tests run on the real clock — never
`start_paused`**: a paused runtime auto-advances time whenever every task is idle, and a task
blocked on real socket I/O counts as idle, so the slow-body timeout would fire instantly and the
test would pass for the wrong reason. Keep the slow-body delay small (≤ 200 ms past a ~100 ms test
deadline). **No new `name = "…"` entry in `Cargo.lock`.**
**Commit**: `task(T4): hand-rolled HTTP/1.1 systemone client with framing and timeout coverage`

### T5: The hub — slots, transport seam, worker pool, fleet bucket, generation, breaker

**File**: `crates/qbots/src/jev/hub.rs` (new), `crates/qbots/src/jev/transport.rs` (new),
`crates/qbots/src/jev/mod.rs` (new), `crates/qbots/Cargo.toml` (add tokio `test-util` to
dev-deps/features)

**What to do**: One **process-wide** hub, mirroring `start_beacon` (`supervisor.rs:222-241`) for
lifecycle and shutdown.

- **Decision slot**: `struct DecisionSlot { inner: Mutex<Option<(AppliedDecision, Instant)>> }` per
  bot index — **a `Mutex` of a `Copy` value, not of an `Arc`**. The hub has already classified and
  gated the answer; only an `AppliedDecision` is ever stored (`Transport`/`Malformed`/`CircuitOpen`
  are counted, not stored). The sync `Brain::tick` reader takes a small copy under a lock held for
  a struct move, and gets `None` once the stored `Instant` is older than `ttl_s`; the writer
  assigns. No allocation on either side, one writer + one reader per bot, contention nil — **no
  fast-path id gate** (an external `AtomicU64` checked before the lock is two sync points that
  can disagree; add only if a profile asks). Slot lives in `crates/qbots` and is surfaced **only**
  through the bot's `Arc<dyn DecisionSource>` handle (`latest` / `submit` / `ack`), so `brain`
  never learns `std::sync` exists.
  *(Review conflict resolved: `tokio::sync::watch` also works — `sync` is already an enabled
  feature and its newest-wins semantics fit — but it pulls tokio's API into the module that owns
  the slot and is awkward to read from a non-tokio test. `arc-swap` is **not in `Cargo.lock`** and
  is banned by the no-new-packages gate.)*
- **Transport seam**: `trait DecisionTransport` (`net.rs`'s client is its production impl, a
  scripted one its test impl). **Without this the fake-clock hub tests cannot drive a worker at
  all**, and it is the reason the seam is listed here and not "later".
- Inbound: a newest-wins **pending** slot per bot (`Mutex<Option<Pending>>` + one shared
  `tokio::sync::Notify`; the bot side is one short lock + `notify_one`, **never a queue**).
  `submit` stamps a `TicketId` from a hub-owned `AtomicU64` and the submit `Instant`.
- **Workers — one pull model, no semaphore**: exactly `max_concurrent` tasks, each looping
  pick → admit → send → classify → gate → store. *Pick* scans the 36 pending slots (trivial):
  drop-and-count any pending older than `snapshot_ttl_ms` (`skipped_no_capacity`), then take the
  **oldest pending event**, else the **oldest pending refresh if fewer than 2 refreshes are in
  flight**; nothing pickable ⇒ await the `Notify`. There is no permit wait, so a picked snapshot is
  at most `snapshot_ttl_ms` old when its send starts.
- Admission: the `max_input_tokens_per_s` bucket, **reserve `chars/4` at admission, true-up with
  the real `usage.input_tokens`, bucket may go negative**; a refused admission is dropped and
  counted (`jev_budget`) — the bot's next submit replaces it anyway.
- Per-bot ask pacing (enforced at `submit`): `event_floor_s` between any two asks and refresh at
  `min_dwell_s`, both with **one-sided ×[1.0,1.3]** jitter; an event resets the refresh timer.
- A hub-owned `AtomicU64` `generation` (bumped per bot by the level-change hook, T8; an answer
  carrying an older generation is discarded before it is stored); a **tiered** circuit breaker
  (`refused`/4xx/5xx/model-mismatch after N; timeouts on their own higher threshold; open ⇒ admit
  nothing 5 s, `error!` once); optional response cache keyed
  `hash(STATE_SCHEMA_VERSION ‖ rendered_state)`, **off by default**.

**Before**: *(none — this is the only place that ever touches the socket)*

**Verify**: `cargo test -p qbots -- jev_hub` with `tokio::time::pause`/`start_paused` (needs the
`test-util` feature) **and the scripted transport**: the bucket limits to config using the
reserve/true-up algorithm above (not post-hoc charging); a bot's second submit **replaces** its
pending snapshot and returns a new ticket (assert one in flight per bot); a pending snapshot older
than `snapshot_ttl_ms` is dropped **at pick time**, counted `skipped_no_capacity`, and never sent;
with all workers busy, the **oldest** pending snapshot is served first when one frees (no bot
starves); an event is picked before any refresh, and refresh never occupies more than 2 workers;
`submit` inside `event_floor_s` of the previous ask is refused, and an event resets the refresh
timer; the breaker opens for `refused`/5xx/model-mismatch and **does not** open on isolated
timeouts; a `generation` bump discards every in-flight answer; a gated-off answer stores a
decision whose question field is `None` (it supersedes, it does not vanish); a `Transport` result
stores nothing; a stored decision older than `ttl_s` reads as `None`; a decision slot read from a
non-tokio thread returns the last write; `ack(Applied)` increments `applied`.
**Overrun note**: seven interacting mechanisms (slots, pull workers, bucket, pacing, generation,
breaker, cache), each with a test — this is the plan's schedule risk. Cut the response cache
from this task first if it slips.
**Commit**: `task(T5): process-wide jev hub — pull workers, bucket, tickets, pacing, breaker`

**W1 EXIT GATE** *(recorded in the tracker before T7 starts — T6 does not depend on it)*:
`cargo build`/`clippy -D warnings`/`test` green with **no new packages in `Cargo.lock`** and
`crates/brain/Cargo.toml` byte-unchanged, every test above passing with no network available.

---

### Wave 2 — the q3 seam, the brain kind, and the probe tool

### T6: `GoalPolicy` seam inside q3 — behaviour-preserving

**File**: `crates/brain/src/brains/q3/policy.rs` (new), `crates/brain/src/brains/q3/mod.rs`
(edit: struct + `impl` blocks made generic, constructor `:164`, `select_enemy` `:448` / the `:506`
veto, `:886-891`, the `:926` arm, the recovery match `:297-325`),
`crates/brain/src/brains/q3/testdata/parity_200.txt` (new — the golden)

**What to do — two commits, in this order.**

**Step 1 — record the golden from the *pre-seam* code** (its own commit, before any refactor).
Add `q3_parity_golden` inside `q3/mod.rs`'s own `#[cfg(test)] mod tests` (where `view_enemy`
`:1064`, `view_enemy_at` `:1147` and `crate::nav_mode::StubNav` are already in scope). It drives
`Q3Brain::new(Q3Character::sarge())` with a `StubNav` for 200 ticks over a scripted sequence that
visits **every** node and both posture sites: roam in `SeekLtg` (set `roam_nodes` directly — the
private field is visible to the test module — so the `:926` arm and its 50-tick cycle run), a
visible enemy (Fight), low health (Retreat), the enemy dropping out of LOS (Chase), a distant
unaware enemy (the `:506` veto), an item nearby (Nbg), and a recovery escalation. Each tick
appends one line — `format!("{:?} | {:?} | {}", out, nav.last_goal, b.status())`; Rust's `{:?}`
for `f32` is the shortest round-tripping form, so string equality is bit equality — and the test
compares the whole trace with `include_str!("testdata/parity_200.txt")`, reporting the first
differing tick. With `Q3_PARITY_RECORD=1` it writes the file instead. Commit the test + file.

*Why a golden and not a two-brain comparison:* the first draft compared `Q3Brain::new()` against
`Q3Brain::new_with_policy(NullPolicy)` — but `new()` **delegates** to `new_with_policy(NullPolicy)`,
so that test compares the refactored code with itself and passes even if the refactor changed
behaviour. Only a trace recorded **before** the change can catch a change.

**Step 2 — the seam.** q3 becomes generic over its policy, the `HashMap::new` pattern:

```rust
// crates/brain/src/brains/q3/policy.rs
pub trait GoalPolicy {
    /// Replacement goal for the roam-fallback arm (`:926`) only. `None` ⇒ `roam_goal`.
    fn objective(&mut self, _q: &GoalQuery) -> Option<NavGoal> { None }
    /// Called ONCE per tick, before `select_enemy`, so `:506` and `:890` see one answer.
    fn posture(&mut self, _q: &PostureQuery) -> Posture { Posture::default() }
    /// Stuck recovery escalated (`BackOffThenRepath` / `BoxedIn`): release any pinned goal.
    fn on_stall(&mut self) {}
}
pub struct NullPolicy;
impl GoalPolicy for NullPolicy {}                 // every method default => today's behaviour
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Posture { pub disengage: bool }        // the only bias: add caution
pub struct GoalQuery<'a>   { pub view: &'a Worldview, pub ticks: u32, pub pos: Vec3 }
pub struct PostureQuery<'a> { pub view: &'a Worldview, pub ch: &'a Q3Character }

// crates/brain/src/brains/q3/mod.rs
pub struct Q3Brain<P: GoalPolicy = NullPolicy> { /* …existing fields… */ policy: P }
impl Q3Brain<NullPolicy> {
    pub fn new(ch: Q3Character) -> Self { Self::new_with_policy(ch, NullPolicy) }
}
impl<P: GoalPolicy> Q3Brain<P> {
    pub fn new_with_policy(ch: Q3Character, policy: P) -> Self { /* today's `new` body */ }
    pub fn policy_mut(&mut self) -> &mut P { &mut self.policy }
    /* …every existing private fn, unchanged except the three sites below… */
}
impl<P: GoalPolicy + Send> Brain for Q3Brain<P> { /* unchanged body + the sites below */ }
```

Existing `Q3Brain::new(ch)` calls (`brains/mod.rs:97`, 12 in q3's tests) and the `&mut Q3Brain`
signature in q3's `drive` test helper keep compiling **unedited**: in type position `Q3Brain`
means `Q3Brain<NullPolicy>`, and `new` exists only on that instantiation. Default methods name
their parameters `_q` — an unused `q` is a warning, and Rule A runs `clippy -D warnings`.

**The posture merge — add caution only, and a disengage also clears chase.**

**Before** (`crates/brain/src/brains/q3/mod.rs:886-891`, plus the `:506` veto inside
`select_enemy` and the `:926` fallback arm):
```rust
        self.select_enemy(view, cm, took_damage);
        …
        let wants_retreat = crate::q3char::wants_to_retreat(view, &self.ch, hdelta);
        let wants_chase = crate::q3char::wants_to_chase(view, &self.ch, hdelta);
        …
            if crate::q3char::wants_to_retreat(view, &self.ch, hdelta) {   // :506
                continue;
            }
        …
                            _ => self.roam_goal(view, ticks, pos),          // :926
```

**After**:
```rust
        // Once per tick, before enemy selection, so both posture sites see the same answer.
        let post = self.policy.posture(&PostureQuery { view, ch: &self.ch });
        self.select_enemy(view, cm, took_damage, post);
        …
        let h_retreat = crate::q3char::wants_to_retreat(view, &self.ch, hdelta);
        let h_chase = crate::q3char::wants_to_chase(view, &self.ch, hdelta);
        let wants_retreat = h_retreat || post.disengage; // add caution only
        let wants_chase = h_chase && !post.disengage;    // a disengage ALSO clears chase
        debug_assert!(!(wants_retreat && wants_chase));  // q3char: `<` vs `>` of one threshold
        …
            if post.disengage || crate::q3char::wants_to_retreat(view, &self.ch, hdelta) { // :506
                continue;
            }
        …
                            _ => match self.policy.objective(&GoalQuery { view, ticks, pos }) {
                                Some(g) => g,
                                None => self.roam_goal(view, ticks, pos),
                            },
        …
        // in `locomote`, after `let rec = …` (:297):
        if matches!(rec, RecoveryAction::BackOffThenRepath | RecoveryAction::BoxedIn) {
            self.policy.on_stall();
        }
```
`NullPolicy` ⇒ `disengage == false` ⇒ `wants_retreat == h_retreat`, `wants_chase == h_chase`,
the `:506` condition unchanged, `objective` ⇒ `roam_goal`, `on_stall` a no-op ⇒ **the golden
must not change by a single byte.** (Why the merge must clear chase: see *What the model may and
may not decide* — retreat + chase together loops `run_fsm` into a non-firing Chase.)
Borrows: `self.policy.posture(&PostureQuery { ch: &self.ch, .. })` and
`self.policy.objective(..)` then `self.roam_goal(..)` are disjoint-field / sequential borrows.

**Verify**: (a) all **12** existing `#[test]`s in `q3/mod.rs` pass **unedited**; (b) the Step-1
golden passes **unedited** after Step 2 (`git diff` of `testdata/parity_200.txt` between the two
commits is empty); (c) a `Posture { disengage: true }` test policy with a visible enemy and a
**high-aggression** state (full health + armor + rocket launcher, so `h_chase == true`) leaves the
node at `BattleRetreat` after one tick — under the first draft's merge this lands in
`BattleChase` after 50 switches — and never trips `MAX_NODESWITCHES`; (d) a test policy whose
`objective` returns `Some(g)` makes `StubNav::last_goal == Some(g)` in `SeekLtg` and has no
effect in any battle node. **No extraction to `crates/brain/tests/common`** — unnecessary risk on
crate-private `Worldview` construction; `crates/brain/tests/q3_brain.rs` already covers the
external-API leg.
**Wave-gate check (live, human — W2)**: `q3,jev_static` after T7 is the live no-op control — **do
not use `--brains q3,q3` as the inertness proof** unless you have shown `matrix_specs`
(`supervisor.rs:589-610`, no dedup) fields two *distinguishable* groups, since identical arms
collide on `group_tag`/bot names (Plan 54 name-coding, `FleetStats` prefix grouping).
**Commits**: `task(T6): record a 200-tick pre-seam q3 golden trace`, then
`task(T6): make q3 generic over GoalPolicy (behaviour-preserving, golden unchanged)`

### T7: `JevBrain`, `BrainKind::{Jev,JevStatic}`, the `jev:` config block

**File**: `crates/brain/src/brains/jev/{mod,policy}.rs` (new), `crates/brain/src/brains/mod.rs`
(`BrainKind` `:35`, `brain_tag` `:58`, `build_brain` `:76-83` + a new sibling constructor),
`crates/qbots/src/config.rs` (the `BeaconCfg` optional-block pattern at `:36-63`, the
`fleet.brain` parse `brain_kind()` at `:132-143`), **`config.example.yaml` (qbots root — there is
no `crates/qbots/src/config.example.yaml`)**, `crates/qbots/src/roster.rs` (brain-token
validation `~:109-123`), `crates/qbots/src/supervisor.rs` (`brain_code` `:506-515`)

**What to do**: `JevBrain { inner: Q3Brain<JevPolicy> }` — a ~120-line composition, **no copy of
q3**. It reaches its policy through `inner.policy_mut()` (T6), so nothing is shared and nothing is
locked on the brain side. `JevPolicy` owns the bot's `Arc<dyn DecisionSource>` handle, its ticket
ring (last 4 tickets, each with the `label → NavGoal` map it offered), the currently applied
objective/posture and their release state, and its own copies of `roam_nodes`,
`nav_graph` (`Arc`) and `items`. **`JevBrain::set_map` intercepts the map**: it clones those three
fields into the policy, then forwards `map` to `inner.set_map` — necessary because
`Q3Brain::set_map` **discards `items` by design** (`:841`) and candidate resolution needs them,
and `BrainMap` derives nothing (no `Clone`), so the fields are cloned one by one.
`JevBrain::tick` sets `policy.inert = ctx.goal_override.is_some()` **before** delegating — the
policy's `posture` is consulted ahead of q3's own `goal_override` branch, so without this flag
posture (and asks) would still run in the `spawn-to-*` harnesses. `JevBrain::on_death` /
`on_map_change` (T8) forward to `inner` and release the policy's applied state.
`JevBrain::status()` **delegates verbatim to `inner.status()`**
— no `"jev:"` prefix, or T12's `assert status() == "retreat"` (`Q3Node::label`, `q3/mod.rs:63`)
and the `build_brain → status()` pattern in `crates/brain/tests/q3_brain.rs` both break.
`JevStatic` = the same construction with `NullDecider`, as the primary A/B control.

**`build_brain` cannot receive a `DecisionSource`** as it stands (`brains/mod.rs:76-83` takes
`kind/skill/cfg/char/persona/xonchar`). Add a sibling
`build_brain_with_decider(kind, …, Option<Arc<dyn DecisionSource>>)` used by the **two** production
call sites (`main.rs:972`, `scenario.rs:544`); leave `build_brain`'s signature and its **7** other
call sites (`brains/mod.rs` tests ×3, `crates/brain/tests/q3_brain.rs` ×4) untouched.

The `jev:` block in `config.yaml` (the config is **YAML**, `serde_yaml`; earlier drafts' `[jev]`
notation was TOML-style shorthand): `enabled`, `endpoint`, `model`, `require` (**default `true`
when the block is present**), `max_input_tokens_per_s`, `tokens_per_req_target`,
`max_concurrent`, `deadline_ms`, `snapshot_ttl_ms`, `min_dwell_s`, `event_floor_s`,
`dwell_jitter_max`, `ttl_s`, `posture_max_age_s`, `max_state_tokens`, `gate_margin_binary`,
`gate_margin_multi`, `region_cap` (T15), `debug`, `debug_sample`, `cache`. `#[serde(default)]` +
`Default` so an existing `config.yaml` behaves exactly as before. **Config-only: no
`--jev-endpoint` flag** (two sources of truth for an endpoint is a support ticket);
`JEV_ENDPOINT` env is honoured **inside `jev-probe` only**, and T9's probe reads the same
`--config` file as the fleet (the CLI's existing global flag, `main.rs:132-133`).
`brain_code` (`supervisor.rs:506-515`) feeds `group_tag` and Q2's **15-char `netname`** budget —
use short codes `jev` / `jvs`, not `jev_static`.

Startup: probe `health`, assert the returned slug against `model`, and on failure with
`require: true` **refuse to start** with the actionable message in *Failure policy*.

**Before** (`crates/brain/src/brains/mod.rs:35-55`):
```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum BrainKind {
    #[value(name = "mai", alias = "main")]      Main,
    #[value(name = "sen", alias = "sentry")]    Sentry,
    #[value(name = "run", alias = "runtester")] RunTester,
    #[value(name = "q3", alias = "quake3")]     Quake3,
    #[value(name = "zb2")]                      Zb2,
    #[value(name = "xon", alias = "xonotic")]   Xon,
}
```
**After**: the house rule (doc comment on `BrainKind`) is that a variant's canonical CLI token
**is** its short code, with the long name as an alias:
```rust
    /// jev — Quake3 core + hosted /v1/systemone decisions (Plan 72).
    #[value(name = "jev")]                         Jev,
    /// jev_static — jev with all plumbing and a NullDecider: the primary A/B control.
    #[value(name = "jvs", alias = "jev_static")]   JevStatic,
```
plus `brain_tag` (`"jev"`, `"jev_static"`), `brain_code` (`"jev"`, `"jvs"`), and
`build_brain_with_decider`.

**Verify**: `cargo test -p qbots` — config absent ⇒ `enabled: false`, no task spawned, no socket;
`require: true` + unreachable endpoint ⇒ startup error containing
`cargo run -p qbots -- jev-probe health` **(drive it against a closed port `127.0.0.1:1` — the box
must not be needed for this test)**; `require: false` ⇒ one `error!` then behaves as `JevStatic`;
`<BrainKind as clap::ValueEnum>::from_str("jev", false)`, `"jvs"` and `"jev_static"` all parse,
and the roster brain token accepts them; `--brains q3,jev_static,jev` parses; `brain_code` values
are ≤ 15 chars as `netname`s. `cargo test -p brain -- jev_brain`: `set_map` leaves the policy
holding the map's `items` while q3's own copy stays discarded; with `goal_override = Some` the
policy is inert — `StubDecider` sees **no `submit`** and posture stays default even with a
scripted `disengage` waiting in `latest()`.
**Commit**: `task(T7): jev + jev_static brain kinds, jev: config block, startup requirement gate`

### T8: Hub lifecycle in the bot loop + invalidation hooks

**File**: `crates/qbots/src/main.rs` (`bot_task` at `:877`, `build_brain` at `:972`, the `select!`
at `:1081`, `brain.tick` at `:1714`, `on_death` at `:1398`/`:1592`, the servercount branch at
`:1365-1412`), `crates/qbots/src/supervisor.rs` (**both** `start_beacon` call sites — `:323` and
**`:682`**, inside `FleetShared`, the fleet/competition path), `crates/brain/src/brains/core.rs`
(`trait Brain` `:108-126`)

**What to do**: Start one hub per process **at both shared-fleet spawn points** — wiring only
`:323` leaves `competition` with **no hub**, so T11's own A/B would silently run jev with no
decider and report zero applied forever. Give each bot an index-keyed slot pair and hand the
brain its own `Arc<dyn DecisionSource>` handle. **No `.await` and no HTTP in the ticker arm**
(`main.rs:946` is `time::interval(100ms)` with **no `MissedTickBehavior` call anywhere**, so
tokio's `Burst` default is live; `main.rs:1180-1186` is Plan 57's ack-on-arrival and the keepalive
gap is 90 ms — a stall there perturbs the one number the repo most carefully engineered, for all
36 bots). Bots only ever `submit(snapshot)` (one short lock + notify), `latest()` (one lock +
copy) and `ack()` (one counter). Add a **new hook** rather than overloading `on_death`, which
`main.rs:1398` fires on servercount change *and* `main.rs:1592` fires on every respawn:

**Before** (`crates/brain/src/brains/core.rs:108-126`): `trait Brain: Send` has `set_map`,
`tick`, `on_kill`, `on_death`, `heatmap_weights` and `status` — no level-change hook.
**After**:
```rust
    /// A level change (not a respawn): the map identity may or may not have changed.
    fn on_map_change(&mut self, _map: &str) {}
```
called at `main.rs:1398` **in addition to** `on_death`, inside the servercount branch — check the
branch's existing ordering still assigns `map_servercount = servercount` (`:1412`) *after* the
hooks, and also bumps the bot's hub `generation` there. Invalidate by `(map_name, region_fp)` so
a same-map restart (identical BSP, graph reloaded from `nav_cache`) keeps a still-valid plan.
Never compare servercounts with `<`/`>` (`beacon.rs:107-110` explains why). **Triggers are
flags, not submits**: `on_death` (`:1592`), `on_map_change` and the pickup / stall prints only
*mark* a pending trigger on the policy; the brain builds the `DecisionState` and calls `submit`
on its **next tick**, because only the brain holds the `Worldview` and the candidate list the
ticket must carry. **jev is inert while `ctx.goal_override.is_some()`** (`core.rs:56`; T7's flag)
so the `spawn-to-*` harnesses stay runnable — see *Baselines* for what that gate therefore cannot
prove.

**Verify**: `cargo test -p qbots -- jev_wiring` — an answer carrying a previous `generation` is
discarded by the hub and never stored; the servercount branch bumps the generation and still
assigns `map_servercount` last. `cargo test -p brain -- jev_apply` (the apply decision lives in
`JevPolicy`, so its tests live in `brain`, with `StubDecider`): a decision whose map id differs ⇒
`ack(Rejected(WrongMap))`, never applied; a label not in its ticket's map ⇒ `OffTicketLabel`; a
ticket no longer in the ring ⇒ `UnknownTicket`; a valid one ⇒ `ack(Applied)` exactly once;
`on_death` marks a trigger and the **next** tick submits; `goal_override = Some` ⇒ nothing applied
or submitted (**a guard test — `JevStatic` is inert by construction, so it proves the guard and
nothing else**).
**Wave-gate check (live, human — W2)**: `connect-one` with `jev:` enabled against the box, and the
same with `require: true` and the box unreachable ⇒ **refuses to start**.
**Commit**: `task(T8): wire the jev hub at both fleet spawn points, add on_map_change, gate on goal_override`

### T9: `jev-probe` — the handoff point, entirely live

**File**: `crates/qbots/src/jev/probe.rs` (new), `crates/qbots/src/main.rs` (one `Cmd::JevProbe`
subcommand arm beside `ConnectOne`/`SpawnToSpawn`, `main.rs:139` onward). **Not `crates/tools`**:
`qbots` has no `lib.rs`, so a `tools` binary cannot import `jev::net`/`wire`/`render`/`config`
and would need a second HTTP client and config parser; and `cargo run -p tools -- …` does not
even select a binary (five `[[bin]]`s, no `default-run`). As a `qbots` subcommand it reuses the
production client byte-for-byte, the global `--config` flag, and `clap`/`serde_json` that `qbots`
already depends on — `crates/tools/Cargo.toml` and `Cargo.lock` are untouched.

**What to do**: `cargo run -p qbots -- jev-probe <sub>`. Subcommands: `health` (health + model
slug + slot count + the **501 capability fingerprint** on `/v1/chat/completions`, sent with a
**well-formed chat body** — `{}` gets a 400); `ask --state @fixtures/x.txt` (typed answers +
probabilities + our computed margin / `n_eff` / gate verdict + `usage.input_tokens` + latency);
`bench --bots N --rate R --concurrent C` (**rate defaults derived from the live `jev:` config, not a
hardcoded 2.8**; prints p50/p95/p99, aggregate input tok/s against **both** the ≈3.9k single-stream
and ≈6.2k hammered figures, and whether p95 stays under `deadline_ms`); `calibrate` (runs the
**pre-registered**, human-labelled *obvious* / *ambiguous* scenario sets per question — committed
under `context/fixtures/jev/calibration/` **before** the first run — plus the flatness set, prints
each answer's margin, and reports the lowest threshold per question shape that applies ≥ 80 % of
*obvious*, ≤ 10 % of *ambiguous* and 0 % of *flat*, or "no threshold satisfies all three");
`sensitivity` (perturbed
pairs differing in exactly one bucket ⇒ the distribution must move); `anticorrelation` (two
materially different states must **not** be byte-identical); `flatness` (deliberately meaningless
legends ⇒ assert our gates reject); `matrix --live` (~40 named scenarios) **plus `matrix
--replay`, which re-asks historical prompts and diffs "what it said then" vs now** — the cheapest
detector of a server restart changing the readout, which is risk 6. Flags: the global `--config
<path>` (the fleet's exact `jev:` block; `JEV_ENDPOINT` env override honoured **here only**) and
`--json` (this output is pasted into a markdown table by hand). Every subcommand **harvests** its
raw request+response into `context/fixtures/jev/`.

**Verify (offline, the agent)**: `cargo test -p qbots -- jev_probe` — argv parsing for every
subcommand; `calibrate`'s threshold search over a canned list of margins returns the expected
threshold and the "no threshold" verdict; `bench`'s default rate equals the config-derived rate.
**Wave-gate check (live, human — W2)**: `jev-probe health` prints the slug and the 501;
`calibrate` produces a threshold per question shape (or deletes the question); `sensitivity`
moves **both** perturbed pairs (full-health+mega vs 20 hp + no armor; near-item vs far-item);
`anticorrelation` passes; `flatness` reports rejection at the calibrated thresholds; `bench`
prints p50/p95/p99, tok/s against both figures, and the p95-vs-`deadline_ms` verdict. Record every
number in the tracker's Evidence table.
**Commit**: `task(T9): qbots jev-probe — health/ask/bench/calibrate/sensitivity/anticorrelation/flatness/matrix`

**W2 EXIT GATE** *(human-run; recorded before T10)*: all six live checks pass and are pasted in
the tracker, and the calibrated `gate_margin_binary` / `gate_margin_multi` are written into
`config.example.yaml` and **frozen** from here on. Additionally: with **`require: false`** and the
endpoint unreachable (under the default `require: true` the fleet refuses to start, which is the
T7 test, not this one), `--brains q3,jev_static,jev` — jev plays **identically to jev_static** —
and the `q3`-vs-`jev_static` K/D gap is within the repo's noise floor. **If `sensitivity` or
`calibrate` fails for a question, that question does not get built** (see *Gates*).

---

### Wave 3 — one `objective` question, event-triggered

### T10: Objective question + candidate generation + event triggers

**File**: `crates/brain/src/jev/objective.rs` (new), `crates/brain/src/jev/state.rs` (edit),
`crates/brain/src/brains/jev/policy.rs` (edit), `crates/qbots/src/jev/hub.rs` (edit: trigger classes
for telemetry — pacing itself landed in T5)

**What to do**: Code enumerates, the model judges. **Candidate generation lives in `JevPolicy`**,
not in a `GoalQuery` — `NavGoal::Waypoint(node)` needs `roam_nodes` and `NavGoal::Position` needs
`MapItem` origins, **neither reachable from `&Worldview`**, so the policy resolves candidates from
the copies `JevBrain::set_map` gave it (T7). Prove reachability and relevance, rank by a cheap
heuristic, offer **at most 3**, each tagged **seek** (release on arrival) or **hold** (start a
`hold_s` timer on arrival, release when it elapses). Run the A\* distance buckets **at submit time
only** (asks are seconds apart), never per tick. Store the offered `label → NavGoal` map in the
ticket ring under the `TicketId` that `submit` returns, and resolve an answer **only** against its
own ticket. Criteria text is generated from a fixed template with every fact **also present in
`state`**: hardcoded landmark slug, item classes as **enum names**, A\* distance **bucket** from
me, my loadout gap (`wants:heavy_damage`), a terrain word, local heat bucket. Ask on the event
triggers (each ≥2 s persistent, subject to `event_floor_s`), plus low-priority dwell refresh.
Apply only as the `roam_goal`-fallback substitution, and **hold** the applied objective until a
release condition (*What the model may and may not decide*): arrival (seek) / `hold_s` (hold),
`on_stall`, death, level change, a newer answered ticket (whose gated-off objective releases to
roam), or the hub slot expiring at `ttl_s`. Arrival = within 64 u horizontally of the goal point.

**Before** (`crates/brain/src/brains/q3/mod.rs:926`): the fallback arm is always `roam_goal`.
**After**: `policy.objective(..)` may supply a goal from the candidate set; otherwise identical.

**Verify**: `cargo test -p brain -- jev_objective` — a `StubDecider` naming candidate *k* drives
the navigator to that goal; a label **outside** its ticket's map ⇒ `ack(Rejected(OffTicketLabel))`
and pure-q3 behaviour; an answer for an **older** ticket resolves against **that** ticket's map,
not the current candidate list; the objective stays applied across ticks with no new answer (no
lapse to roam) and releases on arrival (seek), after `hold_s` (hold), on `on_stall`, on death, and
when a newer ticket arrives with the objective gated off; `JevStatic` is unaffected.
`cargo test -p qbots -- jev_objective_render` (the renderer lives in `qbots`): the rendered
request for a golden scenario matches its golden fixture byte-for-byte; **the golden asserts a
TOTAL rendered-request token budget**, not just the state's (`max_state_tokens` bounds only the
state half); **injection**: drive the real `Worldview → DecisionState → criteria → render`
pipeline with configstrings containing `ignore previous instructions` and assert a
**byte-identical** rendered request.
**Commit**: `task(T10): objective question — ≤3 code-proved move candidates, event-triggered asks`

### T11: Telemetry — `applied/asked` as the headline SLO *(may be pulled forward to after T7)*

**File**: `crates/qbots/src/jev/telemetry.rs` (new), `crates/qbots/src/main.rs` (the `EVT` lines),
`crates/qbots/src/supervisor.rs` (final report), the `competition` arm (`--brains` at
`main.rs:249`, `:2634-2646`)

**What to do**: One `info!` per applied decision in the house `EVT` style (per-bot span prefixes
from Plan 63 give bot identity), the debuggable counters at `warn` when their rate is high, **and
one continuous fleet line every ~10 s** — plans 51 and 63 found their bugs because `wall_press`
and `env_suicide` ran *the whole time*, not because they were in the final report, and a
decision-rate collapse is exactly that failure:

```text
EVT jev_decision obj=? post=? reg=? margin=? neff=? conf=? age_t=? src=model|fallback|stale
                   lat_ms=? st_tok=? st_hash=? keys=? q=? trig=? ticket=?
EVT jev_drop | jev_stale | jev_unresolvable | jev_budget | jev_breaker_open    … err=?
EVT jev_reject ticket=? why=unknown_ticket|wrong_map|off_ticket_label|expired|inert|superseded
EVT jev_fleet applied_min=? admitted_min=? asked_min=? inflight=? bucket_toks=?
                tok_in_10s=? max_goal_bots=?          # T15 adds max_region_bots=?
```

`applied` is counted **only** from the brain's `ack(ticket, Applied)` — the hub knows what it
stored, never what the brain did with it; `jev_reject` is the brain's other `ack` arm.

`err=` names the enum (`timeout` / `refused` / `http_501` / `model_mismatch` / `breaker`) —
without it "box down", "deadline too tight" and "someone swapped the model" are one symptom.
`keys=` makes per-question denominators computable from the log alone. `st_hash` is 6 hex.

Final report, **per brain arm**: `asked`, `admitted`, `applied`, **`applied/asked` (headline)**,
**`gate_reject_rate`**, **`admission_drop_rate`**, all **split by `(question key × trigger class)`**
(the W3 gate is per-trigger and `trig=` is the only thing that makes it computable), fallback /
stale / dropped / unresolvable / brain-rejected (by `BrainReject` arm), decision-age p50/p95
**at apply time**, a margin histogram (and `n_eff`), `distinct_choices`,
`decisions_that_changed_per_bot_per_min`, and the **herd metric**. Regions do not exist until
T14, so here the herd metric is **max concurrent jev bots whose applied objective resolves to the
same goal** (same item / same node) plus the distinct-goal count; T15 adds the region occupancy
histogram and max-concurrent-per-region. **`competition` hard-fails if the jev arm's applied
count is 0 — printing the report FIRST**, or the W3 gate run destroys its own evidence.
Optional `jev.debug: true` + `jev.debug_sample: 20` = **1-in-20** sampled prompt+answer dumps to
`logs/jev/` (gitignored).

**Ordering note**: this task is the one reviewers would move to immediately after T7, because a
human arriving at the box to run T9's gate is under-instrumented if the in-process counters don't
exist yet. If it isn't moved, T9's `bench` is the operator's only window during W2.

**Verify**: `cargo test -p qbots -- jev_telemetry` — a scripted run yields the exact tallies and
all three ratios, split by key and trigger, with `applied` equal to the number of `ack(Applied)`
calls (not the number of stored decisions); **prove the guard is live by feeding the report a
zero-applied tally and asserting a non-zero exit** (removing an assertion cannot make a test
fail, so that phrasing is not a proof).
**Wave-gate check (live, human — W3)**: `cargo run -p qbots -- competition --brains
q3,jev_static,jev` prints all three arms' counts.
**Commit**: `task(T11): jev telemetry — EVT jev_fleet, applied/asked + gate/admission rates, herd`

---

### Wave 4 — engagement posture (commitment, not targeting)

### T12: `posture` question + bounded application + dwell/commitment

**File**: `crates/brain/src/jev/posture.rs` (new), `crates/brain/src/brains/jev/policy.rs`
(edit). `crates/brain/src/brains/q3/policy.rs` is **not** edited — T6's `Posture { disengage }`
is the whole interface.

**What to do**: A **binary** `choice` over **{`disengage`, `hold`}** (`hold` = "leave it to q3")
plus a **`noul` canary** ("is there enough in this state to prefer one of these?") — validated
against the captured flat fixtures before it is allowed to be load-bearing, and **cuttable**
(explicitly not load-bearing). *Why binary*: under add-caution-only an `engage` answer can never
change anything and the first draft's `trade` mapped to nothing, so a 3-way question only split
probability mass between behaviourally identical options and read as uncertainty at the gate
(live: engage/trade/disengage on an obviously bad state came back at margin 0.065). Apply **only**
through T6's merge — `disengage ⇒ Posture { disengage: true }` ⇒ `wants_retreat = true` **and**
`wants_chase = false`, plus the `:506` veto on distant, unaware enemies — and **never** add
aggression. Choosing *among* acquired enemies stays entirely local and per-tick (q3's
`battle_goal` re-picks it, and a stale remote preference over a PVS set is the every-tick
oscillation the Blocks.ai write-up saw). Posture is asked on `damage_above_threshold` and enemy
contact (≥2 s persistent), lives for `posture_max_age_s = 6.0` of brain time from the tick its
ticket is first seen, and has hysteresis on entry and exit so a decision boundary never teleports
the FSM; it is released instantly on `on_stall`, on death, on a level change, and when a newer
answered ticket gates it off. A lapse is not a gap: q3's own heuristic is the default.

**Before** (`q3/mod.rs:886-891`, `:506`): T6's merge with `NullPolicy` ⇒ `disengage == false`.
**After**: `JevPolicy::posture` returns `Posture { disengage: true }` while an applied, unexpired
`disengage` is held; otherwise the default.

**Verify**: `cargo test -p brain -- jev_posture` — a stub `disengage` with a visible enemy and a
**high-aggression** state (`h_chase == true`) forces `status()` to `"retreat"` after one tick
(`Q3Node::label`, `q3/mod.rs:63`; `JevBrain::status()` delegates verbatim per T7) and never trips
`MAX_NODESWITCHES` — the exact case the first draft's merge turned into a non-firing Chase; a
stub `hold` leaves every output identical to `jev_static`; the posture expires after 6.0 s of
brain time and q3's heuristic resumes; the "enemy leaves PVS while a posture is pinned" scenario
still reaches `SeekLtg` — exits at `:576`, `:598-599`, `:607`, `:614`, gated on `enemy_alive`
(`:350`) — **i.e. no frozen bot**; `posture()` is called exactly once per tick, so both posture
sites see the same answer by construction.
**Wave-gate check (live, human — W4)**: publish `decisions_that_changed_per_bot_per_min` — **> 2
posture flips per bot per minute is an investigation** — and the composition check
(`spawn-to-spawn` ≥ 13/24, no `max_speed > 320`), noting in the report that this exercises the
inner q3 brain, not the jev layer.
**Commit**: `task(T12): binary posture question — disengage adds caution and clears chase, with dwell`

---

### Wave 5 — area control (fleet-coach carve-out) and acceptance

### T13: The AGENTS.md amendment — **human approval required before T15**

**File**: `AGENTS.md` (Concurrency Model section)

**What to do**: Propose the exact wording for the one deliberate exception the user chose, e.g.:
the **decider hub** (`crates/qbots/src/jev/hub.rs`) is an *external coach*, not a bot's
perception; it may read every jev bot's latest region **preference** (the probabilities its own
fleet-blind question returned) and assign each bot at most one region under a per-region cap;
**no bot's
`Brain::tick` may read another bot's state**, and every brain other than `jev` remains
fleet-blind. Record alongside it that `--brains jev` is therefore **not strictly comparable** to
fleet-blind brains, and that `jev_static` is the control that absorbs it. Mirror the note in
`beacon.rs`'s module doc, which is the existing precedent for walking this line.

**Verify**: the human has approved the specific diff (recorded in the tracker with date), and
`grep -n "carve-out\|decider" AGENTS.md` shows it in the Concurrency Model section.
**Commit**: `task(T13): AGENTS.md — the jev fleet-coach carve-out, with its non-comparability note`

### T14: Per-map region tables + move generation

**File**: `crates/qbots/src/jev/regions.rs` (new), `config.example.yaml` (qbots root, edit),
`crates/brain/src/jev/state.rs` (edit: add the `Here`/`Alts` `FieldKey`s and bump
`STATE_SCHEMA_VERSION` to 2), `crates/qbots/src/jev/render.rs` (edit: the two new match arms —
the compile error this design promised), `crates/brain/src/jev/objective.rs` (edit: legend text)

**What to do**: Hand-authored regions in config — **4–6 per map**, each `name` slug + `anchor`
node + item classes — resolved to existing `roam_nodes`/`MapItem` origins; validate at load that
every anchor resolves to a real nav node and **refuse to start** naming the bad one. Regions are
labelled and described by **contents** (`dock_mega` = "dock + mega armor"), never bare indices —
the flat-distribution measurement says an unnameable option set cannot be answered. Each jev
bot's **own** request gains a `region` question over **≤3 offered moves** out of the 4–6 known,
in the **same request** as everything else (fleet-blind: no carve-out needed for this task).
Answers are stored and logged (`reg=` on `EVT jev_decision`) but **not applied** until T15's
assignment exists — applying uncoordinated region answers is the herd. **Re-price the budget
here**, because this is the task that grows every request: `tokens_per_req_target` 350 → **420**,
`min_dwell_s` 15 → **20** (36/20 = 1.8 req/s ⇒ 756 tok/s ✓), `ttl_s` 24 → **30**
(26 + 1.5 + 0.35 + 1 = 28.85).

**Verify**: `cargo test -p qbots -- jev_regions` — q2dm1's table loads, all anchors resolve; a
bogus anchor produces a startup error naming it; the rendered region question has ≤3 options and
every option's description contains a fact that also appears in `state`; **the golden asserts the
W5-sized TOTAL request budget (~420 tok)**; the re-priced config keeps worst-case refresh demand
under `max_input_tokens_per_s` and satisfies the `ttl_s` inequality (assert the arithmetic in a
test); **the `ignore previous instructions` injection test runs again here**, against a state
built from a worldview whose **BSP-sourced item classnames** carry it, asserting a byte-identical
rendered request.
**Wave-gate check (live, human — W5)**: `jev-probe flatness` and `calibrate` for the region
question shape.
**Commit**: `task(T14): per-map region tables, per-bot ≤3-move region question, budget re-price`

### T15: Fleet-coach region assignment + herd control

**File**: `crates/qbots/src/jev/assign.rs` (new — a pure function), `crates/qbots/src/jev/hub.rs`
(edit), `crates/brain/src/jev/source.rs` + `crates/brain/src/brains/jev/policy.rs` (edit: the
`AppliedDecision` gains an `assigned_region: Option<RegionIdx>`), `crates/qbots/src/jev/telemetry.rs`
(edit), `config.example.yaml` (edit: `region_cap`)

**What to do**: **No new model question and no extra request** — the model's job ended at T14's
per-bot preference; the assignment is code, inside the hub (the carve-out's home). On every
stored region answer, and at least once per second, re-run a **greedy capacity-capped
assignment** over every jev bot's latest fresh region probabilities: sort all `(bot, region, p)`
triples by `p` descending (tie-break: lower bot index — deterministic), assign when the bot is
unassigned, the region is below `region_cap` (default `max(2, ceil(n_jev_bots / n_regions))`), and
`p` passes the calibrated margin gate for that bot's answer. A bot with no eligible region gets
`None`. Keep an assignment until the bot's own answer changes, it dies, or the map changes —
hysteresis against reshuffles. The assignment rides out to each bot in its `AppliedDecision`, and
`JevPolicy` applies it at the `:926` arm **below** an applied objective (precedence: objective >
region > `roam_goal`). A revoked or absent assignment falls through to objective/roam **the same
tick** (never idles). Record the response-cache decision (on/off) with reasoning — caching
guarantees identical answers for similar bots; persona prompts defeat it. Extend the report with
the occupancy histogram and max-concurrent-per-region (`max_region_bots=` on `EVT jev_fleet`).

*Why not "one fleet-level region question"* (the first draft): a `choice` returns one label, so
one question cannot yield N distinct assignments; and T14 had already put the region question in
each bot's request, so the two tasks described different systems.

**Verify**: `cargo test -p qbots -- jev_fleet` — `assign()` over 36 simulated bots never exceeds
`region_cap`; with identical preferences (the deterministic-herd case) the bots spread across
regions up to the cap instead of piling into one; assignments are stable across re-runs with
unchanged inputs; a bot whose answer fails the gate is never assigned. `cargo test -p brain --
jev_region`: objective beats region at `:926`; a revoked assignment yields a valid q3 goal the
same tick.
**Wave-gate check (live, human — W5)**: a 12-bot run whose report shows a **non-degenerate**
region histogram — one region holding 12 bots is a failure of *the layer*, not of the map.
**Commit**: `task(T15): code-side capped region assignment (fleet coach), herd histogram`

### T16: Acceptance A/B + docs + brain notes

**File**: `docs/BRAINS.md` (§6 "Adding a brain" checklist + switch tables),
`config.example.yaml` (qbots root), `context/brain_notes.md` (dated append — **mandatory** for
brain plans), `context/pitfalls.md`, `context/acceptance.md` (if the A/B command shape changes)

**What to do**: Run the pre-registered A/B — arms `q3`, `jev_static`, `jev`; **N ≥ 5** runs each
(the repo's noise-floor rule); paired by `(map, servercount)`; primary metrics decision-sensitive
(`time_in_target_region`, latency-to-a-named-item, status churn, region entropy, `applied/asked`);
frags/K/D secondary; **including at least one metric predicted to show no difference**. Then write
the docs, append the dated `brain_notes.md` section, and add `context/pitfalls.md` entries for at
least: *brain-rebuilt-per-session ⇒ generation must be hub-owned*; *the two clocks*
(`self.time += dt` vs `Instant`); *determinism ⇒ herding*; *flat distributions are a property of
the question, not the model*; *a hostname that resolves on the dev box through `/etc/hosts` is
not a contract (and `.lan` is not mDNS)*; *one jitter direction is a budget constraint, not a
style choice*; *`n_eff ≤ 1.5` demands p1 ≥ 0.86 for n ≤ 3 — use a margin gate for small option
sets*; *forcing `wants_retreat` without clearing `wants_chase` loops q3's `run_fsm` into a
non-firing Chase*; *a parity test that compares a refactor with its own delegate proves nothing —
record a golden first*.

**Verify**: the A/B numbers are in the tracker's **Evidence** table (command, build, result, run
count, spread); `docs/BRAINS.md` §6 contains `jev`; `context/brain_notes.md` has a
`## 2026-…` section for this plan; the named `context/pitfalls.md` entries exist.
**Commit**: `task(T16): jev acceptance A/B, BRAINS.md + brain_notes + pitfalls`

---

## Dependency matrix

```text
T1 (human, any time; parallel with W1)

T6 (independent — best first task) ─────────────┐
T2 ─▶ T3 ─▶ T4 ─▶ T5 ─▶ [W1 gate] ──────────────┴─▶ T7 ─▶ T8 ─▶ T9 (LIVE) ─▶ [W2 gate]
                                                                                 │
                                                       T10 ─▶ T11 ─▶ [W3 gate] ◀─┘
                                                                         │
                                        ┌────────────────────────────────┴───────────┐
                                        ▼                                            ▼
                                 T12 ─▶ [W4 gate]               T13 (human) ┐   T14 ─┤
                                        │                                   └──────▶ T15 ─▶ [W5 gate]
                                        └──────────────────────▶ T16 ◀──────────────────────┘
```

- **Offline span: T6, then T2–T5, [W1], T7–T8**; **T9 is the live handoff**; then **T10–T11**; then
  the W3 gate.
- **T6 is independent of T2–T5** and is the best first task for a fresh session: it is verifiable
  against the 12 tests that already exist plus its own pre-seam golden, with zero new scaffolding.
  It touches live play, so it gets its own commits and its own inertness proof. **W1 gates T7**,
  not T6 — T7 is the first task that needs the hub's types.
- **T11 may move to immediately after T7** (see the task's ordering note) so the W2 live session is
  instrumented.
- **After W3, two independent tracks**: posture (T12 → W4 gate) and regions (T14, plus T13's human
  approval, → T15 → W5 gate). Neither waits on the other. **T16 needs both gates** (or a recorded
  cut of the region track if T13 is declined — see risk 4).
- **T13 gates T15, not T14**: T14's per-bot region question is fleet-blind and its answers are
  only logged; applying them is T15, and T15 is what needs the carve-out.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/brains/q3/mod.rs` | `Q3Brain<P: GoalPolicy = NullPolicy>`; posture once per tick before `select_enemy`, merged at `:886-891` (disengage ⇒ retreat **and** no chase) and at the `:506` veto; the `:926` objective arm; `on_stall` from the recovery match; **behaviour-preserving — pre-seam golden unchanged** | P0 |
| `crates/brain/src/brains/q3/policy.rs` | `GoalPolicy` (`&mut self` methods, no `Send + Sync` bound), `NullPolicy`, `Posture { disengage }`, `GoalQuery`, `PostureQuery` | P0 |
| `crates/brain/src/brains/q3/testdata/parity_200.txt` | the 200-tick golden recorded **before** the seam | P0 |
| `crates/brain/src/brains/jev/{mod,policy}.rs` | `JevBrain { inner: Q3Brain<JevPolicy> }` (intercepts `set_map`, sets the `goal_override` inert flag), `JevPolicy` (bot's `DecisionSource` handle, ticket ring, held objective/posture + release rules, map copies) | P0 |
| `crates/qbots/src/jev/hub.rs` | pull workers (refresh ≤ 2, events may use all 3), bucket (reserve/true-up), tickets, pacing, newest-wins slots, gate, generation, tiered breaker | P0 |
| `crates/qbots/src/jev/transport.rs` | the seam the fake-clock tests need | P0 |
| `crates/brain/src/jev/{gate,decision,source}.rs` | pure gate (margin; `n_eff` for n ≥ 4), `Decision`/`NoSignalReason`, `TicketId`/`AppliedDecision`/`ApplyOutcome`, `DecisionSource` | P0 |
| `crates/brain/src/jev/state.rs` | closed vocabulary, builder, `STATE_SCHEMA_VERSION`, class→slug table | P0 |
| `crates/qbots/src/jev/net.rs` | HTTP/1.1 POST; chunked + `Connection: close` + timeout-wraps-connect | P0 |
| `crates/qbots/src/main.rs` + **`supervisor.rs` both spawn points** | hub wiring, `on_map_change`, no I/O in the ticker arm | P0 |
| `crates/brain/src/brains/mod.rs` | `BrainKind` arms + `build_brain_with_decider` (sibling; 7 existing call sites untouched) | P1 |
| `crates/brain/src/brains/core.rs` | `fn on_map_change(&mut self, &str)` default hook | P1 |
| `AGENTS.md` | the fleet-coach carve-out (**needs human approval**) | P1 |
| `crates/qbots/src/jev/render.rs` | exhaustive renderer — the security boundary | P1 |
| `crates/qbots/src/jev/probe.rs` + a `Cmd::JevProbe` arm in `main.rs` | `qbots jev-probe` — the live gate and threshold calibration; every live number in this plan comes from here | P1 |
| `crates/qbots/src/config.rs` | `jev:` optional YAML block, `require`, gate margins | P1 |
| `crates/qbots/src/jev/telemetry.rs` | `EVT jev_fleet`, the three ratios, `ack`-based `applied`, herd metric, zero-applied hard-fail | P1 |
| `crates/qbots/src/jev/regions.rs` | 4–6 region tables, anchor validation | P2 |
| `crates/qbots/src/jev/assign.rs` | greedy capacity-capped region assignment (pure function) | P2 |
| `docs/BRAINS.md`, `config.example.yaml`, `context/brain_notes.md` | operator-facing docs | P2 |

## Open Questions / Risks

1. **Applied-rate may be near zero by construction — and the first draft's gate guaranteed it.**
   `n_eff ≤ 1.5` needs p1 ≥ 0.86–0.90 for every n ≤ 3 question; on 2026-11-07 only a cartoonishly
   explicit state ("cannot win this fight", p1 0.880) cleared it, while an obviously bad state
   (hp 18, no armor, 2 enemies) scored 0.663 binary and 0.441 three-way. Even with the margin gate,
   typical states may be *correctly* gated to q3, making the measured effect of the whole feature
   zero for legitimate reasons. *Mitigation*: margin gate with **W2-calibrated, then frozen**
   thresholds; richer, more discriminating state is the lever, never looser thresholds; the W3
   gate is a measurement with **separate gate-reject and admission-drop ratios**; event-triggered
   asking exists specifically to raise state discrimination; `sensitivity`/`calibrate` delete
   unanswerable questions; T11 publishes the rates before W4/W5 spend effort.
2. **Determinism ⇒ herding** (*HIGH*, and unmitigable by sampling). *Mitigation*: one-sided jitter
   on the ask phase, position-dependent candidate sets, persona in `state`, the fleet-coach
   assignment in T15, and the herd metric published in every report so a negative result is
   visible as *herding* rather than recorded as *the model is bad*.
3. **The A/B is confounded against `q3`.** `set_map` discards `BrainMap.items`
   (`q3/mod.rs:836-841`) and `roam_goal` (`:211-231`) is PVS-only, so jev-with-regions vs q3 also
   measures "who read the BSP". *Mitigation*: `jev_static` is the **primary** control, `q3` the
   secondary; the expected sign of the effect is not obviously positive and the plan says so.
4. **`AGENTS.md` carve-out not yet approved.** T15 must not be written before T13 is
   human-approved; if the human declines, T14/T15 are cut and the plan closes after T12 with the
   region layer deferred to a follow-up plan.
5. **`cosmo.lan` resolves on the dev box only through a loopback `/etc/hosts` entry**, so (a) a
   fleet on any other host may not resolve it, and (b) every latency in this plan is loopback.
   *Mitigation*: T1 records the fleet host and its resolver path; `require: true` turns a
   resolution failure into a startup refusal rather than a silent q3 run; a remote fleet re-runs
   `jev-probe bench` before trusting `deadline_ms`; recommend an IP default with the hostname
   documented.
6. **The API may be a stopgap.** The 501 wording reads purpose-built; a server change could
   delete `/v1/systemone` wholesale. *Mitigation*: `jev-probe health` asserts the 501
   fingerprint + slug at startup, `matrix --replay` detects a server-side change, and the run
   header logs the server-reported model, so a silent swap surfaces in the report, not the scoreboard.
7. **`confidence` is identified but undocumented.** Empirically `(p1 − 1/n)·n/(n−1)`, exact on
   three live answers — a function of `p1` alone, so no second opinion, and a server internal that
   could change. *Mitigation*: we compute our own statistic, log theirs, never gate on it; T1
   records the formula and asks whether it is contractual.
8. **`noul` semantics unvalidated.** Cheapest possible "do you actually know" probe, but its
   meaning may not be what we assume. *Mitigation*: canary-only, validated against the captured
   flat fixtures, explicitly **not** load-bearing, and fourth in the cut order.
9. **No dynamic `status()` tag (deferred, and the reason is a judgement call, not a
   constraint).** `fn status(&self) -> &str` (`core.rs:123-125`) **can** carry a dynamic value
   with **zero trait change** — hold `status_tag: String` in `JevBrain`, refresh it on apply
   (~2.4/bot/min by our own dwell math), return `&self.status_tag`. We defer it because the
   decision surface already lives in `EVT jev_decision` + the report, and because a prefix on
   `status()` would break T12's assertion and the `status()` pattern in
   `crates/brain/tests/q3_brain.rs`. Revisit if operators ask for it in logs.
10. **Plan 71 is still open** (T3/T5/T6 pending) and touches `combat.rs`/`recover.rs`/
    `move_ctrl.rs`. It is marked **deferred in `SERIES.md`** per Rule C.5. T12's live check could
    be confounded by plan 71's T5 (unbounded stale-target pursuit); if a jev run shows ghost
    chasing, note it as plan 71 T5 rather than fixing it here.
11. **Cost creep in `state`, and a budget that doesn't cover the whole prompt.** Latency tracks
    input tokens. `max_state_tokens` bounds only the state section — criteria templates and three
    region legends ride on top — which is why T10/T14 goldens assert a **total** rendered-request
    budget, and why `st_tok` is on every decision line with `input_tokens > 2000` as a `warn`.
12. **Calibration can be gamed by its own labels.** `calibrate` picks thresholds from human-labelled
    *obvious* / *ambiguous* sets; labelling after seeing answers would fit the threshold to the
    model. *Mitigation*: the sets are committed **before** the first `calibrate` run (the commit
    order is the evidence), thresholds are frozen at W2, and a question no threshold can serve is
    deleted.
13. **Effort exceeds RULES' 3-day scale** (~5–6 days). *Mitigation*: honest estimate in the TL;DR;
    W4/W5 are independent tracks after W3, and if W3 slips they move to sub-plans `72_1`/`72_2`
    rather than being compressed.

## Verification Checklist

- [ ] T1: `context/distilled/jev.md` holds the contract, the measurement table **labelled loopback**, the `confidence` formula, and the other two answers resolved or struck
- [x] T2: golden render byte-exact; 100× re-render byte-equal; over-budget drops the named bucket; no region facts or opaque indices in the state; `crates/brain/Cargo.toml` byte-unchanged
- [x] T3: every `NoSignalReason` arm covered by a **Rust-constructed** table test (no JSON in brain); flat 4-way ⇒ `Flat{margin 0.175, n_eff 3.20}`; live `{0.880,0.120}` ⇒ apply; `{0.663,0.337}` ⇒ flat; regression row `{0.80,0.20}` ⇒ apply
- [x] T4: chunked / **close-delimited** / truncated / 400 / 501 / malformed / empty / **missing key** / **extra key** / bad-sum / unknown type / **model mismatch** / refused / zero-bytes / slow-body each yield the specified `Decision` variant, on the **real** clock; no new packages in `Cargo.lock`
- [x] T5: scripted-transport + `start_paused` tests prove reserve/true-up limiting, one-in-flight-per-bot replacement with a new ticket, pick-time `snapshot_ttl` drop (`skipped_no_capacity`), oldest-first service, events before refresh and refresh ≤ 2, `event_floor_s` pacing, **tiered** breaker (isolated timeouts do not open it), generation discard, gated-off answers supersede, `ttl_s` expiry, slot read off-runtime, `ack`-counted `applied`
- [x] T6: pre-seam 200-tick golden committed **first**; after the generic seam the 12 existing q3 tests **and** the golden pass unedited; disengage on a high-aggression state lands in `BattleRetreat` without tripping `MAX_NODESWITCHES`; a policy objective reaches `StubNav::last_goal` in `SeekLtg` only
- [ ] T7: absent `jev:` ⇒ no task, no socket; `require: true` + `127.0.0.1:1` ⇒ startup error naming `cargo run -p qbots -- jev-probe health`; `require: false` ⇒ one `error!` then jev-identical-to-`jev_static`; `jev`/`jvs`/`jev_static` parse; `build_brain`'s 7 other call sites untouched; `brain_code`s fit the 15-char `netname`; `goal_override` ⇒ no submit and default posture
- [ ] T8: stale generation discarded by the hub; wrong-map / off-ticket / unknown-ticket ⇒ the matching `ack(Rejected(..))`; triggers submit on the **next** tick; the `goal_override` guard test passes **and is labelled a guard**; hub started at **both** `supervisor.rs:323` and `:682`; `map_servercount` assignment still last in the branch
- [ ] T9: offline argv/calibration-search tests pass; *(W2, live)* `health` (slug + 501 with a well-formed body), `calibrate` thresholds recorded and frozen, **both** sensitivity pairs move, anti-correlation, flatness rejected, bench p50/p95/p99 + tok/s vs **both** figures + p95-vs-deadline verdict — all in the Evidence table
- [ ] T10: stub candidate *k* drives the navigator to that goal; off-ticket label ⇒ `OffTicketLabel` + pure q3; older tickets resolve against their own map; the objective is held without lapsing and releases on each release condition; golden matches **total** request budget; **injection through the real pipeline renders byte-identical**
- [ ] T11: tallies + all three ratios split by `(key × trigger)`; `applied` == `ack(Applied)` count; `EVT jev_fleet` every ~10 s with `err=`/`keys=`; **zero-applied tally ⇒ non-zero exit with the report printed first**
- [ ] W3 gate: the exact `--count 12` command run ≥2×; `applied/asked`, `gate_reject_rate`, `admission_drop_rate` published per arm and per key; **gate_reject ≥ 85 % on event asks ⇒ fix the state or delete the question before T12; admission drops ⇒ raise dwell, never loosen the (frozen) thresholds**
- [ ] T12: binary `disengage`/`hold`; stub `disengage` on a high-aggression state ⇒ `"retreat"` with no node-switch guard trip; stub `hold` ≡ `jev_static`; posture expires after `posture_max_age_s`; enemy-leaves-PVS while pinned still reaches `SeekLtg` via `:576/:598-599/:607/:614`; *(W4, live)* flips > 2/bot/min flagged; composition check passes **and is labelled as such**
- [ ] T13: the exact `AGENTS.md` diff is human-approved and dated in the tracker
- [ ] T14: q2dm1 table loads, all anchors resolve; a bogus anchor names itself at startup; ≤3 options, each containing a fact that is also in `state`; `Here`/`Alts` added with `STATE_SCHEMA_VERSION` 2; W5-sized total-request golden; re-priced budget + `ttl_s` inequality asserted; **classname injection test**
- [ ] T15: `assign()` never exceeds `region_cap`, spreads identical preferences, is stable; objective > region > roam at `:926`; revoked assignment ⇒ valid q3 goal same tick; *(W5, live)* 12-bot region histogram **non-degenerate**
- [ ] T16: A/B in the Evidence table (N ≥ 5/arm, paired, including the predicted-no-difference metric); `docs/BRAINS.md` §6 lists `jev`; dated `brain_notes.md` append; the 10 named `context/pitfalls.md` entries exist
- [ ] Rule A held at every commit: `cargo fmt` + `cargo clippy -- -D warnings` + `cargo test` green
- [ ] Rule C: plan + tracker `git mv` to `completed/` and `SERIES.md` marked done
