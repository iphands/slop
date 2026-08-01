# Plan 01 — Profiling bring-up

> **Status**: pending
> **Created**: 2026-07-30
> **Depends on**: N/A
> **Goal**: Verify the profiling toolchain on `station-lan`, then run the full funnel on a real 2-minute Palworld session to produce a ranked pass-cost table and a GPU-bound / CPU-bound / throttled verdict.
> **Hardware**: A310 / DG2-G11 / `xe`, stock Fedora Mesa `26.3.0-0.3.20260729`. **No custom Mesa build** — that is Plan 04.

---

> **Before starting, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Establish which profiling tools actually work on this box, then use them
top-down on a real Palworld session to find out where the frame time goes.

**Deliverables**:
1. A verified tool inventory: which of `gputop`, u_trace, `INTEL_MEASURE`, MangoHud,
   RenderDoc work here, recorded in `context/distilled.md` with [verified] markers.
2. A **verdict**: GPU-bound, CPU/DXVK-bound, or thermally/power-capped — with the
   `gputop` + MangoHud data backing it.
3. A **ranked pass-cost table** from a real 2-minute session, with UE5 pass names.
4. A reproducible in-game scenario ("the base at dusk with N pals") that reliably
   produces the bad frames, for later plans to target.

**Estimated effort**: Medium (1 day), most of it play time and iteration.

---

## Context

This is the first plan of the series and everything downstream is gated on it. The goal
of the subproject is to optimize ANV, but optimizing before measuring — and specifically
before establishing *what kind* of bottleneck this is — is the default failure mode of
GPU performance work. A shader optimization on a CPU-bound workload is wasted effort, and
it is very easy to spend a week on one without noticing.

See `CLAUDE.md` → "The One Thing That Makes This Hard" for why this project inverts the
normal test-driven shape.

### Key Facts

Established while planning; full syntax in `context/distilled.md`.

- **The box runs `xe`, not `i915`.** Confirmed from `dmesg`: `Initialized xe 1.1.0 for
  0000:03:00.0`, DG2/G11 device `56a6`. This invalidates most Intel profiling recipes
  found online — see `context/pitfalls.md`. Concretely: `gputop` not `intel_gpu_top`,
  `dev.xe.observation_paranoid` not `dev.i915.perf_stream_paranoid` (the knob is **renamed**
  under `xe`, not merely re-namespaced — see `context/pitfalls.md`).
- **3.95 GiB usable VRAM** on a UE5 title makes residency a live hypothesis, not a
  footnote. Check it in T5 before blaming shaders.
- **Palworld ships no kernel anti-cheat** (no EAC/BattlEye/VAC), so Vulkan layer
  injection works. T4 confirms this empirically — the rest of the plan depends on it.
- **Under Proton the game reaches ANV as Vulkan**, translated by DXVK (D3D11, default) or
  vkd3d-proton (D3D12, via `-dx12`). Pass names are DXVK's unless debug markers are on
  (T9).
- **`INTEL_MEASURE` defaults to per-draw and flushes at every boundary.** Running that
  for two minutes produces ~10⁷–10⁸ rows *and* perturbs the timings being measured. The
  whole structure of this plan is built around avoiding that — see `context/pitfalls.md`.

### Why the funnel structure

Each layer is cheaper and wider than the next, and each earns the next:

| Task | Tool | Window | Overhead | Answers |
|---|---|---|---|---|
| T5 | `gputop`, MangoHud | full session | ~none | GPU-bound? throttled? VRAM-capped? |
| T6 | `INTEL_MEASURE=frame` | full session | low | Which *frames* are bad? |
| T7 | `INTEL_MEASURE=rt` / u_trace | full session | moderate | Which *passes* dominate? |
| T8 | `INTEL_MEASURE=draw` + FIFO | seconds | high | Which *draws*? |
| T10 | RenderDoc | one frame | offline | Everything about that frame |

Going in the other order is how the week gets wasted.

---

## Step-by-Step Tasks

### T1: Record the tool inventory for `xe`

**File**: `context/distilled.md`

**What to do**: The driver question is already answered (`xe`), but the consequences are
not yet verified on the box. Confirm each:

```bash
lspci -k -s 03:00.0 | grep -i 'kernel driver'   # expect: xe
sysctl dev.xe                                   # enumerate; do NOT grep a remembered name
```

