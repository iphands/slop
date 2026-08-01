#!/usr/bin/env python3
"""Summarise an INTEL_MEASURE frame-granularity CSV.

Exploratory Layer-1 reader: answers "which frames are bad" from a capture taken with
``INTEL_MEASURE=frame,file=...``. The Rust tool in Plan 02 replaces this; this exists so
the numbers in a commit message can be re-derived instead of retyped.

Format is emitted by mesa ``src/intel/common/intel_measure.c``
(``print_combined_results`` / the header in ``intel_measure_print``). With the FRAME flag
each row combines the events of one frame:

    draw_start,draw_end,frame,batch,batch_size,renderpass,event_index,event_count,
    type,count,vs,tcs,tes,gs,fs,cs,ms,ts,idle_us,time_us

``draw_start``/``draw_end`` are raw GPU timestamp ticks; ``idle_us``/``time_us`` are
already converted by Mesa. Streams the file — captures reach gigabytes when the
granularity flag is wrong (see context/pitfalls.md).
"""

from __future__ import annotations

import argparse
import statistics
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

# A frame doing less than this much GPU work is a menu, a loading screen or a
# compositor-only frame. Gameplay analysis has to exclude them or the percentiles
# describe the main menu.
GAMEPLAY_MIN_GPU_US = 10_000.0

N_COLUMNS = 20


@dataclass(frozen=True)
class Frame:
    """One combined row: all measured events of a single frame."""

    start_ticks: int
    frame: int
    events: int
    first_type: str
    idle_us: float
    gpu_us: float


def parse(path: Path):
    """Yield Frame rows, skipping the header and any interleaving damage.

    Mesa opens the ``file=`` path with ``fopen(name, "w")`` and no pid in the name, so
    every process in a Proton prefix that loads ANV writes to the same file at its own
    offset. That leaves NUL padding and truncated lines mid-file; they are skipped rather
    than treated as a parse failure. See context/pitfalls.md.
    """
    with path.open(encoding="utf-8", errors="replace") as handle:
        for line in handle:
            line = line.strip("\n").strip("\x00").strip()
            if not line or line.startswith("draw_start"):
                continue
            cols = line.split(",")
            if len(cols) < N_COLUMNS:
                continue
            try:
                yield Frame(
                    start_ticks=int(cols[0]),
                    frame=int(cols[2]),
                    events=int(cols[7]),
                    first_type=cols[8],
                    idle_us=float(cols[18]),
                    gpu_us=float(cols[19]),
                )
            except ValueError:
                continue  # truncated or overwritten row


def ticks_per_us(frames: list[Frame]) -> float:
    """Derive the GPU timestamp rate from the capture itself.

    Consecutive ``draw_start`` deltas measure the same interval that
    ``idle_us + time_us`` reports in microseconds, so their ratio is the tick rate. Taking
    it from the data avoids hard-coding a per-platform constant that would be wrong the
    first time this runs on another chip.
    """
    ratios = []
    for prev, cur in zip(frames, frames[1:]):
        span = cur.start_ticks - prev.start_ticks
        reported = prev.idle_us + prev.gpu_us
        if reported > 0 and 0 < span < 10**7:
            ratios.append(span / reported)
    if not ratios:
        raise SystemExit("cannot derive timestamp rate: no usable consecutive rows")
    return statistics.median(ratios)


def percentile(values: list[float], q: float) -> float:
    return values[min(int(len(values) * q), len(values) - 1)]


def report(frames: list[Frame], tick: float, min_gpu_us: float) -> None:
    span_us = (frames[-1].start_ticks - frames[0].start_ticks) / tick
    measured_us = sum(f.gpu_us for f in frames)

    print(f"rows           : {len(frames)}  (frames {frames[0].frame}..{frames[-1].frame})")
    print(f"timestamp rate : {tick:.3f} ticks/us")
    print(f"wall span      : {span_us / 1e6:.2f} s")
    print(
        f"attributed     : {measured_us / 1e6:.2f} s "
        f"= {100 * measured_us / span_us:.1f}% of wall"
    )
    print(
        "                 (compare against gpu-survey busy%; a large shortfall means "
        "INTEL_MEASURE is not seeing all the work)"
    )

    play = [f for f in frames if f.gpu_us > min_gpu_us]
    if not play:
        print(f"\nno frames above {min_gpu_us / 1000:.0f} ms GPU — capture is all menu/idle")
        return

    gpu_ms = sorted(f.gpu_us / 1000 for f in play)
    print(f"\ngameplay frames (> {min_gpu_us / 1000:.0f} ms GPU): {len(play)}")
    for q in (0.50, 0.90, 0.99):
        ms = percentile(gpu_ms, q)
        print(f"  p{q * 100:<4g} {ms:7.2f} ms GPU   ({1000 / ms:6.1f} fps if GPU-bound)")
    print(f"  max   {gpu_ms[-1]:7.2f} ms GPU")

    print("\n  first-event type :", Counter(f.first_type for f in play).most_common(5))
    print("  events per frame :", Counter(f.events for f in play).most_common(6))

    worst = sorted(play, key=lambda f: -f.gpu_us)[: max(1, len(play) // 100)]
    mean_gpu = sum(f.gpu_us for f in worst) / len(worst) / 1000
    mean_events = sum(f.events for f in worst) / len(worst)
    print(f"\n  worst {len(worst)} frames: mean {mean_gpu:.1f} ms GPU, {mean_events:.1f} events")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("csv", type=Path, help="INTEL_MEASURE frame-granularity CSV")
    ap.add_argument(
        "--min-gpu-ms",
        type=float,
        default=GAMEPLAY_MIN_GPU_US / 1000,
        help="frames below this much GPU time are treated as menu/loading (default: 10)",
    )
    args = ap.parse_args()

    frames = list(parse(args.csv))
    if len(frames) < 2:
        print(f"{args.csv}: fewer than 2 usable rows — nothing to summarise", file=sys.stderr)
        return 1

    report(frames, ticks_per_us(frames), args.min_gpu_ms * 1000)
    return 0


if __name__ == "__main__":
    sys.exit(main())
