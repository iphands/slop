# Plan 72 — Jev decision brain (`jev`): model-driven high-level play over a q3 core

> **Status**: pending
> **Created**: 2026-11-07
> **Revised**: 2026-11-07 (pre-commit) — corrected after two artifact reviews: the `GoalPolicy`
> trait shape (ownership made the policy unreachable from the wrapper), the posture merge (it
> permitted *more* aggression, the one thing the design forbids), three budget-table arithmetic
> errors, an unsatisfiable `Cargo.lock` gate, a missing second hub spawn point, and a false
> justification for deferring the `status()` tag.
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
> Rule A (zero warnings) and Rule B (commit per task) are not optional here.

---

## TL;DR

**What**: Add `BrainKind::Jev` — a thin brain that composes the existing `Q3Brain` and, through
a new `GoalPolicy` seam inside q3, substitutes the model's answer for three *high-level*
choices (which objective to pursue, whether to engage or disengage, which area to control).
All model I/O lives in one process-wide hub in `crates/qbots`; the brain only ever reads a
fresh, code-validated decision out of a slot, so a dead endpoint degrades to pure q3 per answer
and never stalls a bot's socket loop.

**Deliverables**:
1. `crates/brain/src/jev/` — closed-vocabulary `DecisionState` + its `Worldview`→state builder,
   the `Decision`/`NoSignalReason` gate type, `DecisionSource` trait (no `async fn`), `NullDecider`,
   `StubDecider`, `JevBrain`, `JevPolicy`. **`crates/brain/Cargo.toml` is not touched at all.**
2. `crates/brain/src/brains/q3/policy.rs` — the `GoalPolicy` seam over q3's three goal pickers
   and its posture inputs, defaulting to today's code (**behaviour-preserving**, gated by a
   parity test inside q3's own test module).
3. `crates/qbots/src/jev/` — hand-rolled HTTP/1.1 `POST` (~150 lines, **no new packages**), the
   `/v1/systemone` wire types, the closed-set JSON renderer, and the hub: newest-wins snapshot
   slots per bot, a worker pool behind an injectable transport seam, a **fleet token bucket**, a
   bounded permit wait, a generation counter and a tiered circuit breaker.
4. `crates/tools/src/bin/jev_probe.rs` — `health` / `ask` / `bench` / `sensitivity` /
   `anticorrelation` / `flatness` / `matrix` (+`--json`, `--config`), the only thing that talks to
   the live box during development, and the harvest tool for the offline fixtures.
5. One `objective` question (≤3 offered moves, event-triggered), one `posture` question
   (engage/trade/disengage + `noul` canary, permitted **only to add caution / remove aggression**),
   and a region layer of 4–6 regions asked as ≤3 *moves* with **fleet-coach assignment**.
6. Telemetry that can answer "did the model do anything at all?": `EVT jev_*` lines including
   `err=` and `keys=`, a continuous `EVT jev_fleet` line, `applied/asked` as the headline with
   `gate_reject_rate` and `admission_drop_rate` split out, decision-age p50/p95,
   `distinct_choices`, and a **herd metric** (max concurrent bots per region). `competition`
   hard-fails if the jev arm applied 0 decisions — **after** printing the report.
7. Docs: `context/distilled/jev.md` (contract + measurements), `docs/BRAINS.md` §6,
   `config.example.yaml`, a dated `context/brain_notes.md` append, and `context/pitfalls.md`.

**Estimated effort**: Large (3 days), offline-green throughout; **T5 is the overrun risk** (eight
interacting mechanisms + a test each), not T15. The live gates are a human's half-day on the box.
Cut order if W2 slips is in the tracker; none of the four cuts breaks the A/B.

---

## Context

### The endpoint, measured 2026-11-07

`cosmo.lan:8710` is **not** a chat server. It natively implements the TypeSafe **System One**
decision API; `/v1/chat/completions` answers `501 {"error":{"message":"This model is a
decision model without text generation, use /v1/systemone"}}` — that 501 is our capability
fingerprint, assert it in `jev-probe health`.

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

| Probe (all measured from the dev box) | Result |
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

`confidence` reported for that flat case is ≈ `top1 − 1/n` (computed 0.0093 vs reported 0.0096).
If that identity holds we must **not** use it as an independent second opinion. T1 asks.

### Why wrap `q3` instead of forking it

The original brief said "a brain copied from q3". Both reviewers argued against the fork and the
user chose the wrapper. What a fork costs is not merely the module's bulk (`q3/mod.rs` +
`move.rs` + `aim.rs` — quote `wc -l` if you need the number) but its **12** `#[test]` functions
in `q3/mod.rs`, which would stay **permanently green while exercising the heuristic path**, so
the model path ships with zero coverage and every future q3 fix needs a twin commit that nobody
will make.

So: `JevBrain` **composes** a real `Q3Brain` (`crates/brain/src/brains/jev/mod.rs`, ~120 lines)
and the only thing q3 changes is that its three goal pickers — `roam_goal` (`q3/mod.rs:211`),
`battle_goal` (`:407`), `retreat_goal` (`:421`) — and its posture inputs
(`crate::q3char::wants_to_retreat` / `wants_to_chase`, called at `q3/mod.rs:890-891` **and again
at `:506`** for the sneak-past veto) sit behind a `GoalPolicy` trait whose default impl **is
today's code**. `--brains q3,jev_static` then differs by one variable, which is a better
experiment than a forked file, and drift is structurally impossible.

A wrapper alone is *not* enough, and the plan records why: q3 honours
`ctx.goal_override` (`brains/core.rs:56`, consumed at `q3/mod.rs:900`) by **always
path-following**, which suppresses its combat FSM. That is the right mechanism for an
*objective*, and the wrong one for an engagement *posture*. Hence both: wrapper + seam.

### The two-clocks rule

The brain keeps time by `self.time += dt` (`q3/mod.rs:880`), fed from serverframe deltas clamped
`0.02..0.3` and *carried* on ticks that bring no new frame. That is deliberately not a wall
clock, and it is what makes brain tests reproducible. **All deadlines, TTLs, dwell timers and
token buckets live in the hub on `time::Instant`.** The brain's own temporal judgement is
identity (`Decision.id` vs the id it last applied) plus a tick-derived freshness bound.

Consequence for the hub: a `Decision` carries `id: u64` from a **hub-owned** `AtomicU64`, never a
per-brain counter — `build_brain` is re-invoked per connection session (`main.rs:972`, inside
`bot_task` which starts at `:877`; the supervisor re-spawns `bot_task` on reconnect), so a
per-brain counter restarting at 0 makes stale-response rejection unsound across a reconnect.

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
| `posture` | **add** caution and **remove** aggression at the two posture sites (`:506`, `:890-891`), and set dwell/commitment | target selection, weapon selection, dodge |
| `region` | choose among ≤3 code-generated **moves**, each resolving to an existing `NavGoal` | raw coordinates, entity numbers, configstring indices |

Everything else stays q3's: FSM, aim, lead, weapon select, dodge, hazard, recovery, and the
stall/wall-block detector. **A decision is released the moment the stall detector fires, on
death, and on a level change.** The FSM cannot be trapped by a pinned posture: `enemy_alive`
(`q3/mod.rs:350`) means "the entity is in the worldview at all", and the exits to `SeekLtg` are
at `:576`, `:598-599` (BattleChase), `:607` (BattleRetreat) and `:614` (BattleNbg).

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

Their own `confidence` field is not usable as our gate (it appears to be `top1 − 1/n`, and
coupling our thresholds to an undocumented server internal is its own bug). We compute both
statistics in code:

- `n_eff = exp(−Σ p ln p)` — reject unless `n_eff ≤ max(1.5, 0.5·n)`
- **odds ratio** `p1/p2 ≥ 2.0` for `n ≥ 3`; `p1 ≥ 0.75` for binary
- `max(p) − 1/n` kept only as a cheap logged "said nothing at all" secondary

**Worked example from our own captured fixture** (the case that killed the simpler gate):
`objective` came back `{hunt 0.3164, hold_area 0.1040, roam 0.4911, seek_item 0.0885}` —
`n_eff = 3.20` of 4, odds ratio **1.55**. A `max(p) − 1/n` gate (0.241) would have **applied**
it. Our gates reject it. Commit that response as `fixtures/jev_objective_flat.json`; it is the
most valuable fixture we own.

