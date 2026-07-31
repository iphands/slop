# High-level — tool & library choices

Short pros/cons for the tools this subproject depends on. Keep entries brief; detailed
syntax belongs in `distilled.md`, failure modes in `pitfalls.md`.

---

## Profiling tools — what each is actually for

| Tool | Window | Overhead | Needs custom Mesa? | Answers |
|---|---|---|---|---|
| **`gputop`** | full session | ~none | no | Engine busy, clocks, power, VRAM. The "is this even GPU-bound?" tool. `xe`-aware — use instead of `intel_gpu_top`. |
| **MangoHud** | full session | ~none | no | Frametimes + system stats, with `log_duration` for fixed windows. Correlates CPU and GPU sides. |
| **`INTEL_MEASURE`** | tunable | low→high by `type=` | no | Per-frame / per-RT / per-draw GPU timing to CSV. The workhorse. Has a control FIFO for on-demand capture. |
| **u_trace** (`MESA_GPU_TRACES=print_json`) | full session | moderate | **unverified** — test on stock Fedora Mesa | Render-stage timing, richer than `INTEL_MEASURE=rt`. |
| **Perfetto + PPS** | timed window | moderate | **yes** | Hardware counters (EU / sampler / bandwidth occupancy) + CPU⇄GPU timeline. The only tool that answers *why*. `xe` support unresolved. |
| **RenderDoc** | one frame | n/a (offline) | no | Everything about one frame: pipeline state, per-draw timing, resources, shader source. |
| **GFXReconstruct** | seconds | capture-time | no | Deterministic capture/replay. Not a profiler — it's what makes profiling *comparable*. |

**Selection rule:** go down this table, not up. Each row costs more and narrows the
question; earning the next row means the previous one said the bottleneck is really
there. Starting at RenderDoc is how you spend a week optimizing a pass on a CPU-bound
workload.

### `gputop` vs `intel_gpu_top`

`intel_gpu_top` (igt-gpu-tools) is the well-known one and every guide uses it. It **does
not support `xe`** and has been reported to crash on it. `gputop`, same package, is the
`xe`-aware replacement. If Fedora's `igt-gpu-tools` predates it, `xe` engine busy is
readable directly from `/proc/<pid>/fdinfo/<drm-fd>` (`drm-cycles-*`,
`drm-total-cycles-*`) — which is what `gputop` reads anyway, and needs no root.

### Why Perfetto over "just more `INTEL_MEASURE`"

`INTEL_MEASURE` says *how long* a pass took. It cannot say *why*. On a 96-EU, 3.95 GiB
part, "slow" has at least three unrelated causes — EU-bound, sampler-bound,
bandwidth-bound — with completely different fixes. Guessing among them wastes days, and
the counters are the only thing that distinguishes them. That is the entire justification
for the Mesa rebuild in Plan 04.

### Why deterministic replay is infrastructure, not tooling

A driver patch cannot be A/B'd by playing the game twice — the frames differ. Replay
turns an unreproducible workload into a fixed one. Its second job is equally important:
**replaying the same capture twice establishes the noise floor**, and every later claim
is judged against that number (RULES.md Rule D).

---

## Kernel drivers — `xe` vs `i915` on Alchemist

|  | `i915` | `xe` |
|---|---|---|
| Maturity | older, more tooling assumes it | newer, designed for modern Intel GPUs + modern DRM interfaces |
| Perf on Arc | baseline | reported incremental gains, notably OpenCL/compute |
| GuC/HuC | varies | GuC/HuC enabled by default |
| Tooling | everything | `gputop` only; PPS counter support unresolved |
| **Ours** | fallback for counter work if PPS can't speak `xe` | **default — what we measure and play on** |

Switchable at boot: `xe.force_probe=!56a6 i915.force_probe=56a6`. Cross-driver numbers
answer only portable questions about shader behavior — never absolute timings
(`pitfalls.md`).

---

## Analysis code — language choice

| Option | Use for |
|---|---|
| **Rust** | Anything that lives in `analyze/`. Matches the repo's habits; streaming parsers over multi-GB traces want the performance and the type safety. |
| **Python** | Throwaway exploration, one-off plots, checking a hypothesis before committing to it. Do not let it accrete into infrastructure. |

**Constraint that dominates the design:** trace files reach gigabytes. Slurp-then-parse
works on your test file and dies on a real capture. Stream.

Relevant crates already characterized elsewhere in this repo — see `../../context/` for
`rayon` (parallel aggregation), `bytemuck` (zero-copy binary parsing), and the general
`patterns.md` / `algo.md` notes.
