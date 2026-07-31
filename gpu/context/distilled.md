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
| Boost / efficient / min clock | `rp0_freq` **2450** MHz, `rpe_freq` 850, `rpn_freq` 300; idles at `act_freq` 850 |
| Package power limit | `power2_max` = **31.25 W** (`power2_max_interval` 28 s) |

Source: `dmesg | grep 'xe '`, `lspci`, `rpm -qa | grep mesa`, `sysctl dev.xe`,
`/sys/class/drm/card0/device/tile0/gt0/freq0/`, `.../device/hwmon/hwmon2/`.

**Do not use `rpa_freq`** — it reads `627200` while every neighbouring frequency is in MHz
(850 / 2450 / 300). Units are inconsistent or the value is meaningless; `act_freq` vs
`rp0_freq` is the pair to compare. **[verified 2026-07-30]**

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

## `INTEL_MEASURE` — per-interval GPU timestamps to CSV **[from docs]**

Collects GPU timestamps over intervals and writes a CSV. Overhead is the **flush required
at each interval boundary** — this is what makes fine granularity expensive. Output goes
to stderr unless `file=` is given.

Comma-separated options:

| Option | Effect |
|---|---|
| `file=PATH` | write CSV to PATH instead of stderr |
| `type=draw` | snapshot per render call — **the default**, and the trap (see `pitfalls.md`) |
| `type=rt` | snapshot on render-target change — good proxy for "one engine pass" |
| `type=batch` | snapshot at batch submission |
| `type=frame` | snapshot at frame boundaries — cheapest useful granularity |
| `start=N` | begin capturing at frame N |
| `count=N` | capture only N frames (`start=15,count=23` → frames 15–37) |
| `interval=N` | combine N events into one record; single start/end submitted where possible |
| `cpu` | collect CPU timestamps instead of GPU |
| `control=PATH` | **named-pipe start/stop control** — `echo N > PATH` captures N frames |

`control=` is the key mechanism for this project: it turns an unusable
whole-session-at-draw-granularity capture into an armed, on-demand one.

Type values are mutually exclusive.

---

## `MESA_GPU_TRACES` / u_trace — GPU render-stage timing **[from docs; availability in Fedora's build UNVERIFIED]**

Mesa's own tracing framework. Implemented by **ANV and Iris** among Intel drivers.

| Variable | Values / effect |
|---|---|
| `MESA_GPU_TRACES` | `print` (human-readable), `print_json` (parseable), `perfetto`, `markers`, `indirects` |
| `MESA_GPU_TRACEFILE` | output file instead of stdout |
| `INTEL_GPU_TRACEPOINT` | per-tracepoint enable/disable, additive/subtractive: `-blit,+render_pass` |

**Open question:** whether Fedora's stock Mesa is built with u_trace compiled in. Test
cheaply before relying on it:

```bash
MESA_GPU_TRACES=print_json MESA_GPU_TRACEFILE=/tmp/ut.json vkcube
ls -la /tmp/ut.json     # non-empty ⇒ available on stock Mesa
```

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
