# gpu — Intel Arc GPU performance work

Profiling and optimizing the **Intel Arc A310** (DG2/G11) under real proprietary game
workloads on Linux, with the eventual target being **Mesa's ANV** (the Intel Vulkan
driver) itself.

The reference workload is **Palworld** (UE5) under Proton. The reference machine is
`station-lan`: Fedora 44, Mesa `26.3.0` git snapshot, **`xe` kernel driver**.

---

## The One Thing That Makes This Hard

**Every measurement perturbs the thing being measured, and the workload is not
reproducible.**

This is the inverse of a normal software project. There, you write a test, it passes or
fails, and the answer is the same every run. Here:

- **The workload is a closed-source game you cannot script.** Two playthroughs of
  "the same" two minutes render different geometry, at a different camera angle, with
  different pals on screen. Naive A/B testing of a driver patch is worthless.
- **The instrumentation is the bottleneck.** `INTEL_MEASURE=draw` inserts a flush at
  every draw boundary. Turn it on and the frame times you measure are *caused by* the
  measurement. Fine-grained tools are only usable over short windows.
- **The stack has four layers you don't own.** Game → DXVK/vkd3d-proton → ANV → `xe`.
  A regression can live in any of them, and the game's D3D calls reach the driver as
  Vulkan, stripped of the engine's own names unless debug markers are on.
- **The hardware lies about being idle.** Thermal and power limits silently change
  clocks between runs. A "10% improvement" is frequently a cooler room.

So the discipline of this project is **not** "write code that works." It is
**"establish that a number is real before believing it."** Everything below serves that.

Two consequences that shape every plan here:

1. **Deterministic replay is infrastructure, not a nice-to-have.** Capture the Vulkan
   stream once (GFXReconstruct), then replay that identical stream against each driver
   build. Without this, no patch can be evaluated. Build it early.
2. **Measure top-down, never bottom-up.** Establish GPU-bound vs. CPU-bound vs.
   thermally-capped *first*. Optimizing a shader on a CPU-bound workload is wasted work,
   and it is the default failure mode of this kind of project.

---

## Project Goal

Find real, measurable performance wins for Arc Alchemist on modern game workloads, and
land them in ANV where possible.

### Core Features
- **A profiling funnel**: layered capture from near-zero-overhead whole-session
  sampling down to single-frame exhaustive inspection, each layer justifying the next.
- **Deterministic replay harness**: capture once, replay against N driver builds,
  compare with known noise bounds.
- **Trace analysis tooling**: aggregate `INTEL_MEASURE` CSV and u_trace JSON into
  ranked pass-cost tables. This is the only part not already provided by existing tools.
- **A local Mesa build loop**: patch ANV, rebuild, replay, compare — without disturbing
  the system Mesa.

---

## Critical Facts About This Hardware

**The box runs the `xe` kernel driver, not `i915`.** Nearly every Intel GPU profiling
guide on the internet assumes `i915`, and the tools fail in confusing ways rather than
saying "wrong driver." Confirmed differences:

| Thing | `i915` | `xe` (ours) |
|---|---|---|
| Engine-busy top | `intel_gpu_top` | **`gputop`** (`intel_gpu_top` is unsupported and has been reported to crash) |
| OA paranoia sysctl | `dev.i915.perf_stream_paranoid` | **`dev.xe.observation_paranoid`** — renamed; the `perf_stream_paranoid` name does **not** exist under `dev.xe` |
| Mesa Perfetto data source | `gpu.counters.i915` | **unverified** — see `context/pitfalls.md` |

**Always confirm which driver is bound (`lspci -k -s 03:00.0`) before trusting any
recipe.** Alchemist can run on either, and it is switchable at boot via `force_probe`.