Additional rules: **no `none_of_the_above` option** (it parks mass and turns the gate into a third
outcome to calibrate — let the gate produce "no decision ⇒ q3"); ask about **moves, not places**
(`hold_rail_bridge_wait_respawns`, not `region_7`), because actions describe themselves;
**cap options at 3 offered out of 4–6 known**; treat the answer as *not actionable* in the type,
not via a magic float:

```rust
pub enum Decision<T> { Apply(T), NoSignal { reason: NoSignalReason, stats: DistStats },
                       Malformed(String), Transport(String), CircuitOpen }
```

Every arm is one telemetry line and one counter. This enum **is** the observability design.

**Denominators — three, and they answer different questions.** Publishing one ratio conflates two
failures:
- `applied / asked` — the **headline**. Everything we wanted to ask and could not is counted
  against us, which is honest. (A ratio whose denominator excludes `Transport`/`Malformed`/
  `CircuitOpen` **passes at 100 % on a hung box that answered four times, confidently**.)
- `gate_reject_rate = NoSignal / admitted` — "when we got an answer, did the state discriminate?"
- `admission_drop_rate = (asked − admitted) / asked` — "was the budget/permit queue the limiter?"
  **This one is never part of the W3 judgement.**

### Event-triggered asking, not timer asking

A timer asks about every state, including the ~90 % where nothing is distinguishable, which
*guarantees* flat distributions and a low applied-rate. A bot that just respawned at 20 hp with
no armor has a state that genuinely discriminates. Triggers: `death/respawn`,
`loadout_gap_became_critical`, `damage_above_threshold`, `area_heat_crossed_threshold`,
`arrived_in_region` — each requiring the condition to **persist ≥2 s** (enemy LOS flickers at
10 Hz; `took_damage` is a per-frame health delta), plus a per-bot **one-sided-jittered** re-ask
floor so 36 bots never arrive at the permit queue in phase. Background refresh at `min_dwell`
remains, at lower priority. Invalidation means "don't apply; run pure q3", never "clear the plan".
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
eyes) sees a fleet summary and hands each bot a distinct region. This is a **written exception to
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

- **Configured-but-unreachable endpoint ⇒ refuse to start.** `[jev] require = true` (default when
  the block is present) with an actionable message:
  `brain 'jev' needs a reachable [jev] endpoint at cosmo.lan:8710; try: cargo run -p tools -- jev-probe health`.
  Beacon's "degrade to disabled with one `error!`" is right for telemetry and **wrong for a
  brain**: a fleet that silently runs q3 for 40 minutes while you believe you are measuring a
  model is a **voided experiment**, not a degraded one.
- **An individual answer that is late, flat, unresolvable, or malformed ⇒ silent q3, counted.**

Circuit breaker, **tiered by cause**: `refused` / 4xx / 5xx / model-slug-mismatch open it after N
consecutive; **timeouts get their own, higher threshold** — a busy-but-alive box must not trip a
5-second feature outage on an ordinary burst, and every open carries `err=` naming the cause.
Open ⇒ admit nothing for 5 s, keep serving last-good-or-none, `error!` **once**.
Name-resolution failure of `cosmo.lan` (usually mDNS/avahi or `/etc/hosts`; `getaddrinfo` does
not resolve mDNS) is a **startup refusal** under `require`, never a retry storm.

### Budget defaults (arithmetic shown; corrected in review)

| Knob | Default | Why |
|---|---|---|
| `max_input_tokens_per_s` | **1000** | **The primary knob.** A fleet bucket survives 8→36 bots without retuning; a per-bot rate silently rescales GPU load with `--count`. It is **≈26 % of the honest single-stream headroom (≈3.9k tok/s)** — ≈16 % of the hammered 8-concurrent ceiling, and that larger number is not the one to quote. |
| `tokens_per_req_target` | **350** (W5: **420**) | Sibling knob — the *rate* is derived, `fleet_rate = budget ÷ tokens_per_req` = 1000/350 ≈ **2.86 req/s** of capacity. |
| implied demand | **2.4 req/s** at 36 bots / 15 s | Demand < capacity ⇒ **the bucket is provably not the limiter** in the default config. 350 tok × 4/min × 36 = 50.4k/min = **840 tok/s**. |
| `min_dwell_s` | **15.0**, jitter **×[1.0, 1.3]** ⇒ 15.0–19.5 s | **One-sided, deliberately.** Symmetric ±30 % puts 30 % of bots at a 10.5 s floor ⇒ 36/10.5 = 3.43 req/s ⇒ **1 200 tok/s, over the knob** — i.e. the bucket becomes the thing that drops them, defeating the row's own reason for existing. One-sided keeps worst case 36/15 = 2.4 req/s and still decorrelates phase (the actual purpose, exactly as `rejoin_jitter` does). |
| `ttl_s` | **24.0** | Must satisfy **`ttl_s ≥ dwell_max + deadline + permit_wait + 1 s`** = 19.5 + 0.35 + 0.2 + 1 = 21.05 ⇒ 24. At the previously-stated 20.0 a *healthy* run can legitimately age past TTL and fire `EVT jev_stale`, which this row then calls a config error. With the inequality honoured, non-zero `jev_stale` **is** a config error. |
| `max_age_at_apply_s` | **8.0** | A **brain-side** knob, separate from hub TTL: at 36 bots the worst *legitimate* decision age is ~19.5 s, which is far too stale to steer 8 seconds of play with. Freshness-of-play and hub TTL are two questions. |
| `deadline_ms` | **350**, hard client-side cancel, **wrapping connect** | Single-request p95 is ≤150 ms. Caveat recorded honestly: the 8-concurrent probe's **mean was ≈346 ms**, so 350 is *not* comfortably above a loaded box — T9's bench must report whether p95 at the real demand rate stays under it, and if it does not, **raise dwell before raising deadline**. |
| `max_concurrent` | **3** | In-flight L = λ·W = 2.4 × 0.35 ≈ **0.84** using the *measured* mean (not an optimistic 0.15), so 3 is ≈3.6× headroom, not "a 7× burst". Overflow is **shed** (`skipped_no_capacity`, queue depth 1), never absorbed. Serialisation is already visible *at* 8; **measured, not `total_slots`.** |
| slot policy | **refresh ≤ 2, events ≤ 3** | Events may use all three permits but never more; refresh traffic can never starve a respawn ask. |
| `permit_wait_ms` | **200** | `timeout(200ms, sem.acquire())` — see the permit-wait bug below. |
| `snapshot_ttl_ms` | **1500**, from snapshot creation, re-checked **after** acquiring a permit | A **different knob from `deadline_ms`**: the deadline protects the worker from a hung socket; the TTL stops us spending a scarce permit asking about a dead world. |
| queue depth per bot | **1, newest wins** | A queue lets 4 obsolete snapshots per bot consume the whole budget on the past. **MUST NOT queue >1.** |
| `max_state_tokens` | **512 hard, 250–350 target** | Overflow drops the lowest-priority bucket **and logs which**. **This bounds the state only** — criteria templates + three region legends ride on top, so T10/T14 also assert a **total rendered-request** token budget, not just the state's. |
| questions per request | **all of them, one call** | ~8 ms marginal/question vs ~48 ms per 1 200 state tokens. The region `state` block is therefore **a section of the same request**, included when the region question is included — never a second call. |
| bucket accounting | **reserve `chars/4` at admission, true-up with `usage.input_tokens`, bucket may go negative** | The real count only exists after the response; charging post-hoc means `max_concurrent` × state can admit above 1000 tok/s before the first true-up. The fake-clock test must exercise *this* algorithm, not a different one. |

**W5 re-pricing (mandatory, at T15):** with the region block a full ask is ~420 tok, so
36 × 420 ÷ 15 s = **1 008 tok/s — over the knob**. `min_dwell_s` becomes **20** at W5
(36/20 = 1.8 req/s ⇒ 756 tok/s ✓), and `jev-probe bench`'s default rate must be **derived from the
live config**, not a hardcoded 2.8.

The permit-wait bug this prevents (neckbeard): if a worker blocks on `sem.acquire()` *before*
starting the deadline timer, permit-wait is unbounded, so by send time the snapshot is already
2–4 s old and you get a perfectly valid, perfectly on-time answer about a game state four server
frames dead. Both fixes are required: bounded acquire **and** age re-check after acquire.

