# Plan Series — Dependency Chain

Master ordering of all `gpu` plans. **Update this file whenever a plan is added, starts,
or completes.** Status values: `pending` | `in-progress` | `done` | `blocked` |
`skipped` | `abandoned`.

> **North star:** find real, measurable performance wins for Intel Arc Alchemist on
> modern game workloads, and land them in Mesa's ANV where possible. The reference
> workload is Palworld (UE5) under Proton on an A310 / `xe`.
>
> **The ordering principle is top-down measurement.** Plans 01–03 build the ability to
> trust a number. Nothing downstream is worth doing until they are done — optimizing
> against an unreproducible workload is the default failure mode of this kind of project
> (see `CLAUDE.md`, "The One Thing That Makes This Hard").

| Plan | Title | Depends on | Status | Milestone / Notes |
|------|-------|-----------|--------|-------------------|
| **01** | Profiling bring-up | — | in-progress | Verify the box (`xe` tool divergence, u_trace availability, layer injection), then run the full funnel on a real 2-minute Palworld session. **Deliverable: a ranked pass-cost table + a GPU-bound/CPU-bound/throttled verdict.** Everything downstream is gated on this verdict. |
| **02** | Trace aggregation tooling | 01 | pending | The only code not already provided by existing tools: stream-parse `INTEL_MEASURE` CSV and u_trace JSON into ranked pass-cost tables. Must handle multi-GB inputs without slurping. |
| **03** | Deterministic replay harness | 01 | pending | GFXReconstruct capture → replay against N driver builds. **Includes measuring replay stability itself** — the run-to-run spread this number establishes is the noise floor every later plan is judged against. Without this, no patch can be evaluated. |
| **04** | Local Mesa build loop | 01 | pending | Out-of-tree Mesa with `-Dperfetto=true -Dbuildtype=debugoptimized`, run via `VK_DRIVER_FILES` without touching system Mesa. Gate: `vulkaninfo` reports the local build. |
| **05** | Hardware counters (Perfetto + PPS) | 04 | pending | Resolve the open `xe`-vs-`i915` PPS question (see `pitfalls.md`). Answers *why* a pass is slow: EU / sampler / bandwidth bound. Blocked on 04 because the PPS producer comes from the Mesa build. |
| **06+** | ANV optimization work | 02, 03, 04, 05 | pending | Unwritten by design — the target is chosen by what 01 and 05 actually find, not guessed in advance. Candidate directions if the data supports them: shader spilling in Lumen/post-process, pipeline-compile hitching, VRAM residency on a 3.95 GiB part. |
| **07** | GPU power limits | — | done | **Answered 2026-08-02: it binds continuously in Palworld, and it cannot be raised from software.** PL1, PL2 and both together (45 W) all leave package power at 31.2 W; `PKG_POWER_SKU` reads 0; the I1 mailbox is refused by DG2 hardware in both `xe` and `i915`. **Consequence: Palworld on this card is GPU-bound and power-limited, so the only route to more frames is less GPU work per frame — which promotes Plans 02–06 from follow-up to main line.** |

---

## Abandoned / Superseded

| Plan | Reason | Date |
|------|--------|------|

*(Record the reason. A plan dropped without a stated reason gets re-attempted six months
later by someone who doesn't know it failed.)*
