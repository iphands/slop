# GPU Power Limits — Tracker

## Overview
- Status: 33% complete (2/6 tasks; T3 tooling built, T3 measurement **not** collected)
- Start date: 2026-08-01
- Plan: `context/plans/07_power_limits.md`

## Resume Instructions

**Pick up at T3.** The harness is written and exercised; the N ≥ 3 measurement is not
collected. The session was stopped mid-run at the human's request (they were using the
machine). Run:

```bash
./scripts/power-load-run.sh --label t3-baseline --duration 75 --runs 3 --cooldown 25
```

That writes `captures/survey_t3-baseline_*_runN.csv` and prints the
`analyze/power_summary.py` verdict. Takes ~6 min and **puts a vkmark window on screen the
whole time** — do not start it while the machine is in use.

**T2 needs root** and has not been run against real hardware:

```bash
sudo ./scripts/power-regs.sh
```

It is read-only. Its `0x1459A4` (PL2) read has become the most important line in this plan
— see the hypothesis below.

**The hypothesis to test has changed since the plan was written.** The trial run says the
limiter is **PL2**, not PL1, and `power2_max` does not control PL2. So T4's expected
observation is now "raising PL1 changes nothing", and the interesting question moved to
whether PL2 is readable at `0x1459A4` and what value it holds. Do not skip T2.

T4 and T5 both modify `power2_max`. **Always restore it to `31250000` afterwards** — T5 in
particular leaves the attribute invisible on the next driver reload if `PWR_LIM_EN` is left
clear (`xe_hwmon.c:1085-1092`).

Reference source is gitignored: run `scripts/vendor-prep.sh` to repopulate
`vendor/kernel/BUILD/linux-7.1.5/drivers/gpu/drm/xe/`.

## Measurements

| Date | Capture / build | Metric | Result | Runs | Spread |
|------|-----------------|--------|--------|------|--------|
| 2026-08-01 | live sysfs, kernel `7.1.5-201.fc44`, idle | package power, Δ`energy2_input` / Δt over 5.002 s | 13.15 W | 1 | n/a — single sample, **not admissible as a result**, recorded only as an order-of-magnitude sanity check |
| 2026-07-31 | `captures/survey_2026-07-31_193843.csv` (via `pitfalls.md:384`) | package power, GPU idle (`busy_rcs_pct` 0.00, 106 samples) | ~13.9 W | 1 | n/a |
| 2026-07-30 | `vkcube`, via `distilled.md:92` | package power, light load | ~12.6 W, `reasons=none` | 1 | n/a |
| 2026-08-01 | `captures/survey_trial_2026-08-01_110202.csv`, `vkmark -b shading:duration=20`, kernel `7.1.5-201.fc44`, Mesa `26.3.0-0.3.20260801.10.a5ab305`, stock `power2_max` 31250000 µW | plateau package power (skip 5 s, trim 2 s) | **30.71 W**, sd 0.96, max 31.48; `act_freq` 2407 MHz, `cur_freq` 2450 (pinned at rp0); throttle **`pl2` in 70% of plateau samples**; temp_max 55 °C | **1** | n/a — **single run, NOT admissible under Rule D.1** |

**The first three rows do not load the GPU.** The fourth does, but is one run — it is a
pilot that shaped the hypothesis, not a result. The N ≥ 3 measurement is still outstanding.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Create plan 07, tracker, SERIES row | `context/plans/07_power_limits*.md`, `SERIES.md` | done | 2026-08-01 |
| 2 | T2: Read the hardware power fuses | `scripts/power-regs.sh` | blocked | Script written; `--self-test` passes 17/17. **Root path never run** — `sudo` needs a password. Now the highest-value task: `0x1459A4` is PL2. |
| 3 | T3: Does PL1 bind under load? | `scripts/power-load-run.sh`, `analyze/power_summary.py` | in-progress | Harness built and exercised. **N ≥ 3 measurement not collected** — interrupted mid-run 2026-08-01. One pilot run recorded above. |
| 4 | T4: Is a raised PL1 honored? | — | pending | Expected observation **revised**: the pilot says PL2 is the limiter, so raising PL1 should change nothing. Restore `power2_max` after. |
| 5 | T5: PL1-disable writability probe | — | pending | Watch temps; restore immediately. Still worth doing — it is the one write the driver verifies. |
| 6 | T6: Record findings | `context/pitfalls.md`, `context/distilled.md` | pending | Now **four** pitfalls entries — see below. |

## Negative / Inconclusive Results

*(RULES.md requires these be recorded. "Tried X, no measurable effect, 3 runs" stops the
next session re-running it.)*

- **2026-08-01 — MangoHud cannot display GPU power on `xe`, at all.** Not a
  misconfiguration and not fixable by changing PL1. `xe_hwmon.c`'s `hwmon_info[]` declares
  no `HWMON_P_INPUT` / `HWMON_P_AVERAGE` for any platform; MangoHud 0.8.3~rc1 only looks
  for `power1_average` and `power1_input` (verified by `strings` on
  `/usr/lib64/mangohud/libMangoHud.so`). Any future attempt to read watts from MangoHud on
  this box is wasted effort — use `gpu-survey.sh`'s `power_w` column. Goes to
  `pitfalls.md` in T6.

- **2026-08-01 — `kill -INT` does not stop `gpu-survey.sh` when it was backgrounded from a
  script.** A background (`&`) child of a **non-interactive** shell inherits `SIGINT` as
  `SIG_IGN`, and bash cannot re-trap a signal that was ignored on entry — so
  `gpu-survey.sh`'s `trap finish INT TERM` is a no-op for INT in that context. Observed:
  the sampler ran straight through three consecutive loads, producing one 919-sample CSV
  where three 150-sample ones were expected, and `power-load-run.sh` hung in `wait`. The
  polluted capture still *analysed cleanly* — 24.91 W mean, sd 8.14 — which is the
  dangerous part: it looked like a result. `kill -TERM` works. `power-load-run.sh` now uses
  `stop_survey()` (TERM, bounded 5 s wait, KILL fallback). Goes to `pitfalls.md` in T6.

- **2026-08-01 — the throttle reason under load is `pl2`, not `pl1`.** Pilot run only, so
  this is a hypothesis, not a finding. But if it holds it inverts the plan's premise:
  `power2_max` is PL1, and PL1 is not what fires. `xe` never exposes PL2 on DG2 — the
  visibility check is gated by `else if (attr != PL2_HWMON_ATTR)` at `xe_hwmon.c:1080`, and
  `regs/xe_mchbar_regs.h` defines no fields in the RAPL limit register's upper dword
  (`0x1459A4`) at all. That would explain the original symptom completely: raising
  `power2_max` cannot move a ceiling enforced by a different limit that the driver does not
  plumb. **Test it in T4** — raise PL1 and expect the plateau *not* to move.

- **2026-08-01 — `gpu-survey.sh` stamps `RUN INVALID per Rule D.5` on any throttled
  capture.** Correct for performance comparisons, actively misleading for Plan 07, where
  the throttle is the measurement. `analyze/power_summary.py` prints a note when it sees
  that header so the data is not discarded by a later reader. Worth remembering before
  reusing the survey for any other throttle-centric question.
