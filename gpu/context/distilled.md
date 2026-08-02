# Distilled — confirmed tool, driver & hardware facts

Compact reference for Intel Arc profiling on Linux. Read before new work; append after
every session that learns something. Keep it dense.

> **Status marker convention.** **[verified]** = observed on `station-lan`.
> **[from docs]** = read in upstream documentation, not yet confirmed on our hardware.
> Promote only after actually seeing it.

---

## The machine — `station-lan` **[verified 2026-07-30]**

| Property | Value |
|---|---|
| GPU | Intel Arc A310, DG2/G11, PCI device `56a6` rev 05, stepping C0, `03:00.0` |
| Kernel driver | **`xe` 1.1.0** (not `i915`) |
| VRAM | 4 GiB physical, `0xfd000000` ≈ **3.95 GiB usable**, fully CPU-accessible (ReBAR on) |
| Firmware | GuC 70.53.0, DMC v2.8 |
| Fused off | `ccs0`, `ccs2`, `ccs3` (one compute engine remains); `vcs1,3,4,5,6,7`; `vecs2,3` |
| Power | `PL1` supported on channel 1; driver uses register-based power limits |
| OS / Mesa | Fedora 44, Mesa `26.3.0-0.3.20260729.05.21dc9d4` (git snapshot), i686 + x86_64 |
| OA paranoia sysctl | **`dev.xe.observation_paranoid`** (= `1` by default). The only knob under `dev.xe`. |
| Boost / efficient / min clock | `rp0_freq` **2450** MHz (fused), `rpn_freq` **300** (fused), `rp1_freq` 2350. `rpe_freq` is **recomputed at runtime** — see below |
| Package power limit | `power2_max` = **31.25 W** (`power2_max_interval` 28 s), and it **binds**: sustained load plateaus at 31.20 W. See *Power limits* below |

Source: `dmesg | grep 'xe '`, `lspci`, `rpm -qa | grep mesa`, `sysctl dev.xe`,
`/sys/class/drm/card0/device/tile0/gt0/freq0/`, `.../device/hwmon/hwmon2/`.

**`rpa_freq` reads 256× too high — it is a driver bug, not a meaningless value.**
**[verified 2026-08-01 by MMIO read; supersedes the 2026-07-30 note that called it
meaningless]**

`FREQ_INFO_REC` (`0x145EF0`) reads `0x31001100`. The RPa ratio is an 8-bit field at
**[31:24]** = `0x31` = 49, and 49 × 50 MHz = **2450 MHz — exactly `rp0`**. Bits [23:16] are
zero. But `xe` declares `RPA_MASK = REG_GENMASK(31, 16)` (`xe_guc_pc.c:49`), 8 bits too
wide, so it scoops up `ratio << 8` and reports 49 × 256 × 50 = `627200`.

Corroboration: the sibling `RPE_MASK` on the same register is 8 bits at [15:8], and MTL's
`MTL_RPA_MASK` / `MTL_RPE_MASK` are both 9 bits (`regs/xe_regs.h:55,58`) — the pre-1270
fallback path looks untested. **Divide `rpa_freq` by 256 to get the real value.** Nothing
in the driver consumes it, so the bug is cosmetic. Upstreamable one-line fix, not yet filed.

**`rpe_freq` is dynamic — do not record it as a spec constant.** PCODE recomputes it at
runtime: sysfs read **900 MHz** at 10:14 and the register read **850 MHz** at 21:15 the same
day (2026-08-01). It is `0444`, so nothing wrote it. Capture it per measurement rather than
citing a remembered value. (`min_freq`, by contrast, is `0644` and *is* a working user
setting — a changed value there means someone set it.)

---

## Power limits — the A310 is power-capped at ~31.2 W **[verified 2026-08-01]**

**PL1 binds.** Under sustained `vkmark` load, package power plateaus at **31.20 W across 3
runs, run-to-run spread 0.00 W** (`captures/survey_t3-baseline_2026-08-01_211654_run{1,2,3}.csv`).
The configured PL1 is 31.25 W; the 0.05 W gap is below one register quantum (1/8 W) and so
unresolvable. Not thermal — 70 °C peak, `reason_thermal` never set. Not frequency-limited —
`cur_freq` stays pinned at `rp0` 2450 while `act_freq` is held to ~2380.

**The reported reason is `pl2`, in 93–98 % of plateau samples, even though the value
enforced equals PL1.** Likely reading: PL2 is the fast-acting limiter the PCU uses to hold
the 28 s average at the PL1 target.

