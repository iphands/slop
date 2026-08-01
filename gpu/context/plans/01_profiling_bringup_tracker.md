# Profiling bring-up — Tracker

## Overview
- Status: 45% complete (5/11 tasks; T5/T6 done at n=1, below the plan's N>=3 bar)
- Start date: 2026-07-30
- Plan: `01_profiling_bringup.md`

## Resume Instructions

Resume at the first `pending` task. Ordering constraints:

- **T1–T4 are gates.** T2 decides whether T7 has the u_trace path available; T3 decides
  whether T5 uses `gputop` or an fdinfo fallback; T4 decides whether Plan 03 is viable at
  all. Do these before any capture work.
- **T5 must precede T6–T8.** Its verdict determines whether the rest of the funnel is
  even the right question — a CPU-bound or throttled verdict retargets everything.
- **T9 (debug markers) is worth doing before deep T7/T8 analysis**, not after. It costs
  ~15 minutes and makes every pass name legible. It is listed late in the plan only
  because it is mechanically optional.
- **T10 needs T6's scenario**; T11 is the write-up and machine restore, last.

Each task: run it → record numbers in the Measurements table below → **commit**
(RULES.md Rule B: commit before marking anything complete).

**Before any capture session**: confirm the machine is in a known state — no leftover
`dev.xe.observation_paranoid`, no stale debug env vars in Steam launch options, stock Mesa ICD
active. See RULES.md Rule E.4.

## Open unknowns to resolve first

1. ~~Is u_trace compiled into Fedora's stock Mesa?~~ **RESOLVED (T2): yes.** T7 takes the
   rich render-stage path; Plan 04 no longer gates it.
2. ~~Does Fedora's `igt-gpu-tools` ship `gputop`?~~ **RESOLVED (T3): it ships, but it is
   unscriptable** (`-h/-d/-n` only, empty output when redirected). Replaced by
   `scripts/gpu-survey.sh` reading `fdinfo` + sysfs, which yields more.
3. Does Vulkan layer injection actually work in Palworld? (T4 — gates Plan 03) —
   **PARTIAL.** MangoHud's implicit layer demonstrably loads and runs inside the Proton
   prefix (it logged blacklist decisions for `explorer.exe` and `EpicWebHelper.exe`), so
   injection works at the loader level. It was *not* confirmed attached to
   `Palworld-Win64-Shipping.exe` itself — no MangoHud line names it. One glance at the
   screen during a run settles this; do that before trusting Plan 03.
4. ~~Is the live translation layer DXVK or vkd3d-proton?~~ **RESOLVED (2026-07-31): DXVK,
   D3D11.** `captures/Palworld-Win64-Shipping_d3d11.log` shows
   `D3D11InternalCreateDevice ... D3D_FEATURE_LEVEL_11_1`; the vkd3d log is 0 bytes.
   DXVK is `v2.7.1-491-g0a70623d` (Proton 11.0). **Good news for Plan 03** — the
   GFXReconstruct risk was the vkd3d path, and we are not on it.
5. **New, open:** ~22-35% of GPU time is unattributed. `INTEL_MEASURE` accounted for
   46.44 s of a 59.96 s wall span (77.5%; 64.8% over the gameplay subset) while the
   sampler reported 95.8% engine busy. A ranked pass table built on this has a hole in
   it. Resolve before T7's table is treated as complete.

## Measurements

> Every row needs provenance and variance (RULES.md Rule D). A delta inside the spread is
> **within noise** — write those words rather than reporting a trend. Record negative and
> inconclusive results too; they stop the next session re-running the same thing.

| Date | Capture / build | Metric | Result | Runs | Spread |
|------|-----------------|--------|--------|------|--------|
| 2026-07-31 | `survey_2026-07-31_201048.csv`, stock Mesa 26.3.0-0.3.20260801, `xe`, D3D11 | engine busy (rcs) | mean **95.8%**, max 99.3% | 1 | 213 samples @5 Hz |
| 2026-07-31 | same | power throttle | **`pl2` in 196/213 samples (92%)**, 31.1 W vs 31.25 W cap | 1 | — |
| 2026-07-31 | same | act_freq | mean 2237 MHz vs rp0 2450 (91%) | 1 | — |
| 2026-07-31 | same | VRAM resident | 3346 MiB of ~3949 (85%) | 1 | flat |
| 2026-07-31 | `measure_frame_2026-07-31_201013.csv` (`INTEL_MEASURE=frame`) | GPU ms/frame, gameplay (1231 frames) | p50 **28.21**, p90 29.85, p99 47.81, max 177.0 | 1 | p50->p90 = 1.6 ms |
| 2026-07-31 | same | attribution vs wall | 46.44 s of 59.96 s = **77.5%** (gameplay subset 64.8%) | 1 | — |

> **All rows are n=1.** Rule D requires N>=3 with spread before any of this is a result.
> They are recorded because they are the first numbers this project has produced that are
> not corrupted by the `type=` syntax bug, not because they are admissible.

## Verdict

*(T5's output — the single most consequential result of this plan. Fill in, then mirror
into `SERIES.md`, because Plan 06's target depends on it.)*

- **Bound**: **GPU-bound, ceilinged by the power budget.** *(n=1 — provisional.)*
- **Evidence**: 95.8% mean engine busy rules out CPU/DXVK starvation; `pl2` in 92% of
  samples at 31.1 W against a 31.25 W cap, with clocks held ~9% under rp0, identifies the
  limiter as power rather than clocks or VRAM (85% resident, no eviction signature). The
  p50->p90 spread of **1.6 ms** on gameplay frames is the tell: a flat ceiling, not the
  spiky profile of stalls or CPU starvation.
- **Implication for Plan 06**: work that reduces *GPU work per frame* converts directly
  into frames, because the part is saturated. Equally, any A/B must hold the power cap
  constant — it is the steady state here, not an anomaly.
- **Caveat**: Rule D.5 ("throttle invalidates the run") misfires on this workload. It was
  written for transient thermal events; `pl2` is this card's designed operating point
  under load. As written every run is invalid forever. **The rule needs amending** to
  distinguish "throttle appeared mid-run and changed conditions" from "the card sat at its
  power limit throughout".

## Reproducible bad-frame scenario

*(T6's output — the workload later plans target.)*

- **Description**: *pending*
- **How to reproduce**: *pending*

## Progress

| # | Task | File / Area | Status | Notes |
|---|------|-------------|--------|-------|
| 1 | T1: Verify `xe` tool facts | `context/distilled.md` | done | `xe` confirmed. **Docs were wrong**: knob is `dev.xe.observation_paranoid`, not `perf_stream_paranoid`. Corrected in 7 files; new `pitfalls.md` entry |
| 2 | T2: u_trace availability | `context/distilled.md` | done | **AVAILABLE** on stock Fedora Mesa 26.3.0 — T7 gets the rich path, Plan 04 de-risked. Event vocabulary + JSON shape recorded; 3 parser traps in `pitfalls.md` |
| 3 | T3: Install tooling, check `gputop` | `scripts/gpu-survey.sh` | done | Tooling present (igt 2.4, vulkan-tools, mangohud 0.8.2, renderdoc 1.45). **`gputop` unscriptable** — only `-h/-d/-n`, empty output when redirected, exits 0. T5's recipe replaced by the fdinfo/sysfs sampler, which yields more (per-client VRAM + throttle reasons) |
| 4 | T4: Confirm layer injection | — | partial | **DXVK/D3D11 confirmed live**, vkd3d unused — Plan 03 de-risked. MangoHud layer loads in-prefix; not confirmed attached to the game itself |
| 5 | T5: Whole-session survey + verdict | `scripts/record.sh` | done (n=1) | Verdict above: GPU-bound, `pl2` power-capped. **Needs N>=3 to be admissible** |
| 6 | T6: Per-frame capture, find scenario | `captures/` | done (n=1) | 5343 frames after the bare-token fix. Scenario still uncharacterised |
| 7 | T7: Pass-level capture, ranked table | `captures/` | pending | Headline deliverable. **Must be u_trace** — `INTEL_MEASURE=rt` cannot name passes (`pitfalls.md`) |
| 8 | T8: Gated per-draw capture | `scripts/measure-ctl.sh` | blocked | The control fifo aborts the game in `vkQueueSubmit2`; unexplained. `--no-fifo` is the workaround, but it cannot gate |
| 9 | T9: Debug markers | — | in-progress | Mechanism verified from the shipped binary: DXVK v2.7.1-491 understands `DXVK_DEBUG=markers`. Not yet captured with |
| 10 | T10: RenderDoc single frame | `captures/` | pending | Needs T6's scenario |
| 11 | T11: Write-up + restore machine | `context/`, `SERIES.md` | pending | |
