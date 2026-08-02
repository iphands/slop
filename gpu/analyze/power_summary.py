#!/usr/bin/env python3
"""Summarise ``gpu-survey.sh`` CSVs for the power-limit question (Plan 07).

Answers one question across N runs: **what does package power plateau at under sustained
load, and which throttle reason fires there?**

Two things make this less trivial than a mean:

1. **The PL1 time window is 28 s** (``power2_max_interval``). Power derived from
   ``Δenergy2_input / Δt`` is an average over the sample interval, so the first tens of
   seconds of a run are a ramp, not a plateau. Samples before ``--skip-s`` are discarded.
2. **A single run is not a result** (``context/plans/RULES.md`` Rule D.1). This reads N
   CSVs and reports the across-run spread, so a delta can be called "within noise" when it
   is.

Input columns come from ``scripts/gpu-survey.sh`` (see its header comment): ``t_s``,
``power_w``, ``act_freq_mhz``, ``cur_freq_mhz``, ``throttle_reasons``, ``temp_c``.
Comment lines beginning ``#`` carry the run's provenance and are echoed, not parsed.

**On the "RUN INVALID per Rule D.5" line** that ``gpu-survey.sh`` writes into throttled
captures: that gate exists so a throttled run cannot be used for a *performance*
comparison. Plan 07 is measuring the throttle itself, so for this analysis a throttled run
is the signal, not a defect. This tool says so rather than letting a future reader discard
the very data that answers the question.
"""

from __future__ import annotations

import argparse
import csv
import statistics
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

# Samples before this many seconds are treated as ramp-up, not plateau. Default is one
# PL1 time constant (28 s, from power2_max_interval) rounded up.
DEFAULT_SKIP_S = 30.0

# The sampler is stopped after the load process returns, so it outlives the load by up to
# one sample interval plus teardown. Those trailing samples are idle-power and would drag
# the plateau mean down. This is a declared, uniform trim — not a search for a better
# number: it is applied identically to every run and stated in the output.
DEFAULT_TRIM_TAIL_S = 2.0

# PL1 is stored in PWR_LIM_VAL as a U12.3 fixed-point value — units of 1/8 W
# (scl_shift_power = 3, read from PKG_POWER_SKU_UNIT; see scripts/power-regs.sh). The
# hardware therefore cannot express a limit finer than this, so a plateau-to-limit gap
# below one quantum is unresolvable no matter how stable the measurement is.
#
# This matters: the vkmark plateau is stable to sd 0.03 W, which would otherwise make a
# physically meaningless 0.05 W gap read as statistically significant. The noise floor has
# to be bounded below by what the register can actually represent.
PL1_QUANTUM_W = 0.125

# A plateau needs enough samples for its spread to mean anything.
MIN_PLATEAU_SAMPLES = 20


@dataclass(frozen=True)
class Sample:
    """One row of a gpu-survey CSV, with the fields this analysis needs."""

    t_s: float
    power_w: float | None
    act_freq_mhz: float | None
    cur_freq_mhz: float | None
    throttle_reasons: str
    temp_c: float | None


@dataclass(frozen=True)
class RunSummary:
    """Plateau statistics for one capture file."""

    path: Path
    header: list[str]
    n_total: int
    n_plateau: int
    power_mean: float
    power_min: float
    power_max: float
    power_stdev: float
    act_freq_mean: float
    cur_freq_mean: float
    temp_max: float | None
    reasons: Counter[str]

    @property
    def throttled_fraction(self) -> float:
        """Fraction of plateau samples reporting any throttle reason."""
        non_none = sum(n for r, n in self.reasons.items() if r != "none")
        return non_none / self.n_plateau if self.n_plateau else 0.0


def _maybe_float(raw: str) -> float | None:
    """Parse a survey field, treating the script's empty/``NA`` placeholders as missing.

    ``gpu-survey.sh`` emits an empty ``power_w`` on its first sample (no previous energy
    reading to difference against) and ``NA`` where a sysfs file is absent.
    """
    if raw in ("", "NA"):
        return None
    try:
        return float(raw)
    except ValueError:
        return None