`tokio::sync::Semaphore` is FIFO, so with 40 bots and 3 permits a bot that loses the race can be
systematically starved — which correlates with spawn/region and would look like a strategy
effect in the A/B. The jittered per-bot re-ask floor is the mitigation, and the hub's
priority-queue-with-tie-break-oldest is the structural fix.

### `state` shape, and keeping it diffable

Named buckets, **integers/bands only, no floats, never raw `Vec3`** (coordinates are ~15 tokens
each, high-entropy, and change every call, faking "the world changed"). Never: entity lists
(cap to `enemy n:2 near:180 vis:yes`), player names, configstrings, the nav graph, arrays of
history, the bot's own name, or monotonic counters (`serverframe`, `ticks`, `self.time` — the
model has no clock).

```text
# (a) objective + posture — target ~300 tok
self  hp:38(band:lo) armor:12 weapons:2/8 ammo:pistol:12|sg:0|roc:0 low:yes
dmg   recent:74 from:unknown since:3s
enemy n:2 near:180 vis:yes moving:yes
loc   region:4/6 node:84 floor:hi exits:2 seen:dock,mega
items quad:seen(300) mega:no ra:no health:seen(120)
map   q2dm1 phase:active gap:heavy_damage

# (b) region block — ~120 tok, a SECTION of the same request when the region question is asked
here  region:4 held:28s contested:yes foes:2 loot:quad:no mega:yes
alts  3:foes:0 loot:mega  7:foes:1 loot:ra  — no more than 3
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
`type`, an unknown extra field, `answers: {}`, and a close-with-zero-bytes. Plus: decision-table
tests `(constructed answer, ticket, brain state) → (applied | rejected, reason)` covering every
`NoSignalReason` arm — **constructed in Rust, in `brain`**; the JSON→struct leg is tested in
`crates/qbots`, which is where `serde_json` actually lives; request-render golden tests; and
transport framing tests against recorded bytes (**chunked**, truncated, non-200, refused,
slow-body timeout, >1 MiB, **model-slug mismatch**) replayed by a ~40-line
`tokio::net::TcpListener` speaking minimal HTTP/1.1 — **no mock library**. Because the server is
deterministic we can commit exact request+response pairs and get byte-exact client tests for free.

**Live (never CI; `cargo run -p tools -- jev-probe`).** Asserts **shape and direction, never
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

- **Hard gate: `git diff --exit-code crates/brain/Cargo.toml`.** Achievable: `n_eff`, the odds
  ratio, `AtomicU64`, `Arc`, `Mutex` are all std, and the JSON leg lives in qbots (T3).
- **`Cargo.lock`: no new *packages*.** Not "byte-unchanged" — T5 needs tokio's `test-util`
  feature and T9 needs `clap`/`serde_json` in `crates/tools`, and a direct dependency on an
  already-locked crate appends to that package's `dependencies` list in the lockfile. Both are
  allowed; **a new `name = "…"` entry is not.**
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
  `ctx.goal_override.is_some()` — which is exactly what makes the harnesses safe to run — a
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
`cargo test`). **Live-half rule for every task**: a task whose Verify has a live half is
`done (offline)` when the offline assertions pass, with a `_(pending live)_` row added to the
tracker's Evidence table — **a live half is a wave-gate condition, never a task-completion
condition.** Offline span: **T2–T8**. Then a human runs T9. Then **T10–T11**, then the W3 gate.
T1 and T13 are human-only; **T9 is entirely live** and is the handoff point, not a task to grind on.

### Wave 0 — live contract gate (no Rust, human)

### T1: Record the contract, the measurements, and settle three open questions

**File**: `context/distilled/jev.md` (new)

**What to do**: Write the `/v1/systemone` request/response contract and the measurement table
from *The endpoint, measured 2026-11-07* into `context/distilled/jev.md`, in the dense house
style (see `context/distilled/eraser.md`). Then resolve, in writing, the three unknowns that
change our design:

1. **Is `confidence` exactly `top1 − 1/n`?** Evidence: flat case computed 0.0093 vs reported
   0.0096. Ask the API owner, or determine it empirically with 5 constructed requests (2/5/24
   options). We compute our own gate statistic either way.
2. **Does `cosmo.lan` resolve from wherever qbots will run?** `getaddrinfo` does not resolve
   mDNS; a `/etc/hosts` entry on the dev box is not a contract. Record the IP and decide whether
   the config default is an IP with the hostname documented as an alternative.
3. **Is `/v1/systemone` a stable API or a stopgap?** The 501 wording ("use /v1/systemone") reads
   purpose-built. A one-line answer decides whether we pin the slug hard or version-tolerantly.

Any question that cannot be answered today gets a `~~strike~~ UNANSWERED (T1): <what was tried>`
line — never a silent deletion.

**Verify**: `context/distilled/jev.md` exists and contains the request/response shapes verbatim,
the measurement table, and three resolved-or-struck answers.
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
Also define the hardcoded `EntityClass → slug` table.

**Before**: *(no `jev` module exists)*

**After**:
```rust
// crates/brain/src/jev/state.rs  — closed data, no Serialize
pub const STATE_SCHEMA_VERSION: u32 = 1;
pub enum FieldKey { Self_, Dmg, Enemy, Loc, Items, Map, Here, Alts }
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

**What to do**: Implement `Decision<T>`, `NoSignalReason`, `DistStats`, and the gate functions
`n_eff(&[f32]) -> f32` and `odds_ratio(&[f32]) -> f32` with the thresholds from *Gates*. Define
the seam trait with **no `async fn`** (so `brain` stays tokio-free and `Brain: Send` + sync
`tick` are untouched):

```rust
pub trait DecisionSource: Send + Sync {
    fn latest(&self, key: DecisionKey) -> Option<Decision<AppliedDecision>>;
    fn submit(&self, s: DecisionState, trig: Trigger);
}
```

`NullDecider` (always `None`) is what `JevStatic` and every offline test use. `StubDecider`
replays a scripted list for brain tests. Validation: `probabilities` sum ≈ 1 within 1e-3,
`choice` ∈ the labels we sent, unknown response `type` ⇒ `Malformed` (**never** a silent default).
`score` is converted to a discrete level at parse time (`argmax(probabilities)`) and the float is
discarded except as a within-level tie-break.

**Before**: *(no gate logic exists)*

**Verify**: `cargo test -p brain -- jev_gate` — a table of `(constructed Answer/DistStats, ticket,
brain state) → (applied | rejected, reason)` covering **every** `NoSignalReason` arm, built **in
Rust**. `crates/brain` has **no `[dev-dependencies]` and no serde** (verified: 14-line
`Cargo.toml`, zero `serde_json`/`include_str` usages), so **no brain test may read a JSON
fixture** — the JSON→`Answer` leg is T4's, in `crates/qbots/tests/jev_net.rs`, where `serde_json`
is already a direct dependency. The canonical flat case is a hand-written probability vector
`{0.3164, 0.1040, 0.4911, 0.0885}` whose expected outcome is
`NoSignal { reason: Flat { n_eff: 3.20 } }`.
**Commit**: `task(T3): decision gate algebra (n_eff + odds ratio) and the DecisionSource seam`

### T4: Hand-rolled HTTP/1.1 POST + `/v1/systemone` wire types (zero new packages)

**File**: `crates/qbots/src/jev/net.rs` (new), `crates/qbots/src/jev/wire.rs` (new),
`crates/qbots/tests/jev_net.rs` (new), `crates/qbots/tests/fixtures/*.json` (new)

**What to do**: Types in `wire.rs` mirror the contract; `#[serde(default)]` on structs,
`Option<T>` for advisory fields (`model`, `usage`, `legend`), **required** for `type`/`choice`/
`probabilities`; **no `deny_unknown_fields`** (the server is independently versioned);
`#[serde(tag = "type", rename_all = "lowercase")]` with unrecognised variants mapped to
`Malformed`. In `net.rs`, one `POST` over `tokio::net::TcpStream`: `Content-Length`,
**`Connection: close` always** (one connection per request — deletes the whole keep-alive-pool
bug class for ~zero cost on LAN), handle **both** `Content-Length` and `Transfer-Encoding:
chunked`, `write_all` under backpressure, cap the response at **1 MiB** → `Malformed`, and wrap
**connect + write + read** in one `tokio::time::timeout(deadline)`. Validate the response:
`answers` keys **exactly equal** requested keys (reject the *whole* response on a missing **or
extra** key — partial application is where silent corruption lives), and assert `model` against
config, failing closed to the breaker on mismatch.

