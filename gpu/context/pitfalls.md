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

# `INTEL_MEASURE` defaults to per-draw and will destroy a long capture **[from docs; syntax corrected 2026-07-31]**

> **Correction, 2026-07-31.** This entry documented the granularity selector as
> `INTEL_MEASURE=type=frame`, taken from the Mesa envvars page. **That syntax does not
> work.** Mesa parses the variable with `util/u_debug.c parse_debug_string()`, which
> splits on `", \n"` and requires an **exact token match**:
>
> ```c
> if (!strncmp("all", s, n) ||
>     (strlen(control->string) == n && !strncmp(control->string, s, n)))
> ```
>
> `type=frame` is one 10-character token; `frame` is 5. Nothing matches, `config.flags`
> comes back **0**, and `intel_measure.c` then does
> `if (!config.flags) config.flags = INTEL_MEASURE_DRAW;` — so *every* capture written
> with `type=` was silently **per-draw**, i.e. the exact failure mode this entry warns
> about. The token must be bare: `INTEL_MEASURE=frame,file=out.csv`.
>
> Cost of the bug on `station-lan`: a "frame" capture that produced **1.7 GB in 160 s**
> (rows are per-dispatch, `event_count=1`), and it is the leading explanation for the
> `vkQueueSubmit2` abort — arming per-draw instrumentation, which inserts a CS stall at
> every draw, mid-flight on a UE5 workload. Fixed in `scripts/launch-game-debug.sh`.

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

- Whole session → `frame` (~7200 rows for 2 min, negligible overhead)
- Whole session, mid-grain → `rt` (snapshots on render-target change)
- Seconds → `draw`, and **gate it with the control FIFO** rather than running it for
  the whole session:

```bash
mkfifo /tmp/measure.fifo
# launch with INTEL_MEASURE=draw,control=/tmp/measure.fifo,file=out.csv
#                            ^^^^ bare token — `type=draw` silently means draw anyway,
#                                 but `type=frame` ALSO silently means draw
echo 10 > /tmp/measure.fifo   # capture next 10 frames, then stop
```

`interval=N` combines N events per record if volume is still too high.

Ten frames of per-draw data on a scenario already known to be slow beats two minutes of
undifferentiated, self-perturbed data.