def read_samples(path: Path) -> tuple[list[str], list[Sample]]:
    """Stream one survey CSV into header comments and parsed samples.

    Reads line-by-line rather than slurping: survey CSVs of a full session reach hundreds
    of thousands of rows (CLAUDE.md, "streaming over slurping").
    """
    header: list[str] = []
    samples: list[Sample] = []

    with path.open(encoding="utf-8") as fh:
        def data_lines():
            for line in fh:
                if line.startswith("#"):
                    header.append(line.rstrip("\n"))
                    continue
                yield line

        for row in csv.DictReader(data_lines()):
            t_raw = _maybe_float(row.get("t_s", ""))
            if t_raw is None:
                continue
            samples.append(
                Sample(
                    t_s=t_raw,
                    power_w=_maybe_float(row.get("power_w", "")),
                    act_freq_mhz=_maybe_float(row.get("act_freq_mhz", "")),
                    cur_freq_mhz=_maybe_float(row.get("cur_freq_mhz", "")),
                    throttle_reasons=row.get("throttle_reasons", "").strip(),
                    temp_c=_maybe_float(row.get("temp_c", "")),
                )
            )

    return header, samples


def summarise(path: Path, skip_s: float, trim_tail_s: float) -> RunSummary | None:
    """Reduce one capture to its plateau statistics, or None if there is no plateau."""
    header, samples = read_samples(path)
    if not samples:
        print(f"{path}: no data rows", file=sys.stderr)
        return None

    end_s = samples[-1].t_s - trim_tail_s
    plateau = [
        s for s in samples if skip_s <= s.t_s <= end_s and s.power_w is not None
    ]
    if len(plateau) < MIN_PLATEAU_SAMPLES:
        print(
            f"{path}: only {len(plateau)} sample(s) in [{skip_s:g}s, {end_s:.1f}s] "
            f"(need {MIN_PLATEAU_SAMPLES}) — run too short to have a plateau",
            file=sys.stderr,
        )
        return None

    powers = [s.power_w for s in plateau if s.power_w is not None]
    acts = [s.act_freq_mhz for s in plateau if s.act_freq_mhz is not None]
    curs = [s.cur_freq_mhz for s in plateau if s.cur_freq_mhz is not None]
    temps = [s.temp_c for s in plateau if s.temp_c is not None]

    return RunSummary(
        path=path,
        header=header,
        n_total=len(samples),
        n_plateau=len(plateau),
        power_mean=statistics.mean(powers),
        power_min=min(powers),
        power_max=max(powers),
        power_stdev=statistics.stdev(powers) if len(powers) > 1 else 0.0,
        act_freq_mean=statistics.mean(acts) if acts else 0.0,
        cur_freq_mean=statistics.mean(curs) if curs else 0.0,
        temp_max=max(temps) if temps else None,
        reasons=Counter(s.throttle_reasons for s in plateau),
    )


def print_run(run: RunSummary) -> None:
    """Print one run's plateau statistics."""
    print(f"── {run.path.name}")
    for line in run.header:
        print(f"   {line}")
    print(
        f"   plateau: {run.n_plateau}/{run.n_total} samples   "
        f"power {run.power_mean:.2f} W  (min {run.power_min:.2f}, max {run.power_max:.2f}, "
        f"sd {run.power_stdev:.2f})"
    )
    print(
        f"   act_freq {run.act_freq_mean:.0f} MHz   cur_freq {run.cur_freq_mean:.0f} MHz"
        + (f"   temp_max {run.temp_max:.0f} °C" if run.temp_max is not None else "")
    )
    ordered = sorted(run.reasons.items(), key=lambda kv: -kv[1])
    reasons = "  ".join(f"{r or '(blank)'}={n}" for r, n in ordered)
    print(f"   throttle: {reasons}   ({run.throttled_fraction * 100:.0f}% of plateau)")
    if any("RUN INVALID" in line for line in run.header):
        print(
            "   NOTE: gpu-survey.sh stamped this capture RUN INVALID (Rule D.5, throttled)."
            "\n         That gate is for performance comparisons. Plan 07 measures the"
            "\n         throttle itself, so here the throttle is the result — not a defect."
        )


