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

**Ship patches.** Study the rendering pipeline until we know exactly where the time goes,
then read the code that spends it and write real optimisations — to Mesa/ANV, to the `xe`
kernel driver, to whatever else turns out to be responsible. Profiling is how we find the
target; the deliverable is a diff.

That ordering is the whole discipline. A patch written before the measurement justifies it
is a guess, and this hardware punishes guesses (see *The One Thing That Makes This Hard*).
But the measurement is not the point either — a beautiful profile that never becomes a
patch is a hobby.

Everything we author is a **patch against the source this box actually runs**, kept in
`patches/` under git and applied at RPM build time. See *Source & Build Environment*.

### Core Features
- **A profiling funnel**: layered capture from near-zero-overhead whole-session
  sampling down to single-frame exhaustive inspection, each layer justifying the next.
- **Deterministic replay harness**: capture once, replay against N driver builds,
  compare with known noise bounds.
- **Trace analysis tooling**: aggregate `INTEL_MEASURE` CSV and u_trace JSON into
  ranked pass-cost tables. This is the only part not already provided by existing tools.
- **A source-RPM build loop**: rebuild the exact installed Mesa and kernel from their
  source RPMs, with our patches applied, and install the result — so the thing we
  measure, the thing we read, and the thing we change are all the same build.

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
├── vendor/                   # Extracted source RPMs — GITIGNORED, regenerable
│   ├── srpms/                #   downloaded .src.rpm files
│   ├── mesa/                 #   rpmbuild topdir: SPECS/ SOURCES/ BUILD/ RPMS/
│   └── kernel/               #   same
├── patches/                  # OUR CHANGES — git-controlled. Read patches/README.md
│   ├── mesa/                 #   0001-*.patch + optional `series`
│   └── kernel/
├── scripts/                  # Capture wrappers + the vendor-*.sh build loop
├── analyze/                  # Trace aggregation tooling
└── captures/                 # Trace/capture output — GITIGNORED, never commit
```

---

## Source & Build Environment

**We rebuild the exact packages this box runs, from their source RPMs.** Reading upstream
git tells you what upstream does; it does not tell you what the binary in your address
space does. Fedora carries patches, the Mesa here is a COPR snapshot, and both move.

| Component | Installed from | Source of truth |
|---|---|---|
| Mesa / ANV | COPR `xxmitsu/mesa-git` | that COPR's SRPM |
| kernel (`xe`) | Fedora `updates` | `updates-source`, or koji when pruned |

### The loop

```bash
./scripts/vendor-prep.sh                          # fetch + extract SRPMs, install builddeps
./scripts/vendor-build.sh --component mesa        # apply patches/mesa/*, rpmbuild
./scripts/vendor-install.sh --component mesa      # install, prints the revert command
```

`vendor-prep.sh` is also the **resync** step: re-run it after any `dnf update` that moves
Mesa or the kernel.

### Two rules that make this correct

1. **Pin to the installed build, never to a package name.** `dnf download --srpm
   mesa-vulkan-drivers` resolves the *newest available*, which is not what you are running.
   Observed 2026-07-31: installed `…20260801.00.6c306fd`, dnf offered `…20260729.05.21dc9d4`
   — a three-day-old snapshot, silently. The scripts pin to `%{SOURCERPM}` of the installed
   binary, and for the kernel to `uname -r` rather than the newest installed kernel.
2. **Always `--refresh`.** With stale metadata even the correct pinned NEVRA reports "no
   package available". Same session: the installed EVR resolved only after `--refresh`.

### Patches

Nothing we author goes in `vendor/` — it is gitignored and gets re-extracted on every RPM
update. `vendor-build.sh` injects `patches/<component>/*` into a **copy** of the distro
spec (`%autosetup` → `Patch NNNN:` declarations; `%setup` → declarations plus explicit
`%patch -P` in `%prep`; Fedora kernel → the `linux-kernel-test.patch` slot). An
unrecognised spec shape is a **hard error**: silently building unpatched would "measure" as
a patch with no effect, which is the same failure class as a driver that never loaded.

Build the pristine baseline with `--no-patches` and compare against **that**, not against
the distro RPM — same toolchain, same box, one variable.

> **Status: the scripts are verified, a full build is not.** Verified 2026-07-31: SRPM
> resolution for both components (including the stale-metadata trap), patch injection into
> both `%autosetup` and `%setup` specs parsing cleanly under `rpmbuild --nobuild`,
> `--no-patches` producing a byte-identical spec, and a missing `series` entry failing
> hard. **Not yet run:** a real `vendor-prep.sh` download, a full Mesa or kernel build, or
> an install. Update this note once they have been.

---

## Domain Knowledge

### The Profiling Funnel

Run wide-and-cheap first; each layer earns the next. **Never start at the bottom.**

| Layer | Tool | Window | Overhead | Answers |
|---|---|---|---|---|
| 0 | `gputop`, MangoHud | full session | ~none | GPU-bound? CPU-bound? throttled? VRAM-capped? |
| 1 | `INTEL_MEASURE=frame` | full session | low | *Which frames* are bad? |
| 2 | u_trace (`--mode utrace --markers`) | full session | moderate | *Which passes* dominate? **Only u_trace can name them** — `INTEL_MEASURE=rt` bounds the cost but never says which pass (`pitfalls.md`). |
| 3 | `INTEL_MEASURE=draw` + control FIFO | seconds | high | *Which draws*? |
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

### Vendor Map (`vendor/`, gitignored)

Two kinds of tree live here, and the difference matters.

**Rebuildable — from source RPMs, via `scripts/vendor-prep.sh`.** These are what this box
actually runs, and what we patch:

| Tree | Read it for |
|---|---|
| `mesa/BUILD/mesa-*/` | ANV — `src/intel/vulkan/`. The subject of this project. Also `src/tool/pps/` for the Perfetto producer. |
| `kernel/BUILD/kernel-*/` | `xe` — `drivers/gpu/drm/xe/`. Engine busy, VM_BIND, power/freq management. |

**Reference-only — plain clones, for reading.** Not rebuilt, not patched:

| Clone | Read it for |
|---|---|
| `perfetto/` | Trace config format, UI. Pin a release tag. |
| `dxvk/` | What D3D11 calls become in Vulkan; the `DXVK_DEBUG` / HUD options. |
| `vkd3d-proton/` | Same for D3D12. `VKD3D_CONFIG` options move between releases — read the version you have. |
| `igt-gpu-tools/` | `gputop` internals; how `xe` engine busy is actually derived from fdinfo. |

Source beats API docs for all of these. Read the C. For Mesa and the kernel, read the
*extracted SRPM*, not an upstream clone — a distro patch you did not know about is exactly
the kind of thing that makes a measurement inexplicable.

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
- **Mesa / kernel patches:** `./scripts/vendor-build.sh --component <c>` exits 0 and
  introduces **zero new warnings** in touched files, and the installed build actually
  loads — `vulkaninfo | grep driverInfo` for Mesa, `uname -r` for the kernel. Verifying
  the load is not optional: a build that never loaded measures as a perfect no-op.
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
- Stuck on driver behavior? **Read `vendor/mesa/BUILD/mesa-*/src/intel/` first** — the answer is in C, in the build this box runs.
- Then `context/distilled.md` / `pitfalls.md`. Only then ask.

---

## Constraints & Rules

1. **Measure before optimizing, top-down.** No shader work before establishing the
   workload is actually GPU-and-shader-bound. This is the default failure mode.
2. **A number without provenance and variance is not a result.** See Measurement
   Discipline above. This is the one rule that, if broken, wastes everything else.
3. **Replacing the system Mesa is allowed, but never casually.** *(Amended 2026-07-31.
   This rule previously said "never" and required `VK_DRIVER_FILES` side-loading. That
   does not survive contact with the kernel half of the project: `xe` cannot be
   side-loaded, and running a patched Mesa against a stock kernel — or vice versa — makes
   the two halves incomparable. `scripts/vendor-install.sh` now installs system-wide.)*
   The obligations that replace it:
   - Know the way back **before** you install. `vendor-install.sh` records the outgoing
     NEVRAs to `vendor/installed-<component>.log` and prints the `dnf history undo` id.
   - Never install a build you have not compared against a `--no-patches` baseline built
     the same way.
   - Kernels are parallel-installable — the distro kernel stays bootable, so a panic is
     recoverable at the boot menu. Mesa is not. Treat a Mesa install as the higher-risk
     one, not the lower.
   - `VK_DRIVER_FILES` side-loading is still the right tool for a quick Mesa-only A/B
     that does not need a matching kernel. Use it when it fits.
4. **Never commit captures, traces, or build output.** `.gfxr`, `.rdc`,
   `.perfetto-trace`, `INTEL_MEASURE` CSVs, and everything under `vendor/` are gitignored.
   They are gigabytes and they are regenerable. If a build command can produce it, it
   does not belong in a commit. **The exception is `patches/`** — that is authored, not
   generated, and it is the point of the project.
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