## Sources
- [Mesa: Environment Variables](https://docs.mesa3d.org/envvars.html) (`INTEL_MEASURE`)
- gpu: `context/plans/01_profiling_bringup.md` §1.4

---

# u_trace `print_json`: truncated on unclean exit, and events are not time-ordered **[verified 2026-07-30]**

## Problem

Three traps in `MESA_GPU_TRACES=print_json` output, all of which produce a *plausible*
wrong answer rather than an obvious failure. Aimed at Plan 02's parser.

**1. The file is only closed on a clean exit.** The top-level `]` is written at driver
teardown. Kill the app — `timeout`, SIGTERM, a crash, Steam's stop button — and the trace
is missing exactly one byte and is rejected outright by any strict JSON parser:

```
killed run : ... "duration_ns": 0\n}\n]\n}\n          <- no final ]
clean run  : ... "duration_ns": 0\n}\n]\n}\n\n]       <- closed
```

A game session profiled this way is worthless at the last step, after the play time is
already spent.

**2. Events within a batch are not sorted by `time_ns`.** Measured across a whole
`vkcube` capture: **121 of 361 batches (33.5 %)** carry out-of-order events. The pattern
is systematic, not jitter — `intel_begin_trace_copy_cb` is stamped *later* than the
events that follow it in file order:

```
intel_begin_trace_copy_cb  0000901567789635    <- latest
intel_begin_cmd_buffer     0000901567768697    <- ~21 us EARLIER, appears after it
intel_begin_btp            0000901567768750
```

A parser that computes durations by walking the array in order will produce negative or
nonsensical intervals for a third of all batches — or, worse, small positive ones that
merely look odd.

**3. Types are inconsistent within one object.** `frame` and `duration_ns` are JSON
numbers; `time_ns` is a **zero-padded string**. `"0000901570334322"` compares as a string
in the obvious sort, and a language that quietly coerces will sort lexicographically —
which happens to be *correct* for equal-width zero-padded values and silently *wrong* the
moment the width changes.

## Fix / How to avoid

- **Sort every batch's events by `int(time_ns)` before pairing `begin`/`end`.** Never
  trust file order.
- **Parse `time_ns` explicitly as an integer**, do not rely on coercion or on the padding
  staying a fixed width.
- **Make the parser tolerate a missing final `]`** — treat truncation as expected input,
  not corruption, since it is what any killed capture produces. This is also the reason to
  stream rather than `json.load()`: the streaming parser recovers the frames that were
  written, and Plan 01's captures are large enough to demand streaming anyway
  (`CLAUDE.md` → "Streaming over slurping").
- Exit the app cleanly when practical, but do not *depend* on it.

## Sources
- gpu: `/tmp/ut_clean.json` (`vkcube --c 120`, stock Mesa 26.3.0) vs a SIGTERM-killed run, 2026-07-30
- gpu: `context/distilled.md` (u_trace output shape), `context/plans/01_profiling_bringup.md` T2/T7

---

# `gputop` is not a scriptable `intel_gpu_top` — it has no machine-readable output **[verified 2026-07-30]**

## Problem

Our own `CLAUDE.md` and the table above present `gputop` as the `xe` replacement for
`intel_gpu_top`. It is — *interactively*. It is not a replacement for
`intel_gpu_top -J`, and Plan 01 T5 was written around a command that cannot exist:

```bash
sudo gputop -J -s 200 > captures/gputop_baseline.json    # -J and -s are not options
```

Fedora's `igt-gpu-tools-2.4` `gputop` accepts **only** `-h/--help`, `-d/--delay`, and
`-n/--iterations`. There is no JSON, no CSV, no interval-in-ms flag.

The dangerous part is the failure mode. Redirected to a file it exits **0** and writes a
few ANSI cursor-home/clear-screen escapes and nothing else:

```
$ gputop -n 2 -d 1 > out.json ; echo $?
0
$ cat out.json
^[[H^[[J^[[H^[[J
```

A zero exit and a non-empty file look like success. A script that then reports "0 %
engine busy" is reporting the absence of a parser, not the state of the GPU — which is
this project's central hazard wearing a different hat.

## Fix / How to avoid

Read `fdinfo` and sysfs directly; it is where `gputop` gets its numbers anyway, it needs
no root, and it yields strictly more (per-client VRAM, throttle reasons). That is what
`scripts/gpu-survey.sh` does — see `distilled.md` for the field map.

Generally: before building a capture step on a tool's flags, run `--help` on the version
actually installed. Do not trust a flag because a man page for another distro, another
release, or the other kernel driver documents it.

## Sources
- gpu: `gputop --help`, `igt-gpu-tools-2.4-1.fc44` on `station-lan`, 2026-07-30
- gpu: `context/plans/01_profiling_bringup.md` T5, `scripts/gpu-survey.sh`

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

---

# Palworld aborts ~40 s in via DXVK -> `vkQueueSubmit2`; Wine turns any error VkResult into a fatal assert **[verified 2026-07-31, n=3, root cause NOT identified]**

> **Correction, same session.** This entry first blamed `record.sh` arming `INTEL_MEASURE`,
> on a single run whose timing appeared to line up. That attribution was wrong and is
> retracted. Three crash dumps later show an identical callstack and a consistent 33-57 s
> time-to-crash, and the timestamps are not clean enough to place arming before or after
> the abort. `INTEL_MEASURE` is a suspect, not the cause. See "What the dumps show" below.

## Problem

Palworld aborts within the first minute of engine uptime, repeatably, with a Wine modal:

```
Assertion failed!  Program: Palworld-Win64-Shipping.exe
File: ../src-wine/dlls/winevulkan/loader_thunks.c  Line: 6767
Expression: "!status && \"vkQueueSubmit2\""
```

`winevulkan`'s generated thunks `assert()` that the ICU returns `VK_SUCCESS`. **Any**
error VkResult from the driver — including ones a native Linux app would handle — becomes
a hard abort under Proton. So the visible crash is Wine's reaction, not the fault itself.

`winevulkan`'s generated thunks `assert()` that the ICD returns `VK_SUCCESS`. **Any**
error VkResult from the driver — including ones a native Linux app would handle — becomes
a hard abort under Proton. So the visible crash is Wine's reaction, not the fault itself,
and the assert message does **not** carry the `status` value.

### What the dumps show

UE writes `CrashContext.runtime-xml` next to `UEMinidump.dmp` under
`…/compatdata/1623730/pfx/…/Pal/Saved/Crashes/UECC-*/`. Three crashes, one session:

| engine uptime | ErrorMessage | callstack (`<PCallStack>`) |
|---|---|---|
| 40 s | Abort signal received | `ucrtbase` ← `winevulkan +0x18984` ← `d3d11 +0x15554d` ← `d3d11 +0xacf0b` |
| 57 s | Unhandled Exception: 0x80000003 | same, `d3d11 +0xacfbd` |
| 33 s | Abort signal received | same, `d3d11 +0xacf0b` |

Identical shape every time: **DXVK's submit path → `vkQueueSubmit2` → error → assert →
abort**, always inside the first minute. `<MemoryStats.bIsOOM>` is 0 and `<IsStall>` false
in all three — but note `bIsOOM` is UE's *system RAM* check, it says nothing about VRAM.

| Established | How |
|---|---|
| The error came out of ANV | wine unwind: `libvulkan_intel.so + 0x472bf5` below `winevulkan.so` |
| **Not** a GPU hang/reset | no `devcoredump` under `/sys/class/drm/card*/device/`; `dmesg` needs root and was **not** read — partial |
| **Not** device-loss | `_vk_device_set_lost()` logs unconditionally via `__vk_errorv`, and Mesa's log *does* reach the Proton log (`MESA: warning: … Xe KMD` lines present); no device-lost line appears |
| **Not** thermal | `captures/survey_*.csv`: `throttle_reasons=none` throughout |
| GPU was **idle** before the abort | `survey_2026-07-31_193843.csv`: `busy_rcs_pct` 0.00 for all 106 samples, 850–900 MHz, ~13.9 W, VRAM flat at 3029.8 MiB of ~3949 |

That last row matters: the game was not rendering during the minute preceding the abort.
Whatever fails, it is not a heavy-workload effect.

Unresolved: the actual VkResult. It is not in the Proton log, not in the UE dump (which
only records "Abort signal received" — UE catching Wine's `SIGABRT`), and DXVK never sees
it because Wine asserts before returning.

## Fix / How to avoid

Do **not** attribute this to whatever you happened to change most recently — see the
correction at the top of this entry. Order the tests by what they eliminate:

1. **`launch-game-debug.sh --mode off`, and a run with Steam launch options cleared.**
   Until this is answered, nothing else is interpretable: it decides whether our profiling
   environment is in the picture at all.
2. **`--dx12`** — routes through vkd3d-proton instead of DXVK over the same ANV. If D3D12
   survives, the fault is in the DXVK↔ANV interaction, not ANV alone.
3. **VRAM.** 3.0–3.1 GiB resident of ~3.95 GiB on a 4 GiB card, before gameplay. Drop
   texture/resolution settings and re-run. `bIsOOM:0` does not rule this out.

Only after those: `INTEL_MEASURE=…,batch_size=1024` to test whether the per-command-buffer
allocation it adds (512 KiB BO + ~4.5 MiB host each) is implicated.

## Sources
- gpu: `…/Pal/Saved/Crashes/UECC-*/CrashContext.runtime-xml` ×3, 2026-07-31 session
- gpu: `~/steam-1623730.log` (Proton), `captures/survey_2026-07-31_19{2201,3843}.csv`
- mesa `src/intel/vulkan/anv_measure.c`, `src/vulkan/runtime/vk_device.c` (`_vk_device_set_lost`)

---

# `vkcube` cannot be used to test `INTEL_MEASURE` — ANV refuses to timestamp SIMULTANEOUS_USE command buffers **[verified 2026-07-31]**

## Problem

`vkcube` is the obvious minimal app for checking an `INTEL_MEASURE` recipe without
launching a game. It is useless for that purpose, and it fails *silently*: the run
completes, the CSV is created, and it contains **only the header row**.

Measured on `station-lan`, 600 frames each, all four combinations:

| config | rows |
|---|---|
| `type=frame` | 1 (header) |
| `type=draw` | 1 (header) |
| `type=frame,control=…` armed | 1 (header) |
| `type=draw,control=…` armed | 1 (header) |

Cause is in `anv_measure.c`, first check in `state_changed()`:

```c
if (cmd_buffer->usage_flags & VK_COMMAND_BUFFER_USAGE_SIMULTANEOUS_USE_BIT)
   /* can't record timestamps in this mode */
   return false;
```

`Vulkan-Tools/cube/cube.c` begins its command buffers with exactly that flag
(lines 893 and 1000). Every snapshot is filtered out before it is taken.

The trap is that a header-only CSV looks identical to "the workload had nothing to
measure", so a green-looking vkcube run can be read as "the recipe works" when the
instrumented code path never executed at all.

## Fix / How to avoid

- Do not validate `INTEL_MEASURE` plumbing with `vkcube`. A negative result there says
  nothing about the driver, the fifo, or the env var.
- **A header-only `INTEL_MEASURE` CSV means zero snapshots were taken, not zero work.**
  Check row count, never file existence.
- Note this also applies to the real workload: any DXVK/vkd3d command buffer submitted
  with `SIMULTANEOUS_USE` is invisible to `INTEL_MEASURE` by design.

## Sources
- gpu: vkcube matrix on `station-lan`, 2026-07-31 (Mesa 26.2.99, `xe`, A310)
- mesa `src/intel/vulkan/anv_measure.c` (`state_changed`)
- `KhronosGroup/Vulkan-Tools` `cube/cube.c:893,1000`

---

# `INTEL_MEASURE=rt` and u_trace are **not** interchangeable — only u_trace can name passes **[verified 2026-07-31, from source]**

## Problem

The profiling funnel lists Layer 2 as "`INTEL_MEASURE=rt` / u_trace", which reads as two
routes to the same answer. They are not. Only one of them can produce the thing Layer 2
exists for — a table that says `BasePass`, not `render target 0x7f2a…`.

In `src/intel/vulkan/anv_measure.c`, an event's name comes from exactly one place:

```c
if (event_name == NULL)
   event_name = intel_measure_snapshot_string(type);
```

and the file **never references debug-utils labels at all**. So every `INTEL_MEASURE` row
is named by hardware event *type*. Confirmed in a real capture
(`measure_frame_2026-07-31_201013.csv`): the only names that appear are `draw indexed`,
`compute`, `copy`, `ccs color clear`, `linear surface clear`.

`src/intel/vulkan/anv_utrace.c` is the file that implements
`anv_CmdBeginDebugUtilsLabelEXT` / `anv_CmdEndDebugUtilsLabelEXT` and reads
`cmd_buffer->vk.labels`. The application's pass names reach the trace **only** through
that path.

Setting `DXVK_DEBUG=markers` and then capturing with `INTEL_MEASURE` is therefore a silent
no-op: the markers are emitted, ANV receives them, and `INTEL_MEASURE` discards them. You
get a plausible, complete-looking CSV with no pass names in it, and nothing warns you.

## Fix / How to avoid

- **Pass-level analysis means u_trace**, not `INTEL_MEASURE=rt`:
  `MESA_GPU_TRACES=print_json,markers` paired with `DXVK_DEBUG=markers`
  (`--mode utrace --markers`).
- `INTEL_MEASURE=rt` is still useful — it is cheap and it bounds *how much* time goes to
  render-target-delimited work. It just cannot tell you *which* pass, ever.
- Verify the marker option against the binary you actually have rather than a doc. On
  `station-lan`: `strings .../dxvk/x86_64-windows/d3d11.dll` shows `DXVK_DEBUG`,
  `markers`, `dxvk.enableDebugUtils` and the `vkCmdBeginDebugUtilsLabelEXT` import —
  DXVK v2.7.1-491-g0a70623d, shipped in Proton 11.0.

## Sources
- mesa `src/intel/vulkan/anv_measure.c` (`anv_measure_start_snapshot`),
  `src/intel/vulkan/anv_utrace.c` (`anv_CmdBeginDebugUtilsLabelEXT`)
- gpu: `captures/measure_frame_2026-07-31_201013.csv` — event names observed
- gpu: `strings` on Proton 11.0's bundled DXVK, 2026-07-31

---

# Mesa's tracepoint filter ignores unknown names silently — and `blit`/`copy` are not names **[verified 2026-07-31, from source]**

## Problem

`INTEL_GPU_TRACEPOINT` selects which u_trace tracepoints fire. It is parsed by
`util/u_debug.c parse_enable_string()`, whose inner loop is:

```c
for (const struct debug_control *c = control; c->string != NULL; c++) {
   if (strlen(c->string) == n && !strncmp(c->string, s, n)) { ... }
}
```

An unrecognised token matches nothing and **falls out of the loop without a word** — no
warning, no non-zero exit, nothing in any log. The capture then comes back looking
entirely complete.

This project shipped `INTEL_GPU_TRACEPOINT=-blit,-copy,+render_pass,+draw,+frame` for a
week. `blit` and `copy` are not tracepoint names — the toggle table in
`src/intel/ds/intel_tracepoints.py` has no such entries. The whole `-blit,-copy` half was
a no-op, and the `+` half merely re-enabled things that are on by default anyway.

Worse than useless: the work it was *aiming* at is called **`blorp`** (Intel's
blit/clear/resolve path), and our own frame capture showed BLORP-type events leading
**658 of 1231 gameplay frames**. Had the filter worked as intended it would have hidden
the single most prominent thing in the capture.

The 40 real toggles are: `frame render_pass draw compute compute_indirect batch
cmd_buffer cmd_buffer_annotation queue_annotation barrier stall blorp btp sba xfb rays
query_clear_blorp query_clear_cs query_copy_cs query_copy_shader generate_cmds_pre
generate_cmds_post generate_draws trace_copy trace_copy_cb write_buffer_marker`, plus
thirteen `as_*` acceleration-structure ones. **All are enabled by default except
`stall`.**

## Fix / How to avoid

- **Do not filter until volume proves you must**, and then only with a name from
  `./scripts/launch-game-debug.sh --tracepoint-help`.
- The general rule, third time this has bitten: **every Mesa env var in this stack parses
  by exact token match and says nothing when it does not recognise one.** `INTEL_MEASURE`
  falls back to per-draw; `MESA_GPU_TRACES` yields flags 0; `INTEL_GPU_TRACEPOINT`
  silently keeps the defaults. Read the token table in the source before trusting any of
  them — `docs/envvars.rst` has already been wrong once.
- Verify a filter did what you meant by checking the trace contains the event types you
  expected, not by checking the file is non-empty.

## Sources
- mesa `src/util/u_debug.c` (`parse_enable_string`), `src/intel/ds/intel_tracepoints.py`
  (toggle table, `trace_toggle_name='intel_gpu_tracepoint'`),
  `src/util/perf/u_trace.c:305-319` (`MESA_GPU_TRACES` tokens, `MESA_GPU_TRACEFILE`)
- gpu: `captures/measure_frame_2026-07-31_201013.csv` — BLORP-type events lead 658/1231
  gameplay frames