### Register map (MCHBAR mirror, base `0x140000`) — read with `scripts/power-regs.sh`

| Register | Addr | A310 value | Meaning |
|---|---|---|---|
| `PKG_POWER_SKU` lo | `0x145930` | `0x00000000` | `PKG_TDP`, `PKG_MIN_PWR` — **unpopulated** |
| `PKG_POWER_SKU` hi | `0x145934` | `0x00000000` | `PKG_MAX_PWR` — **unpopulated** |
| `PKG_POWER_SKU_UNIT` | `0x145938` | `0x000a0e03` | power 1/8 W, energy 1/16384 J, time 1/1024 s |
| `PKG_RAPL_LIMIT` | `0x1459A0` | `0x00dc80fa` | PL1 250 (31.25 W), `EN`=1, tau x=3 y=14 → 28 s |
| `PKG_RAPL_LIMIT` +4 | `0x1459A4` | `0x00dc80fa` | **aliases the low dword — not PL2** |
| `RP_STATE_CAP` | `0x145998` | `0x00062f31` | RP0 49, RP1 47, RPn 6 (× 50 MHz) |
| `GT0_PERF_LIMIT_REASONS` | `0x1381A8` | `0x0d000000` | live bits clear; sticky bits 24/26/27 |

**Three consequences worth knowing before touching power on this card:**

1. **`PKG_POWER_SKU` reads 0 in both dwords**, so the hardware's own declared ceiling
   cannot be learned from MMIO at all, `power2_rated_max` is (correctly) hidden, and the
   driver's read-path clamp is inert.
2. **`0x1459A4` is an alias, not PL2.** It reads byte-identical to `0x1459A0` including the
   tau bits, which PL2 encodes independently and normally much shorter. **PL2's value is
   unknown and unreachable** — `xe` also never exposes it, gated out by
   `else if (attr != PL2_HWMON_ATTR)` at `xe_hwmon.c:1080`.
3. **`GT0_PERF_LIMIT_REASONS` has sticky log bits the driver never reads.** `xe` masks with
   `0xde3` (`xe_gt_throttle.c:93-97`), covering only bits 0–11. Bits 24/26/27 were set with
   all live bits clear — the sticky positions for **pl4, pl1, pl2**, i.e. all three have
   fired since boot. Cumulative and never cleared, so not attributable to one run, but it is
   the only way to ask "did this ever throttle?" — sysfs cannot.

There is **no instantaneous power sensor**: `xe`'s `hwmon_info[]` declares no
`HWMON_P_INPUT`/`HWMON_P_AVERAGE` on any platform. Derive watts from `Δenergy2_input/Δt`
(`scripts/gpu-survey.sh`). MangoHud therefore cannot show GPU power on `xe` at all — see
`pitfalls.md`.

### Throttle reasons are readable directly **[verified 2026-07-30]**

`xe` exposes the hardware throttle-reason register as sysfs booleans under
`/sys/class/drm/card0/device/tile0/gt0/freq0/throttle/`:

`reasons` (summary; `none` when clear), `status`, and one file each for `reason_pl1`,
`reason_pl2`, `reason_pl4`, `reason_prochot`, `reason_ratl`, `reason_thermal`,
`reason_vr_tdc`, `reason_vr_thermalert`.

This makes CLAUDE.md's "check for the throttle first" a **direct read**, not an inference
from clock behavior. Sample it every interval during any capture and discard runs where it
goes non-`none`.

The narrow engine set is the SKU, not a fault — expect few rows in any engine-busy view.

---

## Whole-session survey without root — `fdinfo` + sysfs **[verified 2026-07-30]**

`gputop` cannot be scripted (see `pitfalls.md`). Everything the layer-0 survey needs is
readable directly, unprivileged. Implemented in `scripts/gpu-survey.sh`.

### Engine busy — `/proc/<pid>/fdinfo/<drm fd>`

Per engine `E` in `rcs, bcs, vcs, vecs, ccs`:

```
busy_pct(E) = Δ drm-cycles-E / Δ drm-total-cycles-E × 100
```

`drm-total-cycles-E` is a wall-clock reference that advances at the same rate for every
client, so it is the denominator, **not** something to sum.

**Deduplicate by `drm-client-id`, never by fd.** One process holds several DRM fds with
*distinct* client-ids — `vkcube` showed 3 fds (`card0`, `renderD128` ×2) with client-ids
777 / 778 / 782. Summing per-fd double-counts. Sum `drm-cycles-*` across distinct
client-ids; take the max of `drm-total-cycles-*`.

