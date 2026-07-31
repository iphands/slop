# Profiling bring-up — Tracker

## Overview
- Status: 27% complete (3/11 tasks)
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
3. Does Vulkan layer injection actually work in Palworld? (T4 — gates Plan 03) — **open**.
4. **New, open:** is the live translation layer DXVK (D3D11) or vkd3d-proton (D3D12)?
   Palworld had *no* launch options set and defaults to D3D11, so the D3D12 assumption is
   unverified. T4 detects it. If D3D12, **Plan 03's GFXReconstruct capture is at risk** —
   the vkd3d path is upstream "future work".

## Measurements

> Every row needs provenance and variance (RULES.md Rule D). A delta inside the spread is
> **within noise** — write those words rather than reporting a trend. Record negative and
> inconclusive results too; they stop the next session re-running the same thing.

| Date | Capture / build | Metric | Result | Runs | Spread |
|------|-----------------|--------|--------|------|--------|
| | | | | | |

## Verdict

*(T5's output — the single most consequential result of this plan. Fill in, then mirror
into `SERIES.md`, because Plan 06's target depends on it.)*

- **Bound**: GPU-bound / CPU-DXVK-bound / thermally-or-power-capped — *pending*
- **Evidence**: *pending*
- **Implication for Plan 06**: *pending*

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
| 4 | T4: Confirm layer injection | — | pending | Gates Plan 03 |
| 5 | T5: Whole-session survey + verdict | `scripts/record.sh` | pending | N ≥ 3 sessions |
| 6 | T6: Per-frame capture, find scenario | `captures/` | pending | |
| 7 | T7: Pass-level capture, ranked table | `captures/` | pending | Headline deliverable |
| 8 | T8: Gated per-draw capture | `scripts/measure-ctl.sh` | pending | ≤10 frames, FIFO-gated |
| 9 | T9: Debug markers | — | pending | Do early, despite plan ordering |
| 10 | T10: RenderDoc single frame | `captures/` | pending | Needs T6's scenario |
| 11 | T11: Write-up + restore machine | `context/`, `SERIES.md` | pending | |