**The A310 is a small GPU and its limits are real:** ~3.95 GiB usable VRAM (4 GiB
physical), 96 EUs, and most of the media/compute engines fused off on this SKU
(`ccs0/2/3`, most `vcs`/`vecs` — that's the hardware, not a bug). On a UE5 title, VRAM
pressure is a live hypothesis, not a footnote. Check it before blaming shaders.

**Palworld ships no kernel anti-cheat** — no EAC, no BattlEye, no VAC. Vulkan layer
injection (RenderDoc, GFXReconstruct, MangoHud, validation) works. This is unusual for a
proprietary game and it is *why this workload was chosen*. Do not assume it generalizes
to other titles.

---

## Architecture

### Stack
- **Analysis tooling:** Rust (edition current) for anything that lives, Python for
  throwaway exploration. Trace files are large — prefer streaming parsers over
  slurp-then-parse.
- **Driver work:** C, in a local Mesa tree. Meson build, `debugoptimized`.
- **Capture/replay:** GFXReconstruct (Vulkan stream), RenderDoc (single frame),
  Perfetto (timeline + counters).
- **Build/verify:** `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test` for our
  code; `meson compile` for Mesa.

### Workspace Layout
```text
gpu/
├── CLAUDE.md                 # This file
├── context/
│   ├── plans/                # Active plans — read RULES.md & SERIES.md FIRST
│   │   ├── RULES.md          # Plan format + per-task build/commit rules (authoritative)
│   │   ├── SERIES.md         # Cross-plan dependency chain (NN → NN)
│   │   ├── NN_example.md     # Canonical template — copy this for every new plan
│   │   ├── completed/        # Done plans (historical examples)
│   │   ├── abandoned/        # Plans dropped with a recorded reason
│   │   └── NN_name.md        # Each non-trivial change gets a plan + tracker
│   ├── distilled.md          # Confirmed tool/driver/hardware facts (read before new work)
│   ├── pitfalls.md           # Bugs & measurement traps (read before new work)
│   └── high_level.md         # Tool & library pros/cons
├── vendor/                   # READ-ONLY reference clones (gitignored)
├── scripts/                  # Capture wrappers (record.sh, measure-ctl.sh)
├── analyze/                  # Trace aggregation tooling
└── captures/                 # Trace/capture output — GITIGNORED, never commit
```

---

## Domain Knowledge

### The Profiling Funnel

Run wide-and-cheap first; each layer earns the next. **Never start at the bottom.**

| Layer | Tool | Window | Overhead | Answers |
|---|---|---|---|---|
| 0 | `gputop`, MangoHud | full session | ~none | GPU-bound? CPU-bound? throttled? VRAM-capped? |
| 1 | `INTEL_MEASURE=type=frame` | full session | low | *Which frames* are bad? |
| 2 | `INTEL_MEASURE=type=rt` / u_trace | full session | moderate | *Which passes* dominate? |
| 3 | `INTEL_MEASURE=type=draw` + control FIFO | seconds | high | *Which draws*? |
| 4 | RenderDoc | one frame | n/a (offline) | Everything about that frame |
| 5 | Perfetto + PPS counters | timed window | moderate | *Why* — EU / sampler / bandwidth bound |

**Key mechanism:** `INTEL_MEASURE=control=/path/to.fifo` is a literal start/stop button.
`echo 10 > fifo` captures the next 10 frames. This is how you get layer-3 detail without
layer-3 cost — reproduce the bad scenario, then arm the capture.

Full env-var syntax lives in `context/distilled.md`. Read it before writing capture code.

### Where the Names Come From

By default your traces show DXVK's Vulkan objects, not Unreal's passes. UE5 emits
`ID3DUserDefinedAnnotation` / PIX markers; the translation layers can forward them as
`VK_EXT_debug_utils` labels (`DXVK_DEBUG=markers`, pair with `MESA_GPU_TRACES=markers`).
`BasePass is 40% of frame time` is actionable; `render target 0x7f2a… is 40%` is not.
Turn markers on before doing any pass-level analysis.

### Vendor Map (`vendor/`, gitignored — clone what you need)

| Clone | Read it for |
|---|---|
| `mesa/` | ANV source — `src/intel/vulkan/`. The subject of this project. Also `src/tool/pps/` for the Perfetto producer. |
| `perfetto/` | Trace config format, UI. Pin a release tag. |
| `dxvk/` | What D3D11 calls become in Vulkan; the `DXVK_DEBUG` / HUD options. |
| `vkd3d-proton/` | Same for D3D12. `VKD3D_CONFIG` options move between releases — read the version you have. |
| `igt-gpu-tools/` | `gputop` internals; how `xe` engine busy is actually derived from fdinfo. |

Source beats API docs for all of these. Read the C.

---

## Development Workflow

### 1. Planning — MANDATORY before any non-trivial work
1. Read `context/plans/RULES.md` **in full** (format, metadata block, required
   sections, Rules A–E).
2. Read `context/plans/SERIES.md` for the dependency chain.
3. Copy `context/plans/NN_example.md` → `context/plans/NN_name.md`, plus a paired
   `NN_name_tracker.md`. Number it to continue SERIES.
4. Execute task-by-task; update the tracker as you go; `git mv` to `completed/` when done.

### 2. Knowledge Management
- **`context/distilled.md`** — confirmed facts: env-var syntax, counter semantics,
  hardware limits, what a tool actually does vs. what its docs claim. Read before new
  work; append after every session that learns something.
- **`context/pitfalls.md`** — every bug, gotcha, and **especially** every measurement
  that turned out to be wrong. Template: `# Title → Problem → Fix → Source`.
  *(Cross-cutting entries also mirror up to `../context/pitfalls.md` per the slop
  convention. Keep Intel/Arc-specific ones local.)*
- **`context/high_level.md`** — short pros/cons for tool and library choices.
- **`../context/`** (repo-level) — anything that generalizes beyond this GPU.

### 3. Measurement Discipline — this project's version of "tests"

This replaces "write tests first" as the primary quality gate, because a wrong number is
this project's equivalent of a failing test that reports success.

- **No single-run numbers, ever.** Report N ≥ 3 runs with the spread. A delta smaller
  than the observed run-to-run spread is **not a result** — say "within noise."
- **State the capture.** Every performance claim names the capture file, driver build,
  and settings it came from. A number without provenance is not admissible.
- **Never compare across changed conditions.** Different kernel driver, different Mesa
  build, different resolution, different thermal state → different machine. If you must
  cross that line (e.g. `i915` for counters), say so explicitly and only draw
  conclusions that port (*"this pass is sampler-bound"*), never absolute timings.
- **Check for the throttle first.** Clocks below boost while busy invalidates the run.
  Re-measure, don't reason about it.
- **Prefer replay to gameplay.** If a claim can be made against a deterministic replay,
  it must be.

### 4. Code Quality
- **No type suppression:** no `.unwrap()` without justification, no silent `as`
  truncation, no `unsafe` without a SAFETY comment.
- **Small modules:** functions < ~50 lines, single responsibility.
- **Docs:** `///` on public items in `analyze/`. Trace-format parsers should cite the
  Mesa source that emits the format they read.
- **Streaming over slurping:** trace files reach gigabytes. Code that reads a whole
  capture into memory will work on your test file and die on a real one.

### 5. Build Verification — never commit broken code
- **Our Rust:** `cargo build` exits 0 with **zero warnings**, `cargo clippy` clean,
  `cargo test` green, `cargo fmt` applied — *before every commit*.
- **Mesa patches:** `meson compile -C build` exits 0 and introduces **zero new
  warnings** in touched files, and the built driver actually loads
  (`vulkaninfo | grep driverInfo` reports your build).
- If the build breaks, **fix it first.** Do not claim "done" on broken code.
- `context/plans/RULES.md` Rule A is authoritative and stricter; defer to it.

### 6. Commits

**CRITICAL: COMMIT AT EVERY TASK COMPLETION. DO NOT WAIT.**

- Small, frequent, one task per commit. Format: `task(TN): <description>`.
- **YOU MUST COMMIT BEFORE MARKING ANY TASK COMPLETE.**
- **NEVER batch multiple tasks into one commit** unless truly inseparable.
- Fix all warnings and ensure tests pass **before** every commit.
- Never push — the human pushes after review. No co-author trailers unless asked.
  *(Global rule, `~/.claude/CLAUDE.md`.)*
- Git history is **append-only**. No amend, rebase, reset, revert, or force-push. A
  wrong commit is corrected by a new commit that says what was wrong.
  *(Repo rule, `../CLAUDE.md` — read it, it has an incident report attached.)*

### 7. Tooling
- **No `tmp/` scripts.** Helpers live in `scripts/` (capture wrappers) or `analyze/`
  (parsers). If you ran it twice, it belongs in the repo.

### 8. Delegation
- Stuck on driver behavior? **Read `vendor/mesa/src/intel/` first** — the answer is in C.
- Then `context/distilled.md` / `pitfalls.md`. Only then ask.

---

## Constraints & Rules

1. **Measure before optimizing, top-down.** No shader work before establishing the
   workload is actually GPU-and-shader-bound. This is the default failure mode.
2. **A number without provenance and variance is not a result.** See Measurement
   Discipline above. This is the one rule that, if broken, wastes everything else.
3. **Never disturb the system Mesa.** Local builds run via
   `VK_DRIVER_FILES=/path/to/build/…/intel_icd.x86_64.json`. Do not `ninja install`
   over the distro's Mesa — you will lose the ability to boot into a known-good state,
   which is the reference every measurement is against.
4. **Never commit captures, traces, or build output.** `.gfxr`, `.rdc`,
   `.perfetto-trace`, `INTEL_MEASURE` CSVs, and `vendor/` clones are all gitignored.
   They are gigabytes and they are regenerable. If a build command can produce it, it
   does not belong in a commit.
5. **Respect that this is someone's game.** Profiling runs are on a real Palworld save
   on a real machine. Don't leave `observation_paranoid=0` set permanently, don't leave
   the system Mesa swapped out, and don't leave debug env vars in the Steam launch
   options after a session.
6. **No type suppression. No broken commits. No `tmp/` scripts.** (Above.)
7. **Honesty.** When you say you'll do something, do it, then say "done." Never claim
   something is recorded in `distilled.md`/`pitfalls.md`/`CLAUDE.md` unless the bytes
   are actually on disk. If a measurement was inconclusive, the finding is
   "inconclusive" — write that down, don't round it to a win.

---

## Getting Started

1. **Read the rules**: `context/plans/RULES.md`.
2. **Read the plan**: `context/plans/01_profiling_bringup.md` — Phase 0 verifies the box
   before anything else, and several of its checks gate everything downstream.
3. **Read what's known**: `context/distilled.md`, `context/pitfalls.md`.
4. **Execute Plan 01**, updating `01_profiling_bringup_tracker.md` as you go.

The suggested series beyond Plan 01 is in `context/plans/SERIES.md`.
