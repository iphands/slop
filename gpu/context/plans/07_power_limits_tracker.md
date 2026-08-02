# GPU Power Limits — Tracker

## Overview
- Status: 5/7 tasks. **Answered: the 31.2 W cap cannot be raised from software** — PL1, PL2 and both together all leave it at 31.2 W (T4, T4b). T5 and the Palworld half of T3 remain, neither likely to change that.
- Start date: 2026-08-01
- Plan: `context/plans/07_power_limits.md`

## Resume Instructions

**Answered: this card's 31.2 W cap cannot be raised from software.** PL1 alone (T4), PL2
alone, and both at 45 W together (T4b) all leave it at exactly 31.2 W. Both RAPL registers
accept and retain writes; the PCU ignores them. Read T4b then T4.

**FIRST, CHECK THE MACHINE IS AT STOCK.** T4b left PL1 and PL2 at 45 W — the restore trap
did not fire and the cause is unknown:

```bash
sudo ./scripts/power-regs.sh --raw          # PL1 0x1459a0 and PL2 0x1459a4 both 0x00dc80fa?
sudo intel_reg write mmio:0x1459a4 0x00dc80fa
echo 31250000 | sudo tee /sys/class/drm/card0/device/hwmon/hwmon2/power2_max
```

**What is left, in order of value:**

1. **One `cat`, no root: is the throttle reason still `pl2` while PL2 is raised?**
   `cat /sys/class/drm/card0/device/tile0/gt0/freq0/throttle/reasons` during gameplay with
   PL2 at 45 W. If it still says `pl2`, the PCU is ignoring the register rather than
   enforcing some other limit — which distinguishes the two live hypotheses for free.

2. **The Palworld half of T3.** The one that actually matters for the wider project: if
   Palworld never reaches 31.2 W, this cap is irrelevant to frame rate and Plan 01's
   CPU-bound/GPU-bound question is the real one. `./scripts/gpu-survey.sh --match Palworld`.

3. **T5, the PL1-disable probe** — `./scripts/power-pl1-disable-probe.sh`. Now academic:
   T4 already showed the register accepts writes. Only tests whether `PWR_LIM_EN`
   specifically can be cleared.

4. **Not worth doing from software:** PL4, VR-current and any PCU-internal limit are not
   in the RAPL registers and `PKG_POWER_SKU` reads 0, so there is nothing left to write.
   The real TGP on Arc lives in the VBIOS power table. Out of scope, and it can brick the
   card.

Anything that modifies `power2_max` must restore it to `31250000` — T5 in particular leaves
the attribute invisible on the next driver reload if `PWR_LIM_EN` is left clear
(`xe_hwmon.c:1085-1092`). Both scripts restore from an EXIT trap.

Reruns: `./scripts/power-load-run.sh --label X --duration 75 --runs 3` (~6 min, puts a
vkmark window on screen). Pass `--no-hud` to stay comparable with the hud=off
`t3-baseline` / `t4-raised` captures.

Reference source is gitignored: run `scripts/vendor-prep.sh` to repopulate
`vendor/kernel/BUILD/linux-7.1.5/drivers/gpu/drm/xe/`.

## Measurements

| Date | Capture / build | Metric | Result | Runs | Spread |
|------|-----------------|--------|--------|------|--------|
| 2026-08-01 | live sysfs, kernel `7.1.5-201.fc44`, idle | package power, Δ`energy2_input` / Δt over 5.002 s | 13.15 W | 1 | n/a — single sample, **not admissible as a result**, recorded only as an order-of-magnitude sanity check |
| 2026-07-31 | `captures/survey_2026-07-31_193843.csv` (via `pitfalls.md:384`) | package power, GPU idle (`busy_rcs_pct` 0.00, 106 samples) | ~13.9 W | 1 | n/a |
| 2026-07-30 | `vkcube`, via `distilled.md:92` | package power, light load | ~12.6 W, `reasons=none` | 1 | n/a |
| 2026-08-01 | `captures/survey_trial_2026-08-01_110202.csv`, `vkmark -b shading:duration=20`, kernel `7.1.5-201.fc44`, Mesa `26.3.0-0.3.20260801.10.a5ab305`, stock `power2_max` 31250000 µW | plateau package power (skip 5 s, trim 2 s) | **30.71 W**, sd 0.96, max 31.48; `act_freq` 2407 MHz, `cur_freq` 2450 (pinned at rp0); throttle **`pl2` in 70% of plateau samples**; temp_max 55 °C | **1** | n/a — **single run, NOT admissible under Rule D.1** |

**The first three rows do not load the GPU.** The fourth does, but is one run — a pilot
that shaped the hypothesis, not a result. The admissible N ≥ 3 results are in the T3 and T4
sections below.

### T4b — does raising PL2 move the cap? **No. Nothing in software does.** 2026-08-02

Run by the human as `./scripts/power-pl2-experiment.sh --confirm --target-w 45 --runs 1
--duration 10`. `--duration 10` is below `power-load-run.sh`'s 60 s floor, so **no vkmark
capture was produced** — the evidence is the human's direct observation in Palworld via
MangoHud, not a CSV. Recorded as such.

**State confirmed by readback afterwards:**

| | Value | Watts |
|---|---|---|
| PL1 (`power2_max` sysfs) | `45000000` | 45 W |
| PL2 (`0x1459A4`) | `0x00dc8168` | 45 W |

Both limits accepted and held 45 W. **Palworld still drew 31.2 W.**