Promote the confirmed entries in `context/distilled.md` from **[from docs]** to
**[verified]**. Do not promote what you did not observe.

**Expected observation**:
- **Confirms**: `xe` bound; `dev.xe.observation_paranoid` exists.
- **Refutes**: an `i915` binding — in which case the whole `xe` section of
  `pitfalls.md` is moot for this session and the plan's tool choices revert to the
  conventional ones. Say so loudly; do not quietly use `i915` tools on an `xe` box or
  vice versa.

**Commit**: `task(T1): verify xe driver facts on station-lan`

---

### T2: Does stock Fedora Mesa have u_trace compiled in?

**File**: `context/distilled.md`

**What to do**: This is the linchpin for T7 and could not be determined from Fedora's
spec remotely. Test on anything Vulkan:

```bash
sudo dnf install vulkan-tools
MESA_GPU_TRACES=print_json MESA_GPU_TRACEFILE=/tmp/ut.json vkcube
ls -la /tmp/ut.json
```

**Expected observation**:
- **Confirms**: `/tmp/ut.json` is non-empty → u_trace works on stock Mesa, T7 can use the
  richer render-stage path.
- **Refutes**: empty or absent → u_trace is not built in. T7 falls back to
  `INTEL_MEASURE=rt` only, and render-stage tracing moves to Plan 04. **This is not
  a blocker** — record it and continue.

**Commit**: `task(T2): record u_trace availability on stock Fedora Mesa`

---

### T3: Install the no-build-cost tooling

**What to do**:

```bash
sudo dnf install igt-gpu-tools vulkan-tools mangohud renderdoc
sudo gputop        # must actually report on the A310
```

If `igt-gpu-tools` is too old to ship `gputop`, fall back to reading
`/proc/<pid>/fdinfo/<drm-fd>` (`drm-cycles-*`, `drm-total-cycles-*`) — that is what
`gputop` reads anyway, and it needs no root. If you write that fallback, it goes in
`scripts/`, not `/tmp` (RULES.md, no `tmp/` scripts).

**Expected observation**: `gputop` shows non-zero engine busy under any GPU load. Zeros
under known load means driver mismatch, not an idle GPU (`pitfalls.md`).

**Commit**: `task(T3): install profiling tooling; note gputop availability`

---

### T4: Confirm Vulkan layer injection works

**What to do**: Steam launch options → `mangohud %command%`. Launch Palworld once.

**Expected observation**:
- **Confirms**: the overlay draws → layers inject, RenderDoc and GFXReconstruct (Plan 03)
  are viable.
- **Refutes**: no overlay → something blocks injection after all. **This invalidates the
  premise of Plan 03** and must be recorded prominently in `pitfalls.md`, since the whole
  series assumes injection works.

**Commit**: `task(T4): confirm Vulkan layer injection under Proton`

---

### T5: Whole-session survey — the bound/throttle verdict

**File**: `scripts/record.sh` (mode `survey`), `captures/`

**What to do**: Run for the full 2 minutes, before anything else.

```bash
sudo gputop -J -s 200 > captures/gputop_$(date +%F)_baseline.json
```

Check `gputop --help` for the actual flags — its JSON/interval options track
`intel_gpu_top`'s but the tool is younger and they are not guaranteed identical.

Steam launch options:

```
MANGOHUD_CONFIG=fps,frametime,gpu_stats,gpu_temp,gpu_core_clock,gpu_mem_clock,cpu_stats,ram,vram,log_duration=120,output_folder=/home/you/prof mangohud %command%
```

**Expected observation** — this task's whole output is one of three verdicts:
- **GPU-bound**: render busy consistently >90%. Continue to T6.
- **CPU/DXVK-bound**: render busy low, frametimes spiky. Continue, but the ANV work worth
  doing is submission and pipeline-compile overhead, not shader cost. **This changes
  Plan 06's target** — record it in SERIES.
- **Throttled**: clocks well below boost while busy, or VRAM pinned near 3.95 GiB. Fix
  the throttle and re-measure. **Any profiling done under a throttle measures the
  throttle** — do not proceed on that data.

Record in the tracker's Measurements table with N ≥ 3 sessions (RULES.md Rule D).

**Commit**: `task(T5): whole-session survey + bound/throttle verdict`

---

### T6: Whole-session, coarse — which frames are bad?

**What to do**:

```
INTEL_MEASURE=frame,file=/home/you/prof/frames.csv mangohud %command%
```

~7200 rows for two minutes — trivial volume, negligible overhead. Sort by duration,
correlate the worst frames' timestamps against what was happening in-game (a base with
many pals, weather, view distance).

**Expected observation**: a small number of identifiable in-game situations account for
most of the bad frames → **a reproducible scenario**, which is the real deliverable here.
If bad frames are uniformly distributed with no correlate, say so — that points at
something systemic (compile hitching, memory) rather than a specific pass, and it changes
what T7 is looking for.

**Commit**: `task(T6): per-frame capture + identify the bad-frame scenario`

---

### T7: Whole-session, mid-grain — which passes dominate?

**What to do**: Run whichever T2 established is available.

Always available:
```
INTEL_MEASURE=rt,file=/home/you/prof/rt.csv mangohud %command%
```

Richer, if T2 passed:
```
MESA_GPU_TRACES=print_json MESA_GPU_TRACEFILE=/home/you/prof/stages.json mangohud %command%
```

Trim volume with `INTEL_GPU_TRACEPOINT` (additive/subtractive, e.g.
`-blit,+render_pass`) if needed.

Aggregate: group by render target / stage, sum GPU time, sort descending. **This is the
ranked pass-cost table — the plan's headline deliverable.** A throwaway script is fine
here; the durable version is Plan 02.

**Expected observation**: the top few passes account for a large majority of frame time.
Sanity check: **the entries should sum to something close to measured frame time.** If
the sum is wildly off, boundaries are wrong or work is being missed — fix that before
believing the ranking.

**Commit**: `task(T7): pass-level capture + ranked pass-cost table`

---

### T8: Seconds, fine-grain — which draws?

**What to do**: Only now is per-draw affordable, because T6 gave a scenario to reproduce.

```bash
mkfifo /tmp/measure.fifo
```
```
INTEL_MEASURE=draw,control=/tmp/measure.fifo,file=/home/you/prof/draws.csv mangohud %command%
```

Play to the bad scenario, then from another terminal:

```bash
echo 10 > /tmp/measure.fifo    # capture the next 10 frames, then stop
```

Add `interval=N` if still too much. **Do not run this ungated for the session** — see
`pitfalls.md`.

**Expected observation**: within the dominant pass from T7, a identifiable set of draws
(or one draw type) carries the cost. If cost is spread evenly across thousands of draws,
that is itself the finding — it points at per-draw overhead rather than any individual
shader, which is a very different Plan 06.

**Commit**: `task(T8): gated per-draw capture on the bad-frame scenario`

---

### T9: Turn on debug markers so passes have names

**What to do**: By default the T7 table carries DXVK's anonymous objects, not Unreal's
pass names. UE5 emits `ID3DUserDefinedAnnotation` / PIX markers; the translation layers
can forward them as `VK_EXT_debug_utils` labels.

- **DXVK (D3D11, default)**: `DXVK_DEBUG=markers`
- **vkd3d-proton (D3D12, `-dx12`)**: check the **installed version's** `VKD3D_CONFIG`
  docs — the option name has moved between releases; do not trust a blog post.
- Pair with `MESA_GPU_TRACES=markers` so u_trace records them.

Re-run T7 with markers on.

**Expected observation**: pass names read `BasePass`, `ShadowDepths`, `Lumen*`,
`PostProcessing` rather than raw handles. `BasePass is 40% of frame time` is actionable;
`render target 0x7f2a… is 40%` is not.

*Ordering note*: this is worth doing **before** a lot of T7/T8 analysis, not after —
it is ~15 minutes and it makes everything upstream legible. Listed here only because it
is optional to the mechanics.

**Commit**: `task(T9): enable UE5 debug markers; re-run pass-level capture`

---

### T10: One frame, exhaustive — RenderDoc

**What to do**:

```
renderdoccmd capture -w %command%
```

Capture the bad frame with F12, open the `.rdc`. If the launcher re-execs and RenderDoc
loses the process, use the global-hook mode in the GUI.

**Expected observation**: full pipeline state, per-draw timings, resources, and shader
source for the frame T6/T8 identified — enough to name a concrete optimization target for
Plan 06.

**Commit**: `task(T10): RenderDoc capture of the identified bad frame`

---

### T11: Write up findings and restore the machine

