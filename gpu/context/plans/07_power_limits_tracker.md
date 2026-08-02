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

### T3 — does PL1 bind? **Yes.** 2026-08-01 21:16–21:21

`./scripts/power-load-run.sh --label t3-baseline --duration 75 --runs 3 --cooldown 25`
Kernel `7.1.5-201.fc44`, Mesa `26.3.0-0.3.20260801.10.a5ab305`, stock `power2_max`
31250000 µW, load `vkmark -b shading:duration=75`, plateau = samples in [30 s, end−2 s].

| Run | Capture | Plateau power | sd | act_freq | cur_freq | temp_max | `pl2` share |
|---|---|---|---|---|---|---|---|
| 1 | `captures/survey_t3-baseline_2026-08-01_211654_run1.csv` | 31.20 W | 0.03 | 2384 MHz | 2450 | 68 °C | 94% |
| 2 | `captures/survey_t3-baseline_2026-08-01_211654_run2.csv` | 31.20 W | 0.03 | 2382 MHz | 2450 | 69 °C | 98% |
| 3 | `captures/survey_t3-baseline_2026-08-01_211654_run3.csv` | 31.20 W | 0.02 | 2376 MHz | 2450 | 70 °C | 93% |

**Across 3 runs: 31.20 W, run-to-run spread 0.00 W, noise floor 0.125 W** (bounded by the
PL1 register quantum, not by measurement scatter). Gap to the configured 31.25 W is
**+0.05 W — below one register quantum, i.e. unresolvable.**

**Verdict: the card is power-limited, and the plateau sits exactly at the PL1 setting.**
Not thermal (70 °C peak, `reason_thermal` never set). Not frequency-limited: `cur_freq` is
pinned at rp0 2450 the whole time while `act_freq` is held to ~2380, i.e. SLPC is asking
for full clocks and something else is refusing.

**The reported reason is `pl2` in 93–98% of plateau samples, yet the value enforced equals
PL1.** Those two facts together are the crux. Read alongside T2's sticky bits (pl1, pl2 and
pl4 have all fired since boot), the most likely reading is that PL2 is the fast-acting
limiter the PCU uses to hold the 28 s average at the PL1 target — so the *reason* reported
is pl2 while the *value* tracked is PL1's.

This makes T4 more likely to move the needle than the pilot suggested. If the plateau
tracks the PL1 setting, raising PL1 should raise it. If it stays at 31.20 W, the PCU is
clamping to something of its own. **Both outcomes are informative; do not skip T4.**

**Gap — the Palworld half of T3 was never run.** The plan called for the synthetic load
*and* a real Palworld session. Only the synthetic ran. That is enough to answer "is this
card power-limited" (definitively: yes), because a fixed load is the better evidence for
that question. It does **not** answer "does PL1 bind during actual gameplay" — Palworld may
not sustain enough GPU load to reach 31.2 W, in which case the cap is irrelevant to the
frame rate and Plan 01's CPU-bound/GPU-bound question is the one that matters. To close it:

```bash
./scripts/gpu-survey.sh --match Palworld --interval 0.5 --out captures/survey_palworld_pl1.csv
./analyze/power_summary.py --limit-w 31.25 captures/survey_palworld_pl1.csv
```

Needs ≥ 3 sessions of ≥ 105 s each to satisfy Rule D.1.

### T2 — register read, 2026-08-01 21:15, `sudo ./scripts/power-regs.sh`

Kernel `7.1.5-201.fc44`, card `0000:03:00.0` device `0x56a6`, driver `xe`. Card idle at
read time (a load run was between iterations). Raw values:

| Register | Addr | Value |
|---|---|---|
| `PKG_POWER_SKU` lo | `0x145930` | `0x00000000` |
| `PKG_POWER_SKU` hi | `0x145934` | `0x00000000` |
| `PKG_POWER_SKU_UNIT` | `0x145938` | `0x000a0e03` |
| `PKG_RAPL_LIMIT` | `0x1459a0` | `0x00dc80fa` |
| `PKG_RAPL_LIMIT` +4 | `0x1459a4` | `0x00dc80fa` |
| `RP_STATE_CAP` | `0x145998` | `0x00062f31` |
| `FREQ_INFO_REC` | `0x145ef0` | `0x31001100` |
| `GT0_PERF_LIMIT_REASONS` | `0x1381a8` | `0x0d000000` |

