# GPU Power Limits — Tracker

## Overview
- Status: 17% complete (1/6 tasks)
- Start date: 2026-08-01
- Plan: `context/plans/07_power_limits.md`

## Resume Instructions

**T3 is the gate.** Do not spend time on T4 or T5 until T3 has established whether PL1
actually binds under load. If T3 shows `power_w` topping out well below 31.25 W with
`throttle_reasons=none`, the answer to this plan's question is "PL1 is not the cap" —
record that in Measurements below, skip to T6, and close the plan.

T2 (`scripts/power-regs.sh`) needs **root**. It is independent of T3 and can be done first
or in parallel; its `PKG_MAX_PWR` read is what tells you what value to write in T4.

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

**None of the above loads the GPU.** The plan's central question is unmeasured as of T1.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Create plan 07, tracker, SERIES row | `context/plans/07_power_limits*.md`, `SERIES.md` | done | 2026-08-01 |
| 2 | T2: Read the hardware power fuses | `scripts/power-regs.sh` | pending | Needs root. Also confirms/refutes the `RPA_MASK` theory. |
| 3 | T3: Does PL1 bind under load? | `scripts/gpu-survey.sh` (reuse) | pending | **Gates T4 and T5.** N ≥ 3, ≥ 30 s judging windows. |
| 4 | T4: Is a raised PL1 honored? | — | pending | Blocked on T3 confirming. Restore `power2_max` after. |
| 5 | T5: PL1-disable writability probe | — | pending | Blocked on T3 confirming. Watch temps; restore immediately. |
| 6 | T6: Record findings | `context/pitfalls.md`, `context/distilled.md` | pending | Two pitfalls entries + `rpa_freq` correction + drifted-freq refresh. |

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