**Files**: `context/distilled.md`, `context/pitfalls.md`, `context/plans/SERIES.md`,
`01_profiling_bringup_tracker.md`

**What to do**:
1. Promote verified facts in `distilled.md`; add any new trap to `pitfalls.md`.
2. Record the verdict in SERIES.md — **Plan 06's target depends on it**.
3. Fill in the tracker's Measurements table completely (capture names, N, spread).
4. **Restore the machine** (RULES.md Rule E.4): reset `dev.xe.observation_paranoid` to `1` if changed,
   clear debug env vars from the Steam launch options, remove the FIFO. The next
   session's baseline depends on a known state.

**Commit**: `task(T11): record Plan 01 findings; restore machine state`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `context/distilled.md` | Promote [from docs] → [verified]; add observed facts | P0 |
| `context/pitfalls.md` | Any new trap found while running the funnel | P0 |
| `context/plans/SERIES.md` | Record the bound/throttle verdict — it retargets Plan 06 | P0 |
| `01_profiling_bringup_tracker.md` | Measurements table, per-task status | P0 |
| `scripts/record.sh` | Mode-driven capture wrapper (`survey`/`frames`/`rt`/`draws`) | P1 |
| `scripts/measure-ctl.sh` | FIFO create + poke, for T8 | P2 |

---

## Open Questions / Risks

1. **u_trace may not be in Fedora's build** (T2). *Mitigation*: `INTEL_MEASURE=rt`
   covers the same question less richly; render stages move to Plan 04. Not a blocker.
2. **`gputop` may not be in Fedora's `igt-gpu-tools`** (T3). *Mitigation*: read
   `fdinfo` directly — same data source, no root needed.
3. **Layer injection could fail despite no kernel anti-cheat** (T4). *Mitigation*: none —
   this would invalidate Plan 03's premise. Find out early, which is why T4 is early.
4. **The workload is not reproducible.** Two "identical" sessions render different
   content. *Mitigation*: T6's job is to find a scenario that at least reproduces
   *qualitatively*; genuine determinism waits for Plan 03. **Until then, treat every
   number in this plan as provisional** and do not use them as a patch baseline.
5. **Is any observed effect above noise?** No noise floor exists yet — Plan 03 measures
   it. Consequently this plan should report *rankings and proportions* (which pass
   dominates), not small absolute deltas.
6. **Thermal state drifts across a 2-minute session** and across sessions. *Mitigation*:
   T5 watches clocks; discard any run that throttled rather than caveating it.

---

## Verification Checklist

- [ ] T1: `lspci -k` output recorded; `distilled.md` `xe` entries marked [verified]
- [ ] T2: `/tmp/ut.json` non-empty, **or** u_trace recorded as unavailable and T7 scoped
      to `INTEL_MEASURE=rt`
- [ ] T3: `gputop` reports non-zero engine busy under known load
- [ ] T4: MangoHud overlay renders in Palworld
- [ ] T5: `gputop` JSON covers the full 120 s with no gaps; verdict recorded; N ≥ 3
      sessions with spread in the Measurements table
- [ ] T6: worst frames identified and tied to a describable, repeatable in-game scenario
- [ ] T7: ranked pass-cost table exists **and its entries sum to within ~10% of measured
      frame time**
- [ ] T8: per-draw capture is ≤ 10 frames and did not visibly perturb frame time
- [ ] T9: pass names in the table are Unreal's, not raw handles
- [ ] T10: `.rdc` opens and shows the frame identified in T6
- [ ] T11: SERIES.md carries the verdict; `dev.xe.observation_paranoid` back to `1`; Steam launch
      options clean; no stray FIFO

---

## What comes next

Deliberately **not** in this plan (see `SERIES.md`):

- **Plan 02** — durable trace-aggregation tooling (T7's script, made real and streaming).
- **Plan 03** — GFXReconstruct capture/replay and the **noise floor** every later claim
  is judged against.
- **Plan 04** — local Mesa build (`-Dperfetto=true -Dbuildtype=debugoptimized`, run via
  `VK_DRIVER_FILES`, never installed over system Mesa).
- **Plan 05** — Perfetto + PPS hardware counters, and resolving whether the PPS producer
  speaks `xe` OA.
- **Plan 06+** — the actual ANV work, targeted by what this plan finds.

Reference material for all of these is already compiled in `context/distilled.md`.