Also present per client: `drm-total-vram0`, `drm-resident-vram0`, `drm-shared-vram0`,
plus `-gtt` and `-system` variants. **Values carry units** (`36288 KiB`, `4 MiB`, or a
bare `0`) — parse them, don't `atoi`.

There is no global VRAM-used counter in sysfs; total usage means summing across clients.

### Clocks, throttle, thermals — `/sys/class/drm/card0/device/`

| Path | Meaning |
|---|---|
| `tile0/gt0/freq0/act_freq` | actual clock; **reads `0` when the GT is idle-parked** — not a failed read |
| `tile0/gt0/freq0/cur_freq` | requested clock |
| `tile0/gt0/freq0/rp0_freq` | max boost, 2450 MHz — the number `act_freq` is judged against |
| `tile0/gt0/freq0/throttle/reasons` | `none` when clear; else the active reason |
| `hwmon/hwmon2/temp2_input` | °C ×1000 |
| `hwmon/hwmon2/fan1_input` | RPM |
| `hwmon/hwmon2/energy2_input` | µJ, monotonic — **there is no `power*_input`; derive watts from Δenergy/Δt** |

Observed idle-to-light-load baseline (`vkcube`, 2026-07-30): `rcs` ≈ 0.7 %, 35.4 MiB
VRAM, 39 °C, ~12.6 W against the 31.25 W limit, `reasons=none`.

---

## `INTEL_MEASURE` — per-interval GPU timestamps to CSV **[from docs]**

Collects GPU timestamps over intervals and writes a CSV. Overhead is the **flush required
at each interval boundary** — this is what makes fine granularity expensive. Output goes
to stderr unless `file=` is given.

Comma-separated options:

| Option | Effect |
|---|---|
| `file=PATH` | write CSV to PATH instead of stderr |
| `draw` | snapshot per render call — **the default**, and the trap (see `pitfalls.md`) |
| `rt` | snapshot on render-target change — good proxy for "one engine pass" |
| `type=batch` | snapshot at batch submission |
| `frame` | snapshot at frame boundaries — cheapest useful granularity |
| `start=N` | begin capturing at frame N |
| `count=N` | capture only N frames (`start=15,count=23` → frames 15–37) |
| `interval=N` | combine N events into one record; single start/end submitted where possible |
| `cpu` | collect CPU timestamps instead of GPU |
| `control=PATH` | **named-pipe start/stop control** — `echo N > PATH` captures N frames |

`control=` is the key mechanism for this project: it turns an unusable
whole-session-at-draw-granularity capture into an armed, on-demand one.

Type values are mutually exclusive.

### `control=` fifo semantics **[verified from Mesa source 2026-07-30; not yet exercised in-game]**

Read out of `src/intel/common/intel_measure.c` rather than inferred from the envvar docs,
because the exact wording matters for building a deferred-start capture:

| Fact | Source |
|---|---|
| Mesa **creates** the fifo itself if absent (`mkfifoat`), tolerating `EEXIST` | `:149-155` |
| Opened `O_RDONLY \| O_NONBLOCK`, held for process lifetime | `:157-158` |
| **With `control=` set, capture starts DISABLED** (`config.enabled = false`) | `:165-168` |
| The fifo is polled once per frame transition | `:349-353` |
| `echo N` (N > 0) → `enabled = true`, `end_frame = frame + N` | `:374-377` |
| **`echo 0` → `enabled = false`, stops immediately**, overriding any pending count | `:372-373` |
| Non-numeric input → disables capture and logs an error | `:366-371` |
| Several counts may be written at once; they are parsed in a loop | `:363-380` |

Two consequences this project relies on:

1. **Deferred start is real.** Launch the game with `control=` set and menus, shaders
   compiling and the walk to the test spot cost nothing. This is what lets a capture be
   armed only once the interesting scenario is on screen.
2. **Open-ended capture with a clean stop** is spelled: arm with an N you will never
   reach, then `echo 0` to end. That is how `scripts/record.sh` maps the capture onto
   Ctrl-C.

`read()` returning 0 (no writer) is treated as "no data", not as EOF-and-stop, so a
persistent reader survives repeated open/write/close cycles by the shell. Verified against
a reader mimicking Mesa's flags: `echo N` then `echo 0` both land.

