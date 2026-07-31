# Pitfalls & Measurement Traps

Read before new work. Every bug, gotcha, and **especially** every measurement that turned
out to be wrong. Template: `# Title → Problem → Fix → Source`.

Cross-cutting entries also mirror up to `../../context/pitfalls.md` per the slop
convention. Intel/Arc-specific ones stay here.

> **Status marker convention.** Entries are tagged **[verified]** (observed on
> `station-lan`) or **[from docs]** (read but not yet confirmed on our hardware).
> Promote to [verified] only after actually seeing it. Do not silently upgrade.

---

# `i915`-vs-`xe` tooling divergence — recipes fail confusingly, not loudly **[verified: driver; from docs: tool behavior]**

## Problem

Nearly every Intel GPU profiling guide on the web assumes the `i915` kernel driver.
`station-lan` runs **`xe`** (confirmed: `dmesg` shows `Initialized xe 1.1.0 for
0000:03:00.0`, DG2/G11 device `56a6`). The tools do not say "wrong driver" — they crash,
return nothing, or silently report zeros.

Known divergences:

| Thing | `i915` | `xe` (ours) |
|---|---|---|
| Engine-busy top | `intel_gpu_top` | **`gputop`** — `intel_gpu_top` is unsupported on `xe` and has been reported to crash |
| OA paranoia sysctl | `dev.i915.perf_stream_paranoid` | **`dev.xe.observation_paranoid`** — *renamed*, see the next entry |
| Mesa Perfetto data source | `gpu.counters.i915` | **unresolved** — Mesa docs name only the i915 source; whether the PPS producer speaks `xe` OA is untested. See the next entry. |

The insidious part: an `i915`-only tool that reports zeros looks exactly like a GPU that
isn't busy. That is a wrong *conclusion*, not a visible failure.

## Fix / How to avoid

**Confirm the bound driver before trusting any Intel profiling recipe:**

```bash
lspci -k -s 03:00.0 | grep -i 'kernel driver'
```

Then translate the recipe. When a tool reports suspiciously clean zeros, suspect driver
mismatch before believing the workload is idle.

Alchemist supports both drivers, switchable at boot
(`xe.force_probe=!56a6 i915.force_probe=56a6`) — so "just use `i915`" is available as a
fallback, but see the cross-driver comparison pitfall below.

