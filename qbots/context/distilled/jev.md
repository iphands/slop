# jev decision endpoint — `/v1/systemone` contract + measured envelope

> Plan 72 T1. The wire contract qbots' `jev` brain speaks, the numbers it was sized on, and the
> answers to the three questions that changed the design. Companion: `jev_lab.md` (what wording
> works — Plan 73). Code: `crates/qbots/src/jev/{net,wire,render,hub}.rs`,
> `crates/brain/src/jev/`.

## Endpoint

`cosmo.lan:8710`, model `h2oai/h2o-lightning-4b` (Q8_0, `n_ctx` 40960, 16 slots), no auth,
plaintext HTTP/1.1. **A decision model, not a chat model**: `POST /v1/chat/completions` with a
well-formed body answers `501 {"error":{"code":501,"message":"This model is a decision model
without text generation, use /v1/systemone","type":"not_supported_error"}}` — the capability
fingerprint `jev-probe health` asserts. An empty `{}` body gets a **400** first.

## Request / response

```text
POST /v1/systemone
{"state": <string|object|array>,
 "questions": {key: {"type":"choice","instructions":…,"criteria":{label:desc, …}}
             | {"type":"score","instructions":…,"criteria":[lv0,lv1,…]}
             | {"type":"noul","instructions":…}}}

→ {"model":"h2oai/h2o-lightning-4b",
   "answers":{key:{"type":"choice","choice":L,"probabilities":{label:p},"confidence":c}
            | {"type":"score","score":f,"legend":{"0":…},"probabilities":{"0":p,…},"confidence":c}
            | {"type":"noul","noul":p}},
   "usage":{"input_tokens":N,"output_tokens":0}}
```

- **Deterministic**: identical requests return byte-identical probabilities; `output_tokens` is
  always 0; probabilities sum to 1. A scoring oracle — retries are safe, there is no temperature.
- Bad question `type` → `400 …"type" must be one of: choice, score, noul`. 300 questions in one
  request are accepted.
- qbots validation (`wire::validate`): `answers` keys must equal the asked keys exactly; `model`
  must equal config (missing fails closed); labels must be offered; probability keys must equal
  the offered labels; unknown answer `type` ⇒ malformed; any structural fault rejects the whole
  response. qbots writes the request body by hand in declared order (this lockfile's
  `serde_json` sorts map keys and would reorder options).

## `confidence` — resolved

`confidence = (p1 − 1/n) · n/(n−1)`, the top-1 excess normalised to [0, 1]. Exact on three live
answers (binary {0.6632, 0.3368} → 0.3263; binary {0.8802, 0.1198} → 0.7604; 3-way top 0.4413 →
0.1620); the 24-option flat case (top 0.051 → 0.0096) fits within rounding. A function of p1
alone — logged, never gated on. qbots gates on the **margin p1 − p2** (`brain::jev::gate`).

## Measured envelope (2026-11-07, **loopback** — the dev box is the model host)

| probe | result |
|---|---|
| 1 `noul`, 82 tok | 12–19 ms |
| 3 questions, 387 tok | 58–147 ms |
| 12 `noul`s, one call | 109–112 ms |
| 1 question, 1 621 tok | 147–150 ms |
| 24-option choice | 57 ms |
| 8 concurrent 3-question requests | 154 → 501 ms (mean ≈ 346) |
| `jev-probe bench --bots 36` (2.4 req/s, 1 posture question, 2026-11-06) | p50 = p95 = 16 ms, 458 input tok/s |

Cost tracks **input tokens**, not question count (~8 ms per extra question, ~48 ms per 1 200 extra
state tokens). Honest headroom ≈ 3.9k tok/s single-stream (6.2k only by hammering, with latency
degrading). Production asks are ~190 input tokens (posture, schema v2).

## The three design questions

1. **`confidence` semantics** — resolved above.
2. **Name resolution**: on the dev box `cosmo.lan` is an `/etc/hosts` entry for `127.0.0.1`
   (fleet and model share the host). `.lan` is not mDNS (that is `.local`). A fleet on another
   host must resolve it itself and re-run `jev-probe bench` before trusting `deadline_ms`.
3. ~~**Is `/v1/systemone` a stable API or a stopgap?**~~ **UNANSWERED (T1)**: tried — the 501
   wording ("use /v1/systemone") reads purpose-built, but there is no API owner to ask from this
   repo. Consequence: the slug is pinned hard (`jev.model`, fail closed) and `jev-probe replay`
   re-asks harvested requests to detect a server-side change.

## Production settings (frozen 2026-11-06)

State schema v2 (coded fields + `threat` + graded summary); posture question `disengage / hold`
(`hold` = no-op); gate margin **0.30** (calibrated on the holdout half, Plan 73 T4); objective
question **not asked** (failed calibration). Budget 1 000 input tok/s, 3 workers, deadline 350 ms,
snapshot TTL 1.5 s, refresh 15 s and event floor 5 s (× one-sided [1, 1.3] jitter), decision TTL
24 s, posture lifetime 6 s of brain time.