**Five findings, in order of how much they change the plan:**

1. **`0x1459a4` is an alias of `0x1459a0`, not PL2.** Byte-identical, *including* the
   time-window bits (x=3, y=14 → 28 s). PL2 encodes its own window independently and it is
   normally far shorter than PL1's, so an exact 24-bit match is the signature of the address
   aliasing back — not of PL2 coincidentally equalling PL1. **PL2's value remains unknown
   and unreachable from this register.** Definitive test deferred to T4: change
   `power2_max` and re-read both; if both move, alias proven.

2. **PL1 fires too — the pilot's "it's PL2, not PL1" reading was too strong.**
   `GT0_PERF_LIMIT_REASONS` = `0x0d000000`: live bits (mask `0xde3`) are all clear, while
   bits **24, 26, 27** are set — the sticky-log positions for **pl4, pl1, pl2**. All three
   have tripped since boot. Sticky-ness is still an inference, but live-clear + upper-set is
   exactly its signature. Cumulative since boot, so not attributable to any one run.

3. **`PKG_POWER_SKU` reads 0 in *both* dwords.** Predicted and confirmed for the low dword;
   the high dword is new. So `PKG_TDP`, `PKG_MIN_PWR` and `PKG_MAX_PWR` are all unpopulated:
   the read-path clamp is fully inert, `power2_rated_max` is correctly hidden, and **the
   hardware's own power ceiling cannot be learned from MMIO.** T4/T5 are the only route.

4. **The `rpa_freq` bug is confirmed on hardware.** `FREQ_INFO_REC` = `0x31001100`:
   bits [31:24] = `0x31` = 49 → 49 × 50 = **2450 MHz, exactly RP0**; bits [23:16] = 0. So
   `RPA_MASK = REG_GENMASK(31, 16)` at `xe_guc_pc.c:49` is too wide by 8 bits and inflates
   the value 256×. Real upstream bug. `distilled.md:30`'s "meaningless" note should become
   "reads 256× high; divide by 256".

5b. **`min_freq` is a working user setting — do not confuse it with `rpe_freq`.** It read
   1500 MHz at 10:14 and 850 MHz at 21:30, and I briefly wrote that down as GuC drift. That
   was wrong: `min_freq` is `0644` and the human sets it externally and successfully, so the
   change was almost certainly theirs. `rpe_freq` is `0444` with no write path, which is why
   point 5 below *is* a real observation and this one is not. **Consequence for
   measurement:** `min_freq` can differ between sessions because someone set it, so capture
   it per run as provenance — not because it drifts on its own.

5. **RPe is dynamic, not a spec constant.** The register gives bits [15:8] = `0x11` = 17 →
   **850 MHz**, but sysfs read **900 MHz** at 10:14 the same day. PCODE recomputes RPe at
   runtime. `distilled.md`'s 850 was not stale — the value genuinely moves. Correct the file
   to say so rather than writing a new fixed number. (`RP1` = `0x2f` = 47 → 2350 MHz; RP0
   2450 and RPn 300 both confirmed.)

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Create plan 07, tracker, SERIES row | `context/plans/07_power_limits*.md`, `SERIES.md` | done | 2026-08-01 |
| 2 | T2: Read the hardware power fuses | `scripts/power-regs.sh` | done | 2026-08-01 21:15. All five predictions resolved; see the register table above. Script updated afterwards to detect the `0x1459a4` alias rather than decode it as PL2. |
| 3 | T3: Does PL1 bind under load? | `scripts/power-load-run.sh`, `analyze/power_summary.py` | done | 2026-08-01. **Yes** — 31.20 W across 3 runs, spread 0.00 W, exactly at the 31.25 W setting. Not thermal, not frequency-limited. |
| 4 | T4: Is a raised PL1 honored? | `scripts/power-pl1-experiment.sh` | pending | Needs sudo. Expectation **revised again**: the plateau tracks PL1 exactly, so it may well move. Both outcomes informative. |
| 5 | T5: PL1-disable writability probe | `scripts/power-pl1-disable-probe.sh` | pending | Needs sudo. Script samples temps, writes 0, captures exit status + new dmesg, reads back, restores from a trap. Deliberately runs the GPU **idle** — the question is whether the register accepts the write. |
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