## Sources
- gpu: `dmesg` on `station-lan`, 2026-07-30
- [intel_gpu_top(1) man page](https://man.archlinux.org/man/extra/intel-gpu-tools/intel_gpu_top.1.en)
- [Arch forums: intel_gpu_top crashes on the xe kernel driver](https://bbs.archlinux.org/viewtopic.php?id=295732)
- [Mesa: Perfetto Tracing](https://docs.mesa3d.org/perfetto.html)

---

# The `xe` OA paranoia knob is `observation_paranoid`, not `perf_stream_paranoid` **[verified 2026-07-30]**

## Problem

Every Perfetto/OA recipe — including, until now, our own `distilled.md` and the table
above — tells you to run:

```bash
sudo sysctl dev.xe.perf_stream_paranoid=0
```

**That sysctl does not exist.** `xe` calls it `observation_paranoid`:

```
$ sysctl dev.xe
dev.xe.observation_paranoid = 1

$ sysctl dev.xe.perf_stream_paranoid
sysctl: cannot stat /proc/sys/dev/xe/perf_stream_paranoid: No such file or directory
```

`i915` keeps the old name, and on this box `/proc/sys/dev/i915/perf_stream_paranoid` **is
present** even though `i915` is bound to nothing (module loaded, refcount 0). So
`sysctl -a | grep perf_stream_paranoid` returns a confident-looking hit — for the wrong,
inactive driver.

Two ways this bites. The `cannot stat` error reads as "this kernel has no OA support",
prompting a hunt for a missing feature that is present under another name. Or you grep,
find the `i915` entry, set *that* to `0`, observe no change in behavior, and conclude OA
is unavailable on `xe`.

## Fix / How to avoid

**Enumerate the namespace instead of grepping for a remembered name:**

```bash
sysctl dev.xe            # authoritative: shows exactly what xe exposes
```

Generally: when porting an `i915` recipe to `xe`, treat every identifier as renamed until
observed. `xe` renamed the whole `perf` subsystem to `observation`. A `grep` across both
namespaces will happily match the inactive driver and give a false positive.

## Sources
- gpu: `sysctl dev.xe` / `ls /proc/sys/dev/{xe,i915}/` on `station-lan`, 2026-07-30
- gpu: `context/distilled.md` (Perfetto + PPS section), `context/plans/01_profiling_bringup.md` T1

---

# `INTEL_MEASURE` defaults to per-draw and will destroy a long capture **[from docs]**

## Problem

`INTEL_MEASURE` with no `type=` defaults to **`draw`** — one CSV row per draw call, with
a **flush at every snapshot boundary** for timestamp accuracy.

On a UE5 title at ~60 fps with a few thousand draws per frame, a two-minute capture is on
the order of 10⁷–10⁸ rows and gigabytes of CSV. Worse than the volume: the per-draw
flushing *changes the frame times being measured*. The capture reports the cost of
measuring, not the cost of rendering.

This is the classic form of this project's central hazard — an instrument that silently
becomes the bottleneck, producing numbers that look plausible and are meaningless.

## Fix / How to avoid

**Match boundary granularity to capture duration.** Coarse for long, fine for short:

- Whole session → `type=frame` (~7200 rows for 2 min, negligible overhead)
- Whole session, mid-grain → `type=rt` (snapshots on render-target change)
- Seconds → `type=draw`, and **gate it with the control FIFO** rather than running it for
  the whole session:

```bash
mkfifo /tmp/measure.fifo
# launch with INTEL_MEASURE=type=draw,control=/tmp/measure.fifo,file=out.csv
echo 10 > /tmp/measure.fifo   # capture next 10 frames, then stop
```

`interval=N` combines N events per record if volume is still too high.

Ten frames of per-draw data on a scenario already known to be slow beats two minutes of
undifferentiated, self-perturbed data.

## Sources
- [Mesa: Environment Variables](https://docs.mesa3d.org/envvars.html) (`INTEL_MEASURE`)
- gpu: `context/plans/01_profiling_bringup.md` §1.4

---

# Comparing measurements across kernel drivers measures the wrong thing **[from docs]**

## Problem

If the Mesa PPS producer turns out not to support `xe` OA, the tempting fix is to boot
`i915` for counter sessions. That is fine — but any *absolute* number gathered under
`i915` describes a different machine than the one being played on. Phoronix measured
meaningful i915-vs-xe performance deltas on Arc; the drivers differ in GuC usage,
submission, and scheduling.

The failure mode is subtle: gather a baseline under `i915` for counter access, gather the
patched number under `xe` because that's what was booted that day, and report the
difference as the patch's effect.

## Fix / How to avoid

Cross-driver data answers **portable questions about shader behavior** — *"is this pass
sampler-bound or EU-bound?"* — and nothing else. Never compare absolute frame times
across drivers.

Generally (RULES.md Rule D.4): **change one thing per comparison.** Driver, Mesa build,
resolution, and settings are four separate axes; a comparison that moves two of them
measures neither.

## Sources
- [Phoronix: Switching from i915 to Xe for Arc A-Series](https://www.phoronix.com/review/intel-i915-xe-linux-2025)
- gpu: `context/plans/RULES.md` Rule D

---

# A local Mesa build that never loaded will happily "measure" as a no-op **[from docs]**

## Problem

The intended workflow is an out-of-tree Mesa run via `VK_DRIVER_FILES`, leaving system
Mesa intact. If the path is wrong, the ICD JSON is stale, or the variable doesn't survive
into the game's process (Steam launch options, Proton's re-exec, a wrapper script that
drops the environment), Vulkan silently falls back to the system driver.

The patch then "measures" as having no effect — which is indistinguishable from a patch
that genuinely does nothing, and it's the more likely explanation of the two.

## Fix / How to avoid

**Verify the driver actually loaded, every time, before recording any number:**

```bash
VK_DRIVER_FILES=/path/to/mesa/build/src/intel/vulkan/intel_icd.x86_64.json \
  vulkaninfo | grep driverInfo
```

It must report the local build, not the distro version (`26.3.0-0.3.20260729…`). This is
RULES.md Rule A.2 and it is cheap; run it as part of the capture script rather than by
hand, so it cannot be skipped when you're in a hurry.

## Sources
- gpu: `context/plans/RULES.md` Rule A.2
- [Mesa: Perfetto Tracing](https://docs.mesa3d.org/perfetto.html) (build layout)