Note `mkfifoat` is called with a **relative-to-CWD** path if the option is relative — pass
an absolute path.

---

## `MESA_GPU_TRACES` / u_trace — GPU render-stage timing **[verified 2026-07-30 — AVAILABLE on stock Fedora Mesa]**

Mesa's own tracing framework. Implemented by **ANV and Iris** among Intel drivers.

| Variable | Values / effect |
|---|---|
| `MESA_GPU_TRACES` | `print` (human-readable), `print_json` (parseable), `perfetto`, `markers`, `indirects` |
| `MESA_GPU_TRACEFILE` | output file instead of stdout |
| `INTEL_GPU_TRACEPOINT` | per-tracepoint enable/disable, additive/subtractive: `-blit,+render_pass` |

**RESOLVED (Plan 01 T2): u_trace IS compiled into Fedora's stock Mesa
`26.3.0-0.3.20260729`.** No local Mesa build is needed for render-stage tracing, so Plan
01 T7 gets the rich path and Plan 04 is de-risked.

```bash
MESA_GPU_TRACES=print_json MESA_GPU_TRACEFILE=/tmp/ut.json vkcube --c 120
```

`vkcube --c 120` produced 121 frames / 361 batches / 249 KB.

### `print_json` output shape **[verified 2026-07-30]**

Top-level is a **JSON array of frame objects**; each frame is
`{"frame": <int>, "batches": [...]}`; each batch is
`{"events": [...], "duration_ns": <int>}`; each event is
`{"event": <str>, "time_ns": <str>, "params": {...}}`.

**Types are inconsistent — do not assume.** `frame` and `duration_ns` are JSON *numbers*;
`time_ns` is a zero-padded **string** (`"0000901570334322"`).

Event vocabulary observed on ANV, and where the payload lives:

| Event | `params` |
|---|---|
| `intel_end_draw` | `count`, **`vs_hash`**, **`fs_hash`** |
| `intel_end_render_pass` | `width`, `height`, `msaa`, `att_count`, `command_buffer_handle` |
| `intel_end_blorp` | `op` (e.g. `HIZ_CLEAR`, `CCS_COLOR_CLEAR`), `width`, `height`, `samples`, `shader_pipe`, `src_fmt`, `dst_fmt`, `predicated` |
| `intel_end_cmd_buffer` | `command_buffer_handle`, `level` |
| `intel_end_btp` | `addr` |
| `intel_end_trace_copy_cb` | `count` |
| `intel_end_frame` | `frame` |
| every `intel_begin_*` | **empty** — all payload is on the matching `_end_` event |

Two consequences for Plan 02's parser: pair `begin`/`end` to get durations, and read
identity off the `end` event only. `vs_hash`/`fs_hash` on `intel_end_draw` give shader
identity for free — that is the hook for attributing cost to a specific shader in Plan 06.

Volume warning: u_trace has **no control fifo**, unlike `INTEL_MEASURE`. It records from
driver init, so a game session includes menus and loading. Trim with
`INTEL_GPU_TRACEPOINT` (e.g. `-blit,-copy,+render_pass,+draw,+frame`) and keep sessions
short.

`print`/`print_json` do **not** require a Perfetto-enabled Mesa; the `perfetto` value
does (see below). Perfetto traces can be collected without `MESA_GPU_TRACES=perfetto`
set, but events before the tracing session starts may be missed.

---

## `INTEL_DEBUG` — driver diagnostics **[from docs]**

| Flag | Effect |
|---|---|
| `fs`, `vs`, `cs` | dump generated shader assembly (grep for spills/fills) |
| `perf` / `fall` | emit messages about performance issues the app is hitting |
| `submit` | batchbuffer usage statistics |
| `stall` | stall the GPU after each draw/dispatch — **diagnostic only, destroys perf by design** |
| `sync` | CPU-wait for each batch to finish — same caveat |

---

## Perfetto + PPS — hardware counters **[from docs; `xe` support UNRESOLVED]**

Build Mesa with `-Dperfetto=true` to get `build/src/tool/pps/pps-producer`.

```bash
# Mesa
meson setup build -Dvulkan-drivers=intel -Dgallium-drivers=iris \
  -Dperfetto=true -Dbuildtype=debugoptimized
meson compile -C build

# Perfetto (pin a tag, e.g. v56.1)
./tools/install-build-deps && ./tools/gn gen --args='is_debug=false' out/linux
./tools/ninja -C out/linux

# capture
sudo sysctl dev.xe.observation_paranoid=0        # xe, NOT i915 — note the NAME, see below
sudo ./build/src/tool/pps/pps-producer &
sudo ./perfetto/out/linux/tracebox --system-sockets --txt \
  -c mesa/src/tool/pps/cfg/system.cfg -o out.perfetto-trace
```

