# bench: M2 Air thermal/throttle baseline

Before/after harness for cooling mods on the fanless MacBook Air M2 (Asahi Linux). No root needed.

```
./bench.sh --label stock            # full run, ~15 min (180 s long phases + cooldown gates)
./bench.sh --label smoke --quick    # ~2 min sanity run, short phases
# ...do the mod, same room/stand/AC...
./bench.sh --label mod1
./compare.py stock mod1 --svg stock-vs-mod1.svg      # args: results dir or substring of one
./compare.py stock                                   # single-run summary
```

Flags: `--duration S` (long phases, default 180), `--no-cooldown`, `--cool-delta C` (default 2),
`--cool-max S` (default 600), `--force` (skip the idle-load check).

## What it measures
| Phase | Load | Purpose |
|---|---|---|
| `st` | 1 thread on a P core, ~20 s | control: should not move with a cooling mod |
| `mt-short` | 8 threads, 30 s from cool | burst score (the "short bench") |
| `p4-long` | 4 threads pinned to P cores (cpu4-7), 180 s | concentrates heat; cleanest throttle curve |
| `mt-long` | 8 threads, 180 s | all-core sustained |
| `7z` | `7z b 1 -mmt=8` | real-world, run hot after the long phases |
| `mem` | `sysbench memory`, 10 s | control: memory bandwidth should not move |

A 1 Hz sampler logs the whole run to `samples.csv`: E/P cluster freq, chassis temps, and the SMC power
rails (total, AC, "Heatpipe Power" = SoC package power; it sags when the SoC is power-capped).

## There is no CPU die temperature
Asahi exposes only NAND, battery hotspot, charge regulator, WiFi/BT module and NVMe temps. They are
chassis-side proxies: good for "did the mod move heat into the case", not for throttle onset. Throttle is
detected from **fixed-work throughput** (`sysbench --report-interval=1`, events/s each second), with P
freq and heatpipe watts as corroboration. In practice WiFi/BT and NAND move the most; battery hotspot
barely moves in short runs.

## Reading compare.py
- `peak` = mean of first 20 s, `sustained` = mean of last 60 s, `drop` = the gap between them.
- `time to <95% of peak` = first second the 10 s rolling mean stays below 95% of peak for 10 s. After a
  successful mod this should be later or `never`. Thresholds: `--thr --hold --win --peak-s --sus-s`.
- Warnings list conditions that differ between runs (AC, charging, governor, max freq, start temps >3 C).
  Treat the numbers as suspect when any show up.

## meta.txt
Per-run record of conditions: label, date, kernel, model, bench flags, governor, max freqs, AC/battery
state, loadavg, start temps, tool versions. Only used for the comparability warnings.

## Hygiene checklist (the dominant source of noise)
- AC plugged in, battery **not charging** (charging heats the chassis). Same battery state both runs.
- Same room/ambient, same surface or stand, lid open the same amount, screen dim, nothing else running.
- Ambient isn't measurable here: put it in the label (`--label stock-22C`).
- Run the baseline **twice** and compare the two to learn the noise floor before trusting small deltas.
- `results/<stamp>-<label>/` holds raw logs; keep the baseline dir, it is the "before".