**Before**: *(no HTTP exists anywhere in the tree — `Cargo.lock` has zero http/tls crates)*

**Verify**: `cargo test -p qbots -- jev` against a ~40-line fixture-replaying
`tokio::net::TcpListener`: chunked body, truncated body, non-200, `400`, `501`, malformed JSON,
`answers: {}`, a **missing** answer key, an **extra** answer key, probabilities not summing to 1,
an **unknown response `type`**, a **model-slug mismatch**, connect-refused, a
**close-with-zero-bytes** response, and a slow-body timeout. Each asserts the specific
`Decision` variant (fallback class + reason), not "didn't panic" — a panicking test already
fails, so that phrase proves nothing. **No new `name = "…"` entry in `Cargo.lock`.**
**Commit**: `task(T4): hand-rolled HTTP/1.1 systemone client with framing and timeout coverage`

### T5: The hub — slots, transport seam, worker pool, fleet bucket, generation, breaker

**File**: `crates/qbots/src/jev/hub.rs` (new), `crates/qbots/src/jev/transport.rs` (new),
`crates/qbots/src/jev/mod.rs` (new), `crates/qbots/Cargo.toml` (add tokio `test-util` to
dev-deps/features)

**What to do**: One **process-wide** hub, mirroring `start_beacon` (`supervisor.rs:222-241`) for
lifecycle and shutdown.

- **Decision slot**: `struct DecisionSlot { id: AtomicU64, inner: Mutex<AppliedDecision> }` per
  bot index — **a `Mutex` of the *value*, not of an `Arc`**. The sync `Brain::tick` reader takes a
  ~64-byte copy under a lock held for a struct move; the writer assigns. No allocation on either
  side, one writer + one reader per bot, contention nil. A 64-byte copy at 10 Hz is free — **no
  fast-path id gate** (an external `AtomicU64` checked before the lock is two sync points that
  can disagree; add only if a profile asks). Slot lives in `crates/qbots` and is surfaced **only**
  through `Arc<dyn DecisionSource>`, so `brain` never learns `std::sync` exists.
  *(Review conflict resolved: `tokio::sync::watch` also works — `sync` is already an enabled
  feature and its newest-wins semantics fit — but it pulls tokio's API into the module that owns
  the slot and is awkward to read from a non-tokio test. `arc-swap` is **not in `Cargo.lock`** and
  is banned by the no-new-packages gate.)*
- **Transport seam**: `trait DecisionTransport` (`net.rs`'s client is its production impl, a
  scripted one its test impl). **Without this the fake-clock hub tests cannot drive a worker at
  all**, and it is the reason the seam is listed here and not "later".
- Inbound: newest-wins snapshot slot per bot (one atomic store on the bot side; **never a queue**),
  read by a pool of `max_concurrent` workers pulling from a **priority queue (event > refresh,
  tie-break oldest)** under **refresh ≤ 2, events ≤ 3**.
- Admission: the `max_input_tokens_per_s` bucket, **reserve `chars/4` at admission, true-up with
  the real `usage.input_tokens`, bucket may go negative**; `timeout(permit_wait_ms, sem.acquire())`;
  **snapshot-age re-check after acquiring** (drop without sending if past TTL).
- A hub-owned `AtomicU64` `generation`; per-bot dwell with **one-sided ×[1.0,1.3]** jitter; a
  **tiered** circuit breaker (`refused`/4xx/5xx/model-mismatch after N; timeouts on their own
  higher threshold; open ⇒ admit nothing 5 s, `error!` once); optional response cache keyed
  `hash(STATE_SCHEMA_VERSION ‖ rendered_state)`, **off by default**.

**Before**: *(none — this is the only place that ever touches the socket)*

**Verify**: `cargo test -p qbots -- jev_hub` with `tokio::time::pause`/`start_paused` (needs the
`test-util` feature) **and the scripted transport**: the bucket limits to config using the
reserve/true-up algorithm above (not post-hoc charging); a bot's second submit **replaces** its
snapshot (assert one in flight per bot); a permit wait over `permit_wait_ms` counts
`skipped_no_capacity`; a snapshot older than TTL is dropped **after** acquiring and never sent;
events are never starved by refresh bursts and never exceed 3; the breaker opens for
`refused`/5xx/model-mismatch and **does not** open on isolated timeouts; `generation` bump
invalidates every in-flight answer; a decision slot read from a non-tokio thread returns the
last write.
**Overrun note**: eight interacting mechanisms, each with a test — this is the plan's schedule
risk. Cut the response cache from this task first if it slips.
**Commit**: `task(T5): process-wide jev hub — bucket, transport seam, reserved slots, newest-wins`

**W1 EXIT GATE** *(recorded in the tracker before T6 starts)*: `cargo build`/`clippy -D warnings`/
`test` green with **no new packages in `Cargo.lock`** and `crates/brain/Cargo.toml` byte-unchanged,
every test above passing with no network available.

---

### Wave 2 — the q3 seam, the brain kind, and the probe tool

### T6: `GoalPolicy` seam inside q3 — behaviour-preserving

**File**: `crates/brain/src/brains/q3/policy.rs` (new), `crates/brain/src/brains/q3/mod.rs`
(edit: `:211`, `:407`, `:421`, `:506`, `:890-891`, `:904-930`, constructor)

**What to do**: Introduce the seam with **`&self` methods and a `Send + Sync` trait**, and q3
holding `Arc<dyn GoalPolicy>`:

```rust
pub trait GoalPolicy: Send + Sync {
    /// A replacement goal for the roam fallback arm. `&self`: q3 owns an Arc, and the
    /// wrapper in T7 must hold a second Arc clone to feed it — &mut here would make the
    /// policy unreachable without Rc<RefCell<_>>, which breaks Brain: Send.
    fn objective(&self, q: &GoalQuery) -> Option<NavGoal> { let _ = q; None }
    fn posture(&self, q: &PostureQuery) -> Posture { Posture::default() }
    fn set_map(&mut self, _map: &BrainMap) {}   // called from Q3Brain::set_map
}
pub struct NullPolicy;                                  // every method default => today's behaviour
pub struct Posture { pub wants_retreat: Option<bool>, pub wants_chase: Option<bool> }
```

`Q3Brain::new_with_policy(Arc<dyn GoalPolicy>)` added; existing `new()` delegates with
`Arc::new(NullPolicy)`. **The posture merge must be conservative-only — it may only ADD caution
and only REMOVE aggression** (`Posture::default()` = both `None`):

**Before** (`crates/brain/src/brains/q3/mod.rs:890-891`; `:506` is the second site, the
sneak-past veto inside enemy selection, and **must route through the same `Posture`** or a
`disengage` is half-wired and the two sites can disagree):
```rust
        let wants_retreat = crate::q3char::wants_to_retreat(view, &self.ch, hdelta);
        let wants_chase = crate::q3char::wants_to_chase(view, &self.ch, hdelta);
        …
                            _ => self.roam_goal(view, ticks, pos),
```

**After**:
```rust
        let h_retreat = crate::q3char::wants_to_retreat(view, &self.ch, hdelta);
        let h_chase   = crate::q3char::wants_to_chase(view, &self.ch, hdelta);
        let post = self.policy.posture(&PostureQuery { view, ch: &self.ch, hdelta });
        let wants_retreat = h_retreat || post.wants_retreat.unwrap_or(false); // add caution only
        let wants_chase   = h_chase   && post.wants_chase.unwrap_or(true);    // remove aggression only
        …
                            _ => self.policy.objective(&GoalQuery { view, ticks, pos })
                                       .unwrap_or_else(|| self.roam_goal(view, ticks, pos)),
```
`NullPolicy` ⇒ `false` / `true` ⇒ byte-identical to `h_retreat` / `h_chase`. (Borrow-checker:
`Posture` returns by value so the `&self.policy` borrow ends at the semicolon; `roam_goal(&mut
self)` is a disjoint-field borrow from `self.policy`. No problem either way — the constraint is
the *bias direction*, not the borrow.)