def print_aggregate(runs: list[RunSummary], limit_w: float | None) -> None:
    """Print the across-run verdict — the part Rule D actually requires."""
    means = [r.power_mean for r in runs]
    across_mean = statistics.mean(means)
    across_spread = max(means) - min(means)

    # The noise floor a delta must clear. Run-to-run spread alone understates it — with a
    # single run it is 0 by construction, which would make any gap look significant. The
    # within-run standard deviation bounds it from below, and the PL1 register quantum
    # bounds it below that: the hardware cannot express a finer limit than 1/8 W.
    within_sd = statistics.mean([r.power_stdev for r in runs])
    noise = max(across_spread, within_sd, PL1_QUANTUM_W)

    print()
    print(f"══ Across {len(runs)} run(s)")
    print(
        f"   plateau power: mean {across_mean:.2f} W   "
        f"spread {across_spread:.2f} W   ({min(means):.2f} … {max(means):.2f})"
    )
    print(
        f"   noise floor: {noise:.3f} W   (max of run-to-run spread {across_spread:.3f}, "
        f"mean within-run sd {within_sd:.3f}, PL1 register quantum {PL1_QUANTUM_W:.3f})"
    )

    if len(runs) < 3:
        print("   WARNING: fewer than 3 runs. RULES.md D.1 — one run is an anecdote.")

    all_reasons: Counter[str] = Counter()
    for run in runs:
        all_reasons.update(run.reasons)
    dominant = [r for r, _ in all_reasons.most_common() if r != "none"]
    if dominant:
        print(f"   throttle reasons seen: {', '.join(dominant)}")
    else:
        print("   throttle reasons seen: none — the GPU was never limited")

    if limit_w is not None:
        gap = limit_w - across_mean
        print(f"   configured PL1: {limit_w:.2f} W   gap to plateau: {gap:+.2f} W")
        if abs(gap) <= noise:
            print(
                "   VERDICT: the plateau sits at the PL1 value — the gap is within noise. "
                "Consistent with being power-limited at PL1."
            )
        elif gap > noise:
            print(
                "   VERDICT: the plateau is BELOW PL1 by more than noise. PL1 is not the "
                "binding constraint — check act_freq vs rp0 and the throttle reasons above."
            )
        else:
            print(
                "   VERDICT: the plateau is ABOVE PL1 by more than noise — either the limit "
                "is not enforced, or the averaging window is shorter than tau."
            )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Summarise gpu-survey.sh CSVs for the Plan 07 power-limit question."
    )
    parser.add_argument("csv", nargs="+", type=Path, help="gpu-survey.sh capture(s)")
    parser.add_argument(
        "--skip-s",
        type=float,
        default=DEFAULT_SKIP_S,
        help=f"discard samples before this time as ramp-up (default {DEFAULT_SKIP_S:g}, "
        "one PL1 time constant)",
    )
    parser.add_argument(
        "--trim-tail-s",
        type=float,
        default=DEFAULT_TRIM_TAIL_S,
        help=f"discard this many seconds at the end, where the sampler outlived the load "
        f"(default {DEFAULT_TRIM_TAIL_S:g})",
    )
    parser.add_argument(
        "--limit-w",
        type=float,
        default=None,
        help="configured PL1 in watts, to judge the plateau against",
    )
    args = parser.parse_args()

    runs = [
        r
        for r in (summarise(p, args.skip_s, args.trim_tail_s) for p in args.csv)
        if r is not None
    ]
    if not runs:
        print("no usable captures", file=sys.stderr)
        return 1

    for run in runs:
        print_run(run)
        print()

    print_aggregate(runs, args.limit_w)
    return 0


if __name__ == "__main__":
    sys.exit(main())