- `INTEL_PERFETTO_METRIC_SET=RasterizerAndPixelBackend` selects a metric set.
- Root is required for system-wide counters; `CAP_PERFMON` on the binary is the
  alternative to `sudo`.
- Data sources: `gpu.counters.i915` (counters), `gpu.renderstages.intel` (render stages).
  **The counter source is documented only for `i915`** — resolve for `xe` in Plan 05.
- `duration_ms` in the trace config is the cleanest fixed-duration capture mechanism
  available (e.g. `120000` for a 2-minute session).
- **Useful split:** the PPS producer reads the kernel OA interface directly, independent
  of the game's Mesa. Counters therefore work with the game on stock Mesa; only
  *render-stage* tracing requires the game to run on the Perfetto-enabled build.
- **Counter semantics:** OA counters are sampled at fixed intervals across the whole GPU,
  **not attributed per-draw**. They characterize a *time window*. Correlate against the
  render-stage track, never against an individual draw.

View traces at [ui.perfetto.dev](https://ui.perfetto.dev).

---

## Out-of-tree Mesa without disturbing the system **[from docs]**

```bash
VK_DRIVER_FILES=/path/to/mesa/build/src/intel/vulkan/intel_icd.x86_64.json <app>
```

`-Dbuildtype=debugoptimized` is the right build type: a plain `debug` build's numbers are
meaningless for perf work, a plain `release` build is painful to step through.

**Always verify it loaded** — `vulkaninfo | grep driverInfo` must report your build, not
the distro's. A silently-unloaded build measures as a perfect no-op (see `pitfalls.md`).

---

## Proton / translation layers **[from docs]**

Under Proton the game is a **Vulkan** app by the time it reaches ANV. Palworld is UE5;
D3D11 by default, D3D12 via the `-dx12` launch option.

| Path | Layer | Debug markers |
|---|---|---|
| D3D11 (default) | DXVK | `DXVK_DEBUG=markers` — forwards app resource names and markers via `VK_EXT_debug_utils` |
| D3D12 (`-dx12`) | vkd3d-proton | `VKD3D_CONFIG` — **option name has moved between releases; read the installed version's docs** |

Pair either with `MESA_GPU_TRACES=markers` so u_trace records the labels. Without markers,
traces show DXVK's anonymous Vulkan objects instead of UE5 pass names
(`BasePass`, `ShadowDepths`, `Lumen*`, `PostProcessing`).

`-dx12` changes which translation layer is being profiled, i.e. it changes the workload.
Profile the one actually played.

Other vkd3d-proton diagnostics: `VKD3D_CONFIG=breadcrumbs` (instruments command lists via
`VK_AMD_buffer_marker` / `VK_NV_device_checkpoints`, dumps on device-lost),
`VKD3D_SHADER_DEBUG_RING_SIZE_LOG2=N` (shader printf ring).

---

## Deterministic replay — GFXReconstruct **[from docs]**

```bash
# capture (F12 triggers by default; re-pressing overwrites)
VK_INSTANCE_LAYERS=VK_LAYER_LUNARG_gfxreconstruct <app>

# replay against any driver build
VK_DRIVER_FILES=/path/to/build/intel_icd.x86_64.json gfxrecon-replay capture.gfxr
```

- The **D3D11/DXVK → Vulkan** capture path is the well-trodden one. **vkd3d-proton capture
  is listed as future work upstream** — expect breakage on the `-dx12` route and be ready
  to fall back to DXVK for the replay loop specifically.
- Captures are large; think seconds, not minutes.
- RenderDoc enables cached host memory by default under vkd3d-proton, for capture speed
  and stability.

Fallback if replay proves unworkable: fixed save file + fixed camera position +
`INTEL_MEASURE=type=frame`. Noisier, but it works.

---

## Palworld specifics **[from docs]**

- **No kernel anti-cheat** — no EAC, no BattlEye, no VAC. Vulkan layer injection works.
  This is why this title was chosen as the reference workload; it does not generalize.
- UE5. Steam Deck Playable / works on desktop Linux under Proton.
- `-dx12` launch option exists; community reports mixed stability vs. DX11.