**Verify**: (a) all **12** existing `#[test]`s in `q3/mod.rs` pass **unedited**; (b) a
**200-tick parity test written inside `q3/mod.rs`'s own `#[cfg(test)] mod tests`** — where
`view_enemy` (`:1064`) / `view_enemy_at` (`:1147`) and `new_with_policy` are already visible —
driving `Q3Brain::new()` and `Q3Brain::new_with_policy(NullPolicy)` over the same scripted frames
and asserting identical `MovementIntent`/`BrainOutput` every tick (feasible because both brains
are deterministic: shared LCG `roll()` at `:201-207`, and `JitterRng::new` seeded from
`(self.time * 1000.0) as u32 ^ self.rng_state ^ (n as u32)` at `:675-676` — both brains see the
same `n`, so the parity holds). **No extraction to `crates/brain/tests/common`** — that is
unnecessary risk on crate-private `Worldview` construction; `crates/brain/tests/q3_brain.rs`
already covers the external-API leg; (c) **live no-op A/B** (human): `q3,jev_static` after T7 is
the live control — **do not use `--brains q3,q3` as the inertness proof** unless you have shown
`matrix_specs` (`supervisor.rs:584-610`, no dedup) fields two *distinguishable* groups, since
identical arms collide on `group_tag`/bot names (Plan 54 name-coding, `FleetStats` prefix
grouping).
**Commit**: `task(T6): extract q3 GoalPolicy seam (behaviour-preserving) + parity test`

### T7: `JevBrain`, `BrainKind::{Jev,JevStatic}`, config `[jev]`

**File**: `crates/brain/src/brains/jev/mod.rs` (new), `crates/brain/src/brains/mod.rs`
(`BrainKind` `:35`, `brain_tag` `:58`, `build_brain` `:76-83` + a new sibling constructor),
`crates/qbots/src/config.rs` (the `BeaconCfg` optional-block pattern at `:36-63`,
`[fleet].brain` at `:132-143`), `crates/qbots/src/config.example.yaml`,
`crates/qbots/src/roster.rs` (brain-token validation `~:109-123`),
`crates/qbots/src/supervisor.rs` (`brain_code` `:506-515`)

**What to do**: `JevBrain { inner: Q3Brain, policy: Arc<JevPolicy> }` — a ~120-line composition,
**no copy of q3**. It holds a **second `Arc` clone of the same policy** q3 holds, which is why
T6's trait is `&self`/`Send + Sync`; `JevPolicy` owns its `Arc<dyn DecisionSource>` clone and
does `latest()` inside q3's tick, and keeps its **own read-only copy of `BrainMap`** via
`GoalPolicy::set_map` because `Q3Brain::set_map` **discards `items` by design** (`:841`) and
candidate resolution needs them. `JevBrain::status()` **delegates verbatim to `inner.status()`**
— no `"jev:"` prefix, or T12's `assert status() == "retreat"` (`Q3Node::label`, `q3/mod.rs:63`)
and the `build_brain → status()` pattern in `crates/brain/tests/q3_brain.rs` both break.
`JevStatic` = the same construction with `NullDecider`, as the primary A/B control.

**`build_brain` cannot receive a `DecisionSource`** as it stands (`brains/mod.rs:76-83` takes
`kind/skill/cfg/char/persona/xonchar`). Add a sibling
`build_brain_with_decider(kind, …, Option<Arc<dyn DecisionSource>>)` used by the **two** production
call sites (`main.rs:972`, `scenario.rs:544`); leave `build_brain`'s signature and its **7** other
call sites (`brains/mod.rs` tests ×3, `crates/brain/tests/q3_brain.rs` ×4) untouched.

`[jev]` config block: `enabled`, `endpoint`, `model`, `require` (**default `true` when the block
is present**), `max_input_tokens_per_s`, `tokens_per_req_target`, `max_concurrent`,
`deadline_ms`, `permit_wait_ms`, `snapshot_ttl_ms`, `min_dwell_s`, `dwell_jitter_max`, `ttl_s`,
`max_age_at_apply_s`, `max_state_tokens`, `debug`, `debug_sample`, `cache`. `#[serde(default)]` +
`Default` so an existing `config.yaml` behaves exactly as before. **Config-only: no
`--jev-endpoint` flag** (two sources of truth for an endpoint is a support ticket);
`JEV_ENDPOINT` env is honoured **inside `jev-probe` only**, and T9's probe gets a `--config <path>`
so an operator can point the probe at the fleet's exact file.
`brain_code` (`supervisor.rs:506-515`) feeds `group_tag` and Q2's **15-char `netname`** budget —
use short codes `jev` / `jvs`, not `jev_static`.

Startup: probe `health`, assert the returned slug against `model`, and on failure with
`require = true` **refuse to start** with the actionable message in *Failure policy*.

**Before** (`crates/brain/src/brains/mod.rs:35`):
```rust
pub enum BrainKind { Main, Q3, Zb2, Xon /* … */ }
```
**After**: `Jev` and `JevStatic` arms + their `brain_tag` strings + `build_brain_with_decider`.

**Verify**: `cargo test -p qbots` — config absent ⇒ `enabled: false`, no task spawned, no socket;
`require=true` + unreachable endpoint ⇒ startup error containing `jev-probe health`
**(drive it against a closed port `127.0.0.1:1` — the box must not be needed for this test)**;
`require=false` ⇒ one `error!` then behaves as `JevStatic`; `BrainKind::from_str("jev")` and the
roster brain token both accept `jev`; `--brains q3,jev_static,jev` parses; `brain_code` values are
≤ 15 chars as `netname`s.
**Commit**: `task(T7): jev + jev_static brain kinds, [jev] config block, startup requirement gate`

### T8: Hub lifecycle in the bot loop + invalidation hooks

**File**: `crates/qbots/src/main.rs` (`bot_task` at `:877`, `build_brain` at `:972`, the `select!`
at `:1081`, `brain.tick` at `:1714`, `on_death` at `:1398`/`:1592`, the servercount branch at
`:1365-1412`), `crates/qbots/src/supervisor.rs` (**both** `start_beacon` call sites — `:323` and
**`:682`**, inside `FleetShared`, the fleet/competition path), `crates/brain/src/brains/core.rs`
(`trait Brain` `:108-126`)

**What to do**: Start one hub per process **at both shared-fleet spawn points** — wiring only
`:323` leaves `competition` with **no hub**, so T11's own A/B would silently run jev with no
decider and report zero applied forever. Give each bot an index-keyed slot pair and hand the
brain an `Arc<dyn DecisionSource>`. **No `.await` and no HTTP in the ticker arm**
(`main.rs:946` is `time::interval(100ms)` with **no `MissedTickBehavior` call anywhere**, so
tokio's `Burst` default is live; `main.rs:1180-1186` is Plan 57's ack-on-arrival and the keepalive
gap is 90 ms — a stall there perturbs the one number the repo most carefully engineered, for all
36 bots). Bots only ever `submit(snapshot)` (one store) and `latest()` (one lock + copy). Add a
**new hook** rather than overloading `on_death`, which `main.rs:1398` fires on servercount change
*and* on every respawn:

**Before** (`crates/brain/src/brains/core.rs:108-126`): `trait Brain: Send` has `set_map`,
`on_death`, `status`, `note_*` — no level-change hook.
**After**:
```rust
    /// A level change (not a respawn): the map identity may or may not have changed.
    fn on_map_change(&mut self, _map: &str) {}
```
called at `main.rs:1398` **in addition to** `on_death`, inside the servercount branch — check the
branch's existing ordering still assigns `map_servercount = servercount` (`:1412`) *after* the
hooks. Invalidate by `(map_name, region_fp)` so a same-map restart (identical BSP, graph reloaded
from `nav_cache`) keeps a still-valid plan. Never compare servercounts with `<`/`>`
(`beacon.rs:107-110` explains why). Submit event triggers from `on_death` (`:1592`), the
level-change hook, and the pickup / stall prints. **jev is inert while `ctx.goal_override.is_some()`**
(`core.rs:56`) so the `spawn-to-*` harnesses stay runnable — see *Baselines* for what that gate
therefore cannot prove.