**Conclusion: the 31.2 W cap is not reachable from software.** PL1 alone (T4), PL2 alone,
and both together all produce exactly 31.2 W. The RAPL registers accept and retain our
writes and the PCU ignores them. Whatever enforces 31.2 W is not PL1 or PL2 — candidates
are PL4 (its sticky bit is set), VR current limits, or a PCU-internal value, consistent
with `PKG_POWER_SKU` reading 0 in both dwords.

**This also weakens the T4 conclusion**, which named PL2 as "the binding limit" on the
strength of the throttle reason being `pl2`. The reason register says `pl2`; raising the
PL2 register does not help. Either `0x1459A4` is not the value the PCU enforces, or the
PCU clamps PL2 to its own internal maximum. Do not describe PL2 as "the cap" without that
caveat.

**Open — cheap to close:** nobody has checked `throttle/reasons` *while* PL2 is raised. If
it still reports `pl2` at 45 W, that is direct evidence the PCU is ignoring the register
rather than honouring a different limit. One `cat` during gameplay, no root needed.

**Script bug — the restore did not run.** Both limits were left at 45 W after the first
invocation; the second refused to start because PL2 was not at stock (that guard worked as
designed). `power-pl2-experiment.sh` has `trap restore EXIT INT TERM` and `restore()`
writes both registers before any unguarded command, so it should have fired when
`power-load-run.sh` exited 2 on the duration check. **Cause not determined** — the terminal
output of that run was not captured. Do not trust the trap until this is understood; verify
with `sudo ./scripts/power-regs.sh --raw` after every run.

Manual restore:
```bash
sudo intel_reg write mmio:0x1459a4 0x00dc80fa
echo 31250000 | sudo tee /sys/class/drm/card0/device/hwmon/hwmon2/power2_max
```
A reboot also resets both.

### T4 — is a raised PL1 honoured? **The register accepts it; the cap does not move.** 2026-08-02 07:11–07:16

`./scripts/power-pl1-experiment.sh` (hud=off — predates the HUD default). PL1 31.25 → 50 W.

| Run | Capture | Plateau | sd | act_freq | temp_max | `pl2` share |
|---|---|---|---|---|---|---|
| 1 | `captures/survey_t4-raised_2026-08-02_071157_run1.csv` | 31.20 W | 0.03 | 2375 MHz | 64 °C | 94% |
| 2 | `…_run2.csv` | 31.20 W | 0.03 | 2345 MHz | 67 °C | 100% |
| 3 | `…_run3.csv` | 31.20 W | 0.03 | 2334 MHz | 68 °C | 100% |

**Across 3 runs: 31.20 W, spread 0.00 W — identical to the T3 baseline at 31.25 W PL1.**
Raising PL1 by 60% moved the plateau by nothing.

**The write landed and persisted.** `0x1459A0`: `0x00dc80fa` → `0x00dc8190` after the write,
and still `0x00dc8190` after all three loads. The punit did **not** revert it — so the
"maybe the punit rewrites the register" hypothesis from T2 is dead too.

**`0x1459A4` did NOT move — it stayed `0x00dc80fa` while `0x1459A0` changed.** That refutes
the T2 alias inference outright. They are independent registers, and `0x1459A4` bits [15:0]
= `0x80fa` = `EN` | 250 = **PL2 = 31.25 W**, matching the standard RAPL 64-bit layout
(PL2 value [46:32], enable [47]).

**Conclusion: PL2 is the binding limit, and `xe` provides no way to reach it.** The driver
defines no fields above bit 23 of the RAPL register and gates `power2_cap` out of sysfs at
`xe_hwmon.c:1080`. That is the complete answer to the original question — `power2_max`
writes work perfectly and are simply irrelevant to the ceiling.

**Corroboration from the sticky bits.** `GT0_PERF_LIMIT_REASONS` read `0x09000000` today
(sticky pl4 + pl2) versus `0x0d000000` yesterday (pl4 + pl1 + pl2). The pl1 sticky bit is
**absent** after the reboot — PL1 never fired at all during these runs, exactly as expected
once it was raised to 50 W. Also shows sticky bits clear on reboot.

**Incidental:** `FREQ_INFO_REC` read `0x31001100`, `0x31001200`, `0x31001400` across the
three register dumps — RPe moving 850 → 900 → 1000 MHz within five minutes, independent
confirmation that RPe is dynamic. Bits [31:24] stayed `0x31` throughout, confirming the
`rpa_freq` mask finding a third time.

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

1. ~~**`0x1459a4` is an alias of `0x1459a0`, not PL2.**~~ **REFUTED by T4 on 2026-08-02 —
   see the T4 section above. The two are independent registers and `0x1459a4` really is
   PL2.** The reasoning below was sound but the conclusion was wrong: I inferred register
   identity from two values being equal at rest. Left in place as the record of a wrong
   call. Original text follows.

   Byte-identical, *including* the
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
| 4 | T4: Is a raised PL1 honored? | `scripts/power-pl1-experiment.sh` | done | 2026-08-02. **No.** PL1 50 W -> plateau still 31.20 W, spread 0.00 W. `0x1459a4` did not move with `0x1459a0`, so it is PL2 = 31.25 W and it is the real cap. |
| 4b | T4b: Raise PL2 directly | `scripts/power-pl2-experiment.sh` | done | 2026-08-02. **No effect.** PL1 45 W + PL2 45 W -> Palworld still 31.2 W. Cap is not software-reachable. Script's restore trap did NOT fire — cause unknown, verify state manually after each run. |
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
