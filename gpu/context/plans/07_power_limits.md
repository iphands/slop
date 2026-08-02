# Plan 07 — GPU power limits: does PL1 bind, and can it be raised?

> **Status**: in-progress
> **Created**: 2026-08-01
> **Depends on**: N/A (feeds Plan 01's throttled verdict)
> **Goal**: Establish with provenance where the A310's ~31 W package power limit comes
> from, whether it actually binds under load, and whether raising it changes anything.
> **Hardware**: A310 / DG2-G11 / `xe`, kernel `7.1.5-201.fc44`. DG2-specific throughout —
> the register path here does **not** apply to BMG or MTL, which use PCODE mailboxes.

---

> **Before starting, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Find out whether the A310's 31.25 W `power2_max` is a real constraint on our
workload, and whether it can be moved. **Diagnosis only — no kernel patches this pass.**

**Deliverables**:
1. `scripts/power-regs.sh` — root MMIO dump/decode of the DG2 power & frequency registers.
2. A filled Measurements table in the tracker: does PL1 bind, N ≥ 3, with spread.
3. A recorded verdict on whether a raised PL1 is honored by the PCU.
4. Two new `context/pitfalls.md` entries and a corrected `context/distilled.md`.

**Estimated effort**: Small–Medium (half day, gated on one real Palworld session)

---

## Context

The human raised `/sys/class/drm/card0/device/hwmon/hwmon2/power2_max`, the write
succeeded, the value read back — and MangoHud showed no change in watts. Two questions
followed: is the cap real, and can a kernel patch or eBPF force past it?

Reading the `xe` source this box actually runs — kernel `7.1.5-201.fc44`, extracted from
`vendor/srpms/kernel-7.1.5-201.fc44.src.rpm`; **no Fedora patch touches
`drivers/gpu/drm/xe`**, so it is stock upstream 7.1.5 — reframes the question entirely.

### Prior Measurements

Nothing in this repo has ever loaded the GPU hard enough to test PL1. Both existing data
points are effectively idle:

| Source | Metric | Result | Runs | Spread |
|--------|--------|--------|------|--------|
| `pitfalls.md:383-384`, `captures/survey_2026-07-31_193843.csv` | package power | ~13.9 W, `busy_rcs_pct` 0.00 across all 106 samples | 1 | n/a |
| `distilled.md:92`, `vkcube` 2026-07-30 | package power | ~12.6 W against the 31.25 W limit, `reasons=none` | 1 | n/a |
| this session, 2026-08-01, Δ`energy2_input` over 5.002 s at idle | package power | 13.15 W | 1 | n/a |

**Neither answers the question.** "Capped at 32 W" is a hypothesis until a sustained load
shows `power_w` plateauing near 31 W *with* `reason_pl1` going to 1.

### Key Facts

Confirmed by reading the source and probing sysfs on 2026-08-01. Durable items go to
`distilled.md` in T6.

**DG2 takes the plain-MMIO power-limit path.** `dg2_desc` sets
`.has_mbx_power_limits = false` (`xe_pci.c:349`), so `xe_hwmon_power_max_write()` ends at:

```c
reg_val = xe_mmio_rmw32(mmio, rapl_limit, PWR_LIM, reg_val);   /* xe_hwmon.c:442 */
```

where `rapl_limit` is `PCU_CR_PACKAGE_RAPL_LIMIT` = `XE_REG(0x140000 + 0x59a0)` =
**0x1459A0** (`regs/xe_mchbar_regs.h:40`). BMG and Crescent Island use PCODE mailboxes
instead; none of this transfers to them.

**Three consequences, all load-bearing for this plan:**

1. **No driver-side clamp on write.** The "clamp to GPU firmware default" block
   (`xe_hwmon.c:421-435`) sits inside `if (hwmon->xe->info.has_mbx_power_limits)` and is
   skipped. The only guard that runs is a U12.3 overflow saturation at 4095 W
   (`xe_hwmon.c:404-416`).
2. **No verification.** `ret` is never assigned on the MMIO branch, so the sysfs write
   returns success even when the hardware ignores it.
3. **The readback re-reads that same register**, and its `PKG_POWER_SKU` clamp is guarded
   by `if (min && max)` (`xe_hwmon.c:357-368`). On this card `power2_rated_max` is absent,
   and its visibility gate is `xe_mmio_read32(0x145930) ? 0444 : 0` (`xe_hwmon.c:1093-1101`)
   — so that dword reads **0**, `min == 0`, and the clamp never fires.

Therefore **a successful readback proves only that the MMIO word holds the value.** The
driver documents the real behavior at `xe_hwmon.c:323`:

> *"HW allows arbitrary PL1 limits to be set but silently clamps these values to 'typical
> but not guaranteed' min/max values in REG_PKG_POWER_SKU."*

**Current register state**, derived from sysfs: `power2_max` = 31250000 µW = raw **250** in
1/8 W units (`scl_shift_power = 3`) → register word `0x80FA`. `power2_max_interval` = 28000
ms → `PWR_LIM_TIME` x=3, y=14, which pins `scl_shift_time = 0xa` and hence
`PKG_POWER_SKU_UNIT` (0x145938) = `0x000A0E03`.

**The card has never runtime-suspended** — `power/runtime_suspended_time = 0` at 10 h 41 m
uptime — so D3cold did not revert the human's earlier write. Either 31.25 W is the boot
default, or the punit rewrote the register.

**There is no instantaneous power sensor on `xe`, for any platform.** `hwmon_info[]`
(`xe_hwmon.c`) declares `HWMON_P_MAX | HWMON_P_RATED_MAX | HWMON_P_LABEL | HWMON_P_CRIT |
HWMON_P_CAP` — no `HWMON_P_INPUT`, no `HWMON_P_AVERAGE`. Only `energy2_input`, a monotonic
µJ counter, exists. `scripts/gpu-survey.sh:13` already derives watts from its deltas.

### Why diagnosis only — eBPF and kernel patches are the wrong tools here

**eBPF cannot do this.** No BPF helper performs MMIO. `bpf_override_return` on
`pc_set_max_freq` would skip the GuC H2G entirely and achieve nothing. BPF is useful here
only for observing, which ftrace already covers.

**A kernel patch buys nothing over root userspace.** MCHBAR is mirrored into BAR0.
`intel_reg` (igt-gpu-tools 2.4, installed) already recognizes this device — a non-root
`intel_reg read 0x1459a0` fails with `EACCES` at `intel_mmio_use_pci_bar`, not "unsupported
chipset". As root it reaches 0x1459A0 by the same path `xe_mmio_rmw32` does. If the PCU
clamps a value, it clamps it identically regardless of who wrote it.

**The frequency ceiling is not patchable.** `rp0_freq` 2450 MHz comes from `RP_STATE_CAP`
(0x145998) bits 7:0 = 49 × 50 MHz, a PCODE fuse mirror the driver only reads
(`xe_guc_pc.c:827-841`). `pc_set_max_freq()` rejects `> rp0` with `-EINVAL`
(`xe_guc_pc.c:377-390`), `pc_adjust_freq_bounds()` actively pulls GuC's own higher default
back down to rp0 (`xe_guc_pc.c:904-913`), and PCODE is the final arbiter regardless
(`xe_gt_freq.c:27-30`). There is no voltage control anywhere in `xe` — voltage is read-only
via `in1_input` from `GT_PERF_STATUS[10:0]`.
`SLPC_PARAM_GLOBAL_OC_UNSLICE_FREQ_MHZ` (id 14, `abi/guc_actions_slpc_abi.h:107`) is the
only OC-shaped thing in the tree and is referenced nowhere; it was explicitly scoped out.

**Deferred patch candidates**, recorded here so they are not rediscovered — *not* in scope:

- Make the DG2 write path read back and return `-EIO` instead of unconditional success.
- Expose PL2 (`power2_cap`) on DG2 — blocked today by `else if (attr != PL2_HWMON_ATTR)`
  at `xe_hwmon.c:1080`; the fields live in bits 47:32 of the 64-bit RAPL limit (0x1459A4)
  and `xe_mchbar_regs.h` does not define them.
- Fix `RPA_MASK` (see T2).

---

## Step-by-Step Tasks

### T1: Create this plan, its tracker, and the SERIES row

**Files**: `context/plans/07_power_limits.md`, `context/plans/07_power_limits_tracker.md`,
`context/plans/SERIES.md`

**What to do**: Written per RULES.md. Add a row 07 to SERIES.md marked `in-progress`.

**Commit**: `task(T1): add plan 07 — A310 power-limit investigation`

---

### T2: `scripts/power-regs.sh` — read the hardware's own power fuses

**File**: `scripts/power-regs.sh` (new)

**What to do**: Wrap `intel_reg read` (root) and decode the DG2 power/frequency registers.
Per CLAUDE.md rule 7 this belongs in `scripts/`, not typed twice at a prompt. Field
definitions: `drivers/gpu/drm/xe/regs/xe_mchbar_regs.h:21-46` and `xe_guc_pc.c:42-49`.

| Register | Addr | Decode |
|---|---|---|
| `PCU_CR_PACKAGE_POWER_SKU` lo | `0x145930` | `PKG_TDP` [14:0], `PKG_MIN_PWR` [30:16] |
| `PCU_CR_PACKAGE_POWER_SKU` hi | `0x145934` | `PKG_MAX_PWR` [46:32], `PKG_MAX_WIN` [54:48] |
| `PCU_CR_PACKAGE_POWER_SKU_UNIT` | `0x145938` | `PKG_PWR_UNIT` [3:0], `PKG_ENERGY_UNIT` [12:8], `PKG_TIME_UNIT` [19:16] |
| `PCU_CR_PACKAGE_RAPL_LIMIT` | `0x1459A0` | `PWR_LIM_VAL` [14:0], `PWR_LIM_EN` [15], `PWR_LIM_TIME_Y` [21:17], `PWR_LIM_TIME_X` [23:22] |
| RAPL_LIMIT upper dword | `0x1459A4` | PL2 — bits 47:32 of the 64-bit register; `xe` defines no fields |
| `RP_STATE_CAP` | `0x145998` | `RP0` [7:0], `RP1` [15:8], `RPN` [23:16], × 50 MHz |
| `FREQ_INFO_REC` | `0x145EF0` | `RPE` [15:8] × 50; see the rpa check below |

Power fields are U12.3 — divide the raw field by 8 for watts.

**Expected observation**:
- **Confirms** that the SKU fuses are unpopulated if `0x145930` reads **0** — that is what
  makes `power2_rated_max` invisible and the read-path clamp inert.
- **Refutes** the above if `0x145930` is non-zero: the clamp *is* active and the Context
  section's reasoning must be re-derived before continuing.
- `PKG_MAX_PWR` (`0x145934` bits 14:0) non-zero → that ÷ 8 W is the hardware's declared
  ceiling and the realistic target for T4. Zero → the PCU's clamp target is unknowable from
  MMIO and T4/T5 become the only way to find it.
- `0x1459A0` should read `0x...80FA`. Anything else means something rewrote it since boot.
- **rpa check.** `distilled.md:30` currently says `rpa_freq = 627200` is "meaningless."
  It is not. `RPA_MASK = REG_GENMASK(31, 16)` (`xe_guc_pc.c:49`) is too wide for this
  layout: the ratio is 8 bits at **31:24**. Arithmetic — 627200 / 50 = 12544 = 0x3100 =
  49 << 8, and 49 × 50 = 2450 = exactly `rp0`. The sibling `RPE_MASK` is 8 bits at 15:8,
  and MTL's `MTL_RPA_MASK`/`MTL_RPE_MASK` are both 9 bits (`regs/xe_regs.h:55,58`).
  **Confirms if** `0x145EF0` bits 31:24 == `0x31`, making real RPa 2450 MHz and this an
  upstream driver bug. **Refutes if** bits 23:16 are non-zero, in which case the field
  genuinely is wider and the value means something else.

**Safety**: `intel_reg` maps BAR0 directly, racing the driver. **Reads only. Never
`intel_reg write` while a workload is running.**

**Commit**: `task(T2): add scripts/power-regs.sh — decode the A310 power/freq registers`

---

### T3: Does PL1 actually bind under load?

**File**: none — reuse `scripts/gpu-survey.sh` unchanged. It already emits `power_w` (from
`energy2_input` deltas), `throttle_reasons`, `act_freq`, `temp_c`, and applies the Rule D.5
throttle gate.

**What to do**: Run it across a real Palworld session, **N ≥ 3**, and separately against a
fixed synthetic sustained load. For *this* question the synthetic load is the better
evidence — Palworld is not reproducible (CLAUDE.md, *The One Thing That Makes This Hard*) —
and the game only answers "does it bind in the actual workload."

Judge `power_w` over windows of **≥ 30 s**. `power2_max_interval` is 28 s, so short
excursions above 31 W are legal and expected; Δenergy/Δt is an average, not a peak.

**Expected observation**:
- **Confirms** PL1 binds if `power_w` plateaus at ~31 W **and** `reason_pl1` goes to 1.
  Proceed to T4.
- **Refutes** it if `power_w` tops out well below 31 W with `reasons=none`. Then compare
  `act_freq` against `rp0_freq` 2450: at the ceiling → frequency-limited, and no lever
  exists (see Context); well below with low engine busy → not GPU-bound at all, which is
  Plan 01's question, not this one. **Either way the power-limit thread is dead — record it
  and stop.**
- If `reason_thermal` / `reason_vr_tdc` / `reason_ratl` set instead, the limit is thermal
  or VR-current and raising PL1 would change nothing.
- **Within noise if** the run-to-run spread of the plateau exceeds the gap to 31.25 W.

**Commit**: `task(T3): record whether PL1 binds under sustained load`

---

### T4: Is a raised PL1 honored, and does it stick?

**Gated on T3 confirming PL1 binds.** If T3 refutes, skip and go to T6.

**What to do**:
1. `echo 50000000 > .../hwmon2/power2_max`
2. `sudo scripts/power-regs.sh` → confirm `0x1459A0`'s `PWR_LIM` holds the new value
   (50 W → raw 400 → `PWR_LIM` = `0x8190`; the full dword keeps its `0x00dc` tau bits, so
   expect `0x00dc8190`).
3. Re-run T3's load, N ≥ 3, compare `power_w`.
4. Afterwards re-read **both** `power2_max` and `0x1459A0`.

**Also settles the alias question (added after T2).** T2 found `0x1459a4` reading
byte-identical to `0x1459a0`. Step 2 is the definitive test: if `0x1459a4` *also* changes
to `0x00dc8190`, the address aliases and PL2 is not reachable there. If it stays
`0x00dc80fa`, it is a genuinely separate register and PL2 really was set equal to PL1.
Record which.

**Expected observation**:
- **Confirms** the write is honored if `power_w` rises meaningfully above 31 W.
- **Refutes** it if `power_w` still plateaus at ~31 W while the register reads 50 W — the
  PCU silently clamps, exactly as `xe_hwmon.c:323` documents. That is the end of the
  userspace road and no kernel patch changes it.
- If step 4 finds the register back at `0x80FA` on its own, the punit re-enforces its own
  table and **no write from any source will persist** — which would explain the 31.25 W
  the human observed after their earlier attempt.
- **Within noise if** the delta vs T3 is inside T3's measured spread. Write those words.

**Restore**: put `power2_max` back to `31250000` at the end of the task (Rule E.4).

**Commit**: `task(T4): record whether the PCU honors a raised PL1`

---

### T5: The definitive writability probe — disable PL1

`echo 0 > power2_max` is the **one** operation the DG2 path verifies
(`xe_hwmon.c:385-402`): it clears `PWR_LIM_EN`, re-reads, and returns `-EOPNOTSUPP` with a
`drm_warn("Power limit disable is not supported!")` if the bit refuses to drop.

**What to do** — brief and watched:
1. Record `temp2_input`, `temp3_input`, `fan1_input` first.
2. `echo 0 > power2_max`; capture the exit status **and** `dmesg | tail`.
3. Read back immediately.
4. **Restore at once**: `echo 31250000 > power2_max`.

**Expected observation**:
- **Confirms** the RAPL register is under our control if exit is 0 and readback is `0`.
  The remaining ceiling is then PL2 / PL4 / thermal / VR.
- **Refutes** it on `-EOPNOTSUPP` — the register is effectively read-only to us, and no
  kernel patch would change that.

**Risks and recovery**:
- This removes PL1 from a 75 W-slot card. Keep the window short and watch temps; PL2, PL4,
  thermal and VR limits all remain active.
- Once `PWR_LIM_EN` is clear, `xe_hwmon_power_is_visible()` gates on it
  (`xe_hwmon.c:1085-1092`) — after a driver reload `power2_max` and `power2_max_interval`
  **disappear entirely**. Step 4 avoids this; a reboot fixes it if forgotten.

**Commit**: `task(T5): record the PL1-disable probe result`

---

### T6: Write down what was learned

**Files**: `context/pitfalls.md`, `context/distilled.md`, `context/plans/SERIES.md`

**What to do**:

`pitfalls.md` — two entries, both measurement traps of exactly the kind that file exists for:
1. *MangoHud shows no GPU power on `xe`.* Not a misconfiguration. `xe` never declares
   `HWMON_P_INPUT`/`HWMON_P_AVERAGE`; MangoHud 0.8.3~rc1 only reads `power1_average` /
   `power1_input` (`strings /usr/lib64/mangohud/libMangoHud.so`). Use `gpu-survey.sh`'s
   `power_w` column.
2. *A successful `power2_max` readback proves nothing on DG2.* The write path is
   unverified and the read path re-reads the same unclamped MMIO word. Confirm the *effect*
   with Δenergy/Δt under load, never with a readback.

`distilled.md` — record the T2 register values with provenance; correct the `rpa_freq`
entry at line 30 if T2 confirmed the mask theory; refresh drifted values — the file records
`rpe_freq` 850 and "idles at `act_freq` 850", but 2026-08-01 reads **rpe 900, min_freq
1500, act_freq 0 when parked**.

Then mark plan 07 done in SERIES.md and `git mv` both files to `completed/` (Rule C).

**Commit**: `task(T6): record power-limit findings in distilled.md and pitfalls.md`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `context/plans/07_power_limits.md` + `_tracker.md` | This plan and its tracker | P0 |
| `context/plans/SERIES.md` | Add row 07; mark done at T6 | P0 |
| `scripts/power-regs.sh` | New — root MMIO dump/decode | P0 |
| `scripts/gpu-survey.sh` | **Reuse unchanged** — already emits `power_w`, `throttle_reasons` | — |
| `context/pitfalls.md` | Two new entries | P0 |
| `context/distilled.md` | Register values; correct `rpa_freq`; refresh drifted freqs | P0 |

Reference source (gitignored, regenerable via `scripts/vendor-prep.sh`):
`vendor/kernel/BUILD/linux-7.1.5/drivers/gpu/drm/xe/{xe_hwmon.c,xe_guc_pc.c,xe_gt_throttle.c,xe_pci.c,regs/xe_mchbar_regs.h}`.

---

## Open Questions / Risks

1. **PL1 may not be the cap at all.** T3 gates everything downstream. A negative result
   there ends the plan and is a perfectly good outcome — record it and stop. *Mitigation:*
   T3's Expected observation names the refuting result in advance.
2. **Is the effect above noise?** Unknown until T3 establishes the plateau's run-to-run
   spread. If the spread is comparable to the gap between the observed plateau and 31.25 W,
   this plan cannot answer its own question as written. Say so at that point.
3. **Palworld is not reproducible.** *Mitigation:* run the synthetic load too, and treat it
   as the primary evidence for the binds/does-not-bind question.
4. **Δenergy/Δt is an average, not a peak.** With a 28 s tau, sub-30 s windows will show
   excursions above 31 W that are not violations. *Mitigation:* fixed ≥ 30 s judging window.
5. **`intel_reg` races the driver on BAR0.** *Mitigation:* reads only; no writes during a
   workload.
6. **Disabling PL1 removes a limit from a 75 W-slot card.** *Mitigation:* see T5 —
   short window, temps watched, immediate restore, reboot as the fallback.
7. **The rpa conclusion is inference from one board.** *Mitigation:* T2's `0x145EF0` read
   confirms or kills it; it does not enter `distilled.md` before that.
8. **Does it survive a reboot?** The RAPL register's boot value is itself in question — T4
   step 4 is partly a persistence test. Anything measured in one session is provisional.
9. **Does it hold on `i915`?** Unknown and untested. `i915`'s hwmon exposes a different
   attribute set on DG2. Every finding here is an **`xe` claim** unless separately shown
   otherwise.
10. **`distilled.md` has drifted** since 2026-07-30 (rpe 850 → 900). *Mitigation:* re-verify
    values against sysfs rather than citing the file.

---

## Verification Checklist

- [ ] T1: `07_power_limits.md` + tracker exist and carry every RULES.md-required section;
      `SERIES.md` has row 07; committed.
- [ ] T2: `scripts/power-regs.sh` runs as root and prints decoded values for all 7
      registers; `PKG_MAX_PWR` recorded; the `0x145EF0` bits 31:24 == `0x31` prediction is
      confirmed or refuted **in writing**.
- [ ] T3: ≥ 3 runs, each with a named capture file in the tracker's Measurements table,
      each judged over ≥ 30 s windows; the binds / does-not-bind verdict is stated with
      its spread.
- [ ] T4: post-write register readback recorded; ≥ 3 load runs at the raised limit; delta
      vs T3 reported with spread, and called "within noise" if it is; `power2_max` restored
      to `31250000`.
- [ ] T5: exit status and `dmesg` output recorded verbatim; `power2_max` confirmed non-zero
      afterwards.
- [ ] T6: the two `pitfalls.md` entries and the `distilled.md` edits are on disk and
      re-read before any commit message claims them (CLAUDE.md rule 7).
- [ ] Plan: system restored to a known state (Rule E.4) — `power2_max` back at its original
      value, no stray sysctls, no `intel_reg write` left in effect.