**Verify**: `cargo test -p qbots` — a decision from a previous `generation` is never applied; a
decision whose `map_name` differs is never applied; a label not in the ticket's label set is
never applied; `goal_override = Some` ⇒ no decision applied (**relabelled honestly: this is a
guard test — `JevStatic` is inert by construction, so it proves the guard and nothing else**).
Plus a **live single-bot smoke** (human): `connect-one` with `[jev]` enabled against the box, and
the same with `require=true` and the box unreachable ⇒ **refuses to start**.
**Commit**: `task(T8): wire the jev hub at both fleet spawn points, add on_map_change, gate on goal_override`

### T9: `jev-probe` — the handoff point, entirely live

**File**: `crates/tools/src/bin/jev_probe.rs` (new), `crates/tools/Cargo.toml` (`[[bin]] name =
"jev_probe"` beside `bsp_verify`/`navinspect`/`gridscan`/`compgaps`/`acceptance`; note tools
currently has **no `clap`/`serde`/`serde_json`** — either hand-roll argv like `acceptance` does or
add them, and expect `Cargo.lock` to gain per-package dependencies, which the gate allows)

**What to do**: Subcommands: `health` (health + model slug + slot count + the **501 capability
fingerprint** on `/v1/chat/completions`); `ask --state @fixtures/x.txt` (typed answers +
probabilities + our computed `n_eff`/odds-ratio + `usage.input_tokens` + latency); `bench
--bots N --rate R --concurrent C` (**rate defaults derived from the live `[jev]` config, not a
hardcoded 2.8**; prints p50/p95/p99, aggregate input tok/s against **both** the ≈3.9k single-stream
and ≈6.2k hammered figures, and whether p95 stays under `deadline_ms`); `sensitivity` (perturbed
pairs differing in exactly one bucket ⇒ the distribution must move); `anticorrelation` (two
materially different states must **not** be byte-identical); `flatness` (deliberately meaningless
legends ⇒ assert our gates reject); `matrix --live` (~40 named scenarios) **plus `matrix
--replay`, which re-asks historical prompts and diffs "what it said then" vs now** — the cheapest
detector of a server restart changing the readout, which is risk 6. Flags: `--config <path>`
(read the fleet's exact `[jev]` block; `JEV_ENDPOINT` env override honoured **here only**) and
`--json` (this output is pasted into a markdown table by hand). Every subcommand **harvests** its
raw request+response into `context/fixtures/jev/`.

**Verify**: **live, human-run**: `jev-probe health` prints the slug; `sensitivity` moves **both**
perturbed pairs (full-health+mega vs 20 hp + no armor; near-item vs far-item); `anticorrelation`
passes; `flatness` reports rejection; `bench` prints p50/p95/p99, tok/s against both figures, and
the p95-vs-`deadline_ms` verdict. Record every number in the tracker's Evidence table.
**W2 EXIT GATE.**
**Commit**: `task(T9): jev_probe — health/ask/bench/sensitivity/anticorrelation/flatness/matrix`

**W2 EXIT GATE** *(human-run; recorded before T10)*: all five live checks pass and are pasted in
the tracker. Additionally: with the endpoint unreachable, `--brains q3,jev_static,jev` — jev plays
**identically to jev_static** — and the `q3`-vs-`jev_static` K/D gap is within the repo's noise
floor. **If `sensitivity` fails for a question, that question does not get built** (see *Gates*).

---

### Wave 3 — one `objective` question, event-triggered

### T10: Objective question + candidate generation + event triggers

**File**: `crates/brain/src/jev/objective.rs` (new), `crates/brain/src/jev/state.rs` (edit),
`crates/brain/src/brains/jev/policy.rs` (edit), `crates/qbots/src/jev/hub.rs` (edit: triggers,
dwell/jitter)

**What to do**: Code enumerates, the model judges. **Candidate generation lives in `JevPolicy`**,
not in a `GoalQuery` — `NavGoal::Waypoint(node)` needs `BrainMap::roam_nodes` and
`NavGoal::Position` needs `MapItem` origins, **neither reachable from `&Worldview`**, so the
policy resolves candidates from the `BrainMap` copy it keeps via `set_map` (T6/T7). Prove
reachability and relevance, rank by a cheap heuristic, offer **at most 3**, and resolve the
returned label → `NavGoal` inside the policy so an unresolvable label is impossible by
construction. Criteria text is generated from a fixed template with every fact **also present in
`state`**: hardcoded landmark slug, item classes as **enum names**, A\* distance **bucket** from
me, my loadout gap (`wants:heavy_damage`), a terrain word, local heat bucket. Ask on the event
triggers (each ≥2 s persistent) with **one-sided** jitter, plus low-priority dwell refresh.
Apply only as the `roam_goal`-fallback substitution.

**Before** (`crates/brain/src/brains/q3/mod.rs:926`): the fallback arm is always `roam_goal`.
**After**: `policy.objective(..)` may supply a goal from the candidate set; otherwise identical.

**Verify**: `cargo test -p brain -- jev_objective` — a `StubDecider` naming candidate *k* drives
the navigator to that goal; a `StubDecider` returning a label **outside** the candidate set
produces `NoSignal(UnknownLabel)` and pure-q3 behaviour; `JevStatic` is unaffected; the rendered
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
EVT jev_decision obj=? post=? reg=? conf=? neff=? oratio=? age_t=? src=model|fallback|stale
                   lat_ms=? st_tok=? st_hash=? keys=? q=? trig=?
EVT jev_drop | jev_stale | jev_unresolvable | jev_budget | jev_breaker_open    … err=?
EVT jev_fleet applied_min=? admitted_min=? asked_min=? inflight=? bucket_toks=?
                tok_in_10s=? max_region_bots=?
```

`err=` names the enum (`timeout` / `refused` / `http_501` / `model_mismatch` / `breaker`) —
without it "box down", "deadline too tight" and "someone swapped the model" are one symptom.
`keys=` makes per-question denominators computable from the log alone. `st_hash` is 6 hex.

Final report, **per brain arm**: `asked`, `admitted`, `applied`, **`applied/asked` (headline)**,
**`gate_reject_rate`**, **`admission_drop_rate`**, all **split by `(question key × trigger class)`**
(the W3 gate is per-trigger and `trig=` is the only thing that makes it computable), fallback /
stale / dropped / unresolvable, decision-age p50/p95 **at apply time**, `n_eff` histogram,
`distinct_choices`, `decisions_that_changed_per_bot_per_min`, and the **herd metric**: max
concurrent bots per region + the occupancy histogram. **`competition` hard-fails if the jev arm's
applied count is 0 — printing the report FIRST**, or the W3 gate run destroys its own evidence.
Optional `[jev] debug = true` + `debug_sample = 20` = **1-in-20** sampled prompt+answer dumps to
`logs/jev/` (gitignored).

**Ordering note**: this task is the one reviewers would move to immediately after T7, because a
human arriving at the box to run T9's gate is under-instrumented if the in-process counters don't
exist yet. If it isn't moved, T9's `bench` is the operator's only window during W2.

**Verify**: `cargo test -p qbots -- jev_telemetry` — a scripted run yields the exact tallies and
all three ratios, split by key and trigger; **prove the guard is live by feeding the report a
zero-applied tally and asserting a non-zero exit** (removing an assertion cannot make a test
fail, so that phrasing is not a proof); `cargo run -p qbots -- competition --brains
q3,jev_static,jev` prints all three arms' counts.
**Commit**: `task(T11): jev telemetry — EVT jev_fleet, applied/asked + gate/admission rates, herd`

---

### Wave 4 — engagement posture (commitment, not targeting)

### T12: `posture` question + bounded application + dwell/commitment

**File**: `crates/brain/src/jev/posture.rs` (new), `crates/brain/src/brains/jev/policy.rs`
(edit), `crates/brain/src/brains/q3/policy.rs` (edit if `Posture` grows)

**What to do**: A `choice` over **{engage, trade, disengage}** plus a **`noul` canary** ("is there
enough in this state to prefer one of these?") — validated against the captured flat fixtures
before it is allowed to be load-bearing, and **cuttable** (explicitly not load-bearing). Apply
**only** through the `Posture` merge from T6 — `disengage ⇒ wants_retreat = true`,
`engage ⇒ wants_chase = false`-capable, **never** the reverse — at **both** call sites
(`q3/mod.rs:506` and `:890-891`). Target selection stays entirely local and per-tick (q3's
`battle_goal` re-picks it, and a stale remote preference over a PVS set is the every-tick
oscillation the Blocks.ai write-up saw). Posture carries its own dwell and `max_age_at_apply_s`
so a decision boundary never teleports the FSM: hysteresis on entry and exit, released instantly
on the stall detector, on death, and on a level change.

**Before** (`q3/mod.rs:890-891`, `:506`): the booleans come from `q3char` alone (T6's shape).
**After**: `Posture { wants_retreat: Option<bool>, wants_chase: Option<bool> }` merged via
`\|\|` / `&&` per T6 — permitted only to add caution and remove aggression.

**Verify**: `cargo test -p brain -- jev_posture` — a stub `disengage` forces `status()` to
`"retreat"` on contact (`Q3Node::label`, `q3/mod.rs:63`; `JevBrain::status()` delegates verbatim
per T7); a stub `engage` **cannot** produce a chase where `q3char` wanted retreat; the
"enemy leaves PVS while a posture is pinned" scenario still reaches `SeekLtg` — exits at
`:576`, `:598-599`, `:607`, `:614`, gated on `enemy_alive` (`:350`) — **i.e. no frozen bot**; both
posture sites agree in every scripted scenario. **Live, human-run**: publish
`decisions_that_changed_per_bot_per_min` — **> 2 posture flips per bot per minute is an
investigation** — and the composition check (`spawn-to-spawn` ≥ 13/24, no `max_speed > 320`),
noting in the report that this exercises the inner `Q3Brain`, not the jev layer.
**Commit**: `task(T12): posture question — conservative-only bias on chase/retreat, with dwell`

---

### Wave 5 — area control (fleet-coach carve-out) and acceptance

### T13: The AGENTS.md amendment — **human approval required before T15**

**File**: `AGENTS.md` (Concurrency Model section)

**What to do**: Propose the exact wording for the one deliberate exception the user chose, e.g.:
the **decider hub** (`crates/qbots/src/jev/hub.rs`) is an *external coach*, not a bot's
perception; it may read a fleet-level summary and hand each bot a distinct region; **no bot's
`Brain::tick` may read another bot's state**, and every brain other than `jev` remains
fleet-blind. Record alongside it that `--brains jev` is therefore **not strictly comparable** to
fleet-blind brains, and that `jev_static` is the control that absorbs it. Mirror the note in
`beacon.rs`'s module doc, which is the existing precedent for walking this line.

**Verify**: the human has approved the specific diff (recorded in the tracker with date), and
`grep -n "carve-out\|decider" AGENTS.md` shows it in the Concurrency Model section.
**Commit**: `task(T13): AGENTS.md — the jev fleet-coach carve-out, with its non-comparability note`

### T14: Per-map region tables + move generation

**File**: `crates/qbots/src/jev/regions.rs` (new), `crates/qbots/src/config.example.yaml`
(edit), `crates/brain/src/jev/state.rs` (edit: `Here`/`Alts` buckets),
`crates/brain/src/jev/objective.rs` (edit: legend text generation)

**What to do**: Hand-authored regions in config — **4–6 per map**, each `name` slug + `anchor`
node + item classes — resolved to existing `roam_nodes`/`MapItem` origins; validate at load that
every anchor resolves to a real nav node and **refuse to start** naming the bad one. Legends name
regions by **contents** (`r4 = dock + mega armor`), never bare indices — the flat-distribution
measurement says an unnameable option set cannot be answered. Region questions are asked over
**≤3 offered moves** out of the 4–6 known, in the **same request** as everything else.

**Verify**: `cargo test -p qbots -- jev_regions` — q2dm1's table loads, all anchors resolve; a
bogus anchor produces a startup error naming it; the rendered region question has ≤3 options and
every option's description contains a fact that also appears in `state`; **the golden asserts the
W5-sized TOTAL request budget (~420 tok)**; **the `ignore previous instructions` injection test
runs again here**, against a state built from a worldview whose **BSP-sourced item classnames**
carry it, asserting a byte-identical rendered request. Live `flatness` for this question shape =
**human-run**.
**Commit**: `task(T14): hand-authored per-map region tables and ≤3-move region questions`

### T15: Fleet-coach region assignment + herd control

**File**: `crates/qbots/src/jev/hub.rs` (edit), `crates/brain/src/jev/state.rs` (edit),
`crates/qbots/src/jev/telemetry.rs` (edit), `crates/qbots/src/config.example.yaml` (edit)

**What to do**: Inside the hub (the carve-out's home), maintain a fleet summary — per-region bot
count, last-assignment times — and ask **one fleet-level region question** per dwell rather than
one per bot (fewer requests, and the only version structurally incapable of producing a herd).
Distribute distinct assignments; enforce a hard per-region cap; a bot whose assignment is revoked
falls back to its own objective decision (never idles). **Re-price the budget here**:
`tokens_per_req_target` 350 → **420**, `min_dwell_s` 15 → **20** (36/20 = 1.8 req/s ⇒ 756 tok/s
✓), and re-check the `ttl_s` inequality. Record the response-cache decision (on/off) with
reasoning — caching guarantees identical answers for similar bots; persona prompts defeat it.
Extend the report with the occupancy histogram and max-concurrent-per-region.

**Verify**: `cargo test -p qbots -- jev_fleet` — 36 simulated bots never exceed the per-region
cap; assignments are distinct while slots allow; a revoked assignment yields a valid q3 goal the
same tick; the budget re-pricing keeps worst-case demand under `max_input_tokens_per_s` (assert
the arithmetic in a test). **Live, human-run**: a 12-bot run whose report shows a **non-degenerate**
region histogram — one region holding 12 bots is a failure of *the layer*, not of the map.
**Commit**: `task(T15): fleet-coach region assignment, per-region cap, herd histogram, budget re-price`

### T16: Acceptance A/B + docs + brain notes

**File**: `docs/BRAINS.md` (§6 "Adding a brain" checklist + switch tables),
`crates/qbots/src/config.example.yaml`, `context/brain_notes.md` (dated append — **mandatory** for
brain plans), `context/pitfalls.md`, `context/acceptance.md` (if the A/B command shape changes)

**What to do**: Run the pre-registered A/B — arms `q3`, `jev_static`, `jev`; **N ≥ 5** runs each
(the repo's noise-floor rule); paired by `(map, servercount)`; primary metrics decision-sensitive
(`time_in_target_region`, latency-to-a-named-item, status churn, region entropy, `applied/asked`);
frags/K/D secondary; **including at least one metric predicted to show no difference**. Then write
the docs, append the dated `brain_notes.md` section, and add `context/pitfalls.md` entries for at
least: *brain-rebuilt-per-session ⇒ generation must be hub-owned*; *the two clocks*
(`self.time += dt` vs `Instant`); *determinism ⇒ herding*; *flat distributions are a property of
the question, not the model*; *the permit-wait staleness bug*; *`.lan` is not resolvable by
`getaddrinfo`*; *one jitter direction is a budget constraint, not a style choice*.

**Verify**: the A/B numbers are in the tracker's **Evidence** table (command, build, result, run
count, spread); `docs/BRAINS.md` §6 contains `jev`; `context/brain_notes.md` has a
`## 2026-…` section for this plan; the named `context/pitfalls.md` entries exist.
**Commit**: `task(T16): jev acceptance A/B, BRAINS.md + brain_notes + pitfalls`

---

## Dependency matrix

```text
T1 (human, parallel)                       ┌─▶ T9 (LIVE, W2 gate) ─┐
T2 ─▶ T3 ─▶ T4 ─▶ T5 ─▶ [W1 gate] ─▶ T6 ─▶ T7 ─▶ T8 ──────────────┘
                                     (T6 is independent of T2–T5)  │
                                                                  ▼
                                              T10 ─▶ T11 ─▶ [W3 gate]
                                                              │
                     T13 (human) ─▶ T14 ─▶ T15 ◀──────────────┘
                                            │
                                            └─▶ T12 ─▶ T16
```

- **Offline span T2–T8**; **T9 is the live handoff**; then **T10–T11**; then the W3 gate.
- **T6 is independent of T2–T5** and is the best first task for a fresh session: it is verifiable
  against the 12 tests that already exist, with zero new scaffolding. It touches live play, so it
  gets its own commit and its own inertness proof.
- **T11 may move to immediately after T7** (see the task's ordering note) so the W2 live session is
  instrumented.
- **T12 and T14/T15 are independent of each other** but both need the W3 gate.
- **T13 gates T15, not T14** (the tables are just data).

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/brains/q3/mod.rs` | `GoalPolicy` seam at `:506`, `:890-891`, the `roam_goal` fallback `:926`, `set_map`; **behaviour-preserving** | P0 |
| `crates/brain/src/brains/q3/policy.rs` | `GoalPolicy: Send + Sync` with `&self` methods, `NullPolicy`, `Posture` | P0 |
| `crates/brain/src/brains/jev/{mod,policy}.rs` | `JevBrain` (composes `Q3Brain`, second `Arc` of the policy), `JevPolicy` (owns the `DecisionSource` + its own `BrainMap` copy) | P0 |
| `crates/qbots/src/jev/hub.rs` | bucket (reserve/true-up), workers, refresh≤2/events≤3, newest-wins slots, generation, tiered breaker | P0 |
| `crates/qbots/src/jev/transport.rs` | the seam the fake-clock tests need | P0 |
| `crates/brain/src/jev/decision.rs` | the gates (`n_eff`, odds ratio) and `Decision`/`NoSignalReason` | P0 |
| `crates/brain/src/jev/state.rs` | closed vocabulary, builder, `STATE_SCHEMA_VERSION`, class→slug table | P0 |
| `crates/qbots/src/jev/net.rs` | HTTP/1.1 POST; chunked + `Connection: close` + timeout-wraps-connect | P0 |
| `crates/qbots/src/main.rs` + **`supervisor.rs` both spawn points** | hub wiring, `on_map_change`, no I/O in the ticker arm | P0 |
| `crates/brain/src/brains/mod.rs` | `BrainKind` arms + `build_brain_with_decider` (sibling; 7 existing call sites untouched) | P1 |
| `crates/brain/src/brains/core.rs` | `fn on_map_change(&mut self, &str)` default hook | P1 |
| `AGENTS.md` | the fleet-coach carve-out (**needs human approval**) | P1 |
| `crates/qbots/src/jev/render.rs` | exhaustive renderer — the security boundary | P1 |
| `crates/tools/src/bin/jev_probe.rs` | the live gate; every live number in this plan comes from here | P1 |
| `crates/qbots/src/config.rs` | `[jev]` optional block, `require` | P1 |
| `crates/qbots/src/jev/telemetry.rs` | `EVT jev_fleet`, the three ratios, herd metric, zero-applied hard-fail | P1 |
| `crates/qbots/src/jev/regions.rs` | 4–6 region tables, anchor validation | P2 |
| `docs/BRAINS.md`, `config.example.yaml`, `context/brain_notes.md` | operator-facing docs | P2 |

## Open Questions / Risks

1. **Applied-rate may be near zero by construction.** Our one real captured answer already has
   `n_eff = 3.20/4`. If typical states look like that, most answers are *correctly* gated to q3
   and the measured effect of the whole feature is zero for legitimate reasons. *Mitigation*: the
   W3 gate is a measurement with **separate gate-reject and admission-drop ratios**, event-triggered
   asking exists specifically to raise state discrimination, the sensitivity probe deletes
   unanswerable questions, and T11 publishes the rates before W4/W5 spend effort.
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
5. **`cosmo.lan` may not resolve from the runner** (mDNS vs `getaddrinfo`). *Mitigation*: T1
   settles it; `require = true` turns it into a startup refusal rather than a silent q3 run;
   recommend an IP default with the hostname documented.
6. **The API may be a stopgap.** The 501 wording reads purpose-built; a server change could
   delete `/v1/systemone` wholesale. *Mitigation*: `jev-probe health` asserts the 501
   fingerprint + slug at startup, `matrix --replay` detects a server-side change, and the run
   header logs the server-reported model, so a silent swap surfaces in the report, not the scoreboard.
7. **`confidence` semantics undocumented.** If it *is* `top1 − 1/n`, using it would couple our
   thresholds to a server internal. *Mitigation*: we compute our own; T1 asks, and if unanswered
   we still log both and never gate on theirs.
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

## Verification Checklist

- [ ] T1: `context/distilled/jev.md` holds the contract, the measurement table, and three resolved-or-struck answers
- [ ] T2: golden render byte-exact; 100× re-render byte-equal; over-budget drops the named bucket; `crates/brain/Cargo.toml` byte-unchanged
- [ ] T3: every `NoSignalReason` arm covered by a **Rust-constructed** table test (no JSON in brain); the flat vector yields `NoSignal{Flat(n_eff≈3.20)}`
- [ ] T4: chunked / truncated / 400 / 501 / malformed / empty / **missing key** / **extra key** / bad-sum / unknown type / **model mismatch** / refused / zero-bytes / slow-body each yield the specified `Decision` variant; no new packages in `Cargo.lock`
- [ ] T5: scripted-transport + `start_paused` tests prove reserve/true-up limiting, one-in-flight-per-bot replacement, `skipped_no_capacity`, post-acquire TTL drop, events ≤ 3 and never starved, **tiered** breaker (isolated timeouts do not open it), generation invalidation, slot read off-runtime
- [ ] T6: the 12 existing q3 tests pass **unedited**; 200-tick `NullPolicy` parity **inside q3's test module**; the posture merge is `\|\|`/`&&` (add-caution / remove-aggression); `:506` routed through the same `Posture`
- [ ] T7: absent `[jev]` ⇒ no task, no socket; `require=true` + `127.0.0.1:1` ⇒ startup error naming `jev-probe health`; `require=false` ⇒ one `error!` then jev-identical-to-`jev_static`; `build_brain`'s 7 other call sites untouched; `brain_code`s fit the 15-char `netname`
- [ ] T8: stale-generation / wrong-map / off-ticket labels never applied; the `goal_override` guard test passes **and is labelled a guard**; hub started at **both** `supervisor.rs:323` and `:682`; `map_servercount` assignment still last in the branch
- [ ] T9: live `health` (slug + 501 fingerprint), **both** sensitivity pairs move, anti-correlation, flatness rejected, bench p50/p95/p99 + tok/s vs **both** figures + p95-vs-deadline verdict — all in the Evidence table *(W2 gate)*
- [ ] T10: stub candidate *k* drives the navigator to that goal; out-of-set label ⇒ `NoSignal(UnknownLabel)` + pure q3; golden matches **total** request budget; **injection through the real pipeline renders byte-identical**
- [ ] T11: tallies + all three ratios split by `(key × trigger)`; `EVT jev_fleet` every ~10 s with `err=`/`keys=`; **zero-applied tally ⇒ non-zero exit with the report printed first**
- [ ] W3 gate: the exact `--count 12` command run ≥2×; `applied/asked`, `gate_reject_rate`, `admission_drop_rate` published per arm and per key; **gate_reject ≥ 85 % on event asks ⇒ fix the state or delete the question before T12; admission drops ⇒ raise dwell, never loosen thresholds**
- [ ] T12: stub `disengage` forces `"retreat"`; stub `engage` cannot manufacture a chase; `:506` and `:890-891` never disagree; enemy-leaves-PVS while pinned still reaches `SeekLtg` via `:576/:598-599/:607/:614`; flips > 2/bot/min flagged; composition check passes **and is labelled as such**
- [ ] T13: the exact `AGENTS.md` diff is human-approved and dated in the tracker
- [ ] T14: q2dm1 table loads, all anchors resolve; a bogus anchor names itself at startup; ≤3 options, each containing a fact that is also in `state`; W5-sized total-request golden; **classname injection test**
- [ ] T15: 36 simulated bots never exceed the cap; revoked assignment ⇒ valid q3 goal same tick; re-priced budget asserted; live 12-bot region histogram **non-degenerate**
- [ ] T16: A/B in the Evidence table (N ≥ 5/arm, paired, including the predicted-no-difference metric); `docs/BRAINS.md` §6 lists `jev`; dated `brain_notes.md` append; the 7 named `context/pitfalls.md` entries exist
- [ ] Rule A held at every commit: `cargo fmt` + `cargo clippy -- -D warnings` + `cargo test` green
- [ ] Rule C: plan + tracker `git mv` to `completed/` and `SERIES.md` marked done
