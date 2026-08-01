#!/usr/bin/env python3
"""Rank GPU cost from a Mesa u_trace ``print_json`` capture.

Layer-2 reader: answers "which passes dominate" from a capture taken with
``MESA_GPU_TRACES=print_json,markers``.

Emitted by mesa ``src/util/perf/u_trace.c`` (``print_json_*``). Shape::

    [ {"frame": N, "batches": [ {"events": [ {"event","time_ns","params"} ],
                                 "duration_ns": N} ]} ]

Four properties of this format drive the implementation, all recorded in
``context/pitfalls.md`` because each produces a plausible wrong answer rather than a
visible failure:

1. **The closing ``]`` is written at driver teardown**, so any killed capture is missing
   it. Truncation is expected input, not corruption — we scan for frame objects rather
   than calling ``json.load``.
2. **Events within a batch are not sorted by ``time_ns``** (measured: 33.5% of batches).
   Walking in file order yields negative intervals. Every batch is sorted first.
3. **``time_ns`` is a zero-padded string** while ``frame`` and ``duration_ns`` are
   numbers. Parsed explicitly as int; never compared as a string.
4. **Mesa opens ``file=`` with ``fopen(path,"w")`` and no pid in the name**, so every
   process in a Proton prefix that loads ANV writes to the same path at its own offset.
   The result is a sparse file whose real content may start hundreds of MB in, with NUL
   holes and other processes' fragments around it. We seek to the largest data extent.

Cost is attributed with a **stack**, so a render pass is not charged for the draws inside
it. `self_ns` is what a type spends outside its children and is the column to rank on;
`incl_ns` is the total span including nested work.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path

CHUNK = 8 << 20
FRAME_START = re.compile(rb'\{\s*"frame"\s*:')


def largest_data_extent(fd: int) -> tuple[int, int]:
    """Return (offset, length) of the largest non-hole extent.

    A capture damaged by the multi-writer bug has a tiny extent at 0 (another process's
    empty ``[\\n\\n]``) and the real trace far into the file. Taking the largest extent
    picks the trace; on an undamaged file there is exactly one extent and this is a no-op.
    """
    size = os.lseek(fd, 0, os.SEEK_END)
    best, off = (0, size), 0
    while off < size:
        try:
            start = os.lseek(fd, off, os.SEEK_DATA)
        except OSError:
            break
        try:
            end = os.lseek(fd, start, os.SEEK_HOLE)
        except OSError:
            end = size
        if end - start > best[1] - best[0]:
            best = (start, end)
        off = end
    return best[0], best[1] - best[0]


def stream_frames(path: Path):
    """Yield decoded frame objects, tolerating a damaged head and a truncated tail.

    Scans for ``{"frame":`` boundaries and decodes one object at a time with
    ``json.JSONDecoder.raw_decode``, so peak memory is one frame rather than one capture.
    """
    fd = os.open(path, os.O_RDONLY)
    try:
        start, length = largest_data_extent(fd)
        os.lseek(fd, start, os.SEEK_SET)
        dec = json.JSONDecoder()
        buf = b""
        remaining = length
        while True:
            if remaining > 0:
                block = os.read(fd, min(CHUNK, remaining))
                remaining -= len(block)
            else:
                block = b""
            if not block and not buf:
                return
            buf += block

            pos = 0
            while True:
                m = FRAME_START.search(buf, pos)
                if not m:
                    break
                try:
                    obj, end = dec.raw_decode(buf[m.start():].decode("utf-8", "replace"))
                except ValueError:
                    break  # object straddles the chunk boundary; refill
                yield obj
                pos = m.start() + end
            buf = buf[pos:] if pos else buf
            if not block:
                return
            if len(buf) > 4 * CHUNK:  # a single frame larger than this is not credible
                buf = buf[-CHUNK:]
    finally:
        os.close(fd)


@dataclass
class Bucket:
    self_ns: int = 0
    incl_ns: int = 0
    count: int = 0

    def add(self, self_ns: int, incl_ns: int) -> None:
        self.self_ns += self_ns
        self.incl_ns += incl_ns
        self.count += 1


@dataclass
class Totals:
    by_type: dict[str, Bucket] = field(default_factory=lambda: defaultdict(Bucket))
    by_shader: dict[str, Bucket] = field(default_factory=lambda: defaultdict(Bucket))
    by_target: dict[str, Bucket] = field(default_factory=lambda: defaultdict(Bucket))
    by_blorp: dict[str, Bucket] = field(default_factory=lambda: defaultdict(Bucket))
    frames: int = 0
    batches: int = 0
    events: int = 0
    unmatched: Counter = field(default_factory=Counter)
    span_ns: int = 0
    first_ns: int = 0
    last_ns: int = 0


def label_for(name: str, params: dict) -> tuple[str, str] | None:
    """Map an event to (dimension, key) for the secondary rankings.

    Palworld is a Shipping build and emits no `ID3DUserDefinedAnnotation` markers, so
    there are no UE5 pass names in the trace. Shader hashes and render-target geometry are
    what identify work here instead, and for driver-side optimisation they are arguably
    the better handle anyway.

    **The payload is on the `intel_end_*` event, not the begin** — every
    `intel_begin_*` carries `"params": {}`. Reading the begin's params yields empty
    rankings that look like "no data" rather than a bug.
    """
    if name.startswith("draw"):
        vs, fs = params.get("vs_hash"), params.get("fs_hash")
        if vs or fs:
            return "shader", f"vs={vs or '-'} fs={fs or '-'}"
    if name.startswith("compute"):
        cs = params.get("cs_hash")
        if cs:
            return "shader", f"cs={cs}"
    if name == "render_pass":
        w, h = params.get("width"), params.get("height")
        if w and h:
            return "target", f"{w}x{h} att={params.get('att_count','?')} msaa={params.get('msaa','?')}"
    if name == "blorp":
        op = params.get("op")
        if op:
            w, h = params.get("width", "?"), params.get("height", "?")
            return "blorp", f"{op} {w}x{h}"
    return None


def walk_batch(events: list[dict], t: Totals) -> None:
    """Pair begin/end with a stack, charging each type its exclusive time."""
    # Sort by timestamp; on a tie put `begin` first so a zero-length span still nests.
    def key(e):
        return (int(e["time_ns"]), 0 if e["event"].startswith("intel_begin_") else 1)

    stack: list[tuple[str, int, dict, int]] = []  # name, t_begin, params, children_ns
    for ev in sorted(events, key=key):
        name = ev["event"]
        ts = int(ev["time_ns"])
        t.events += 1
        if t.first_ns == 0 or ts < t.first_ns:
            t.first_ns = ts
        t.last_ns = max(t.last_ns, ts)

        if name.startswith("intel_begin_"):
            stack.append((name[len("intel_begin_"):], ts, ev.get("params") or {}, 0))
            continue
        if not name.startswith("intel_end_"):
            continue
        short = name[len("intel_end_"):]
        # Match the most recent begin of the same name and remove ONLY that entry.
        #
        # A strict stack unwind is wrong here. GPU timestamps repeat — many draws share a
        # single `time_ns` — so events legitimately interleave rather than nest:
        # begin(A) begin(B) end(A) end(B). Discarding everything above the match reported
        # 219k "unmatched" events (14% of the capture) that were nothing of the sort, and
        # charged their time to no one.
        idx = next((i for i in range(len(stack) - 1, -1, -1) if stack[i][0] == short), None)
        if idx is None:
            t.unmatched[short] += 1
            continue

        bname, t_begin, _begin_params, children = stack.pop(idx)
        end_params = ev.get("params") or {}
        incl = ts - t_begin
        if incl < 0:
            t.unmatched[f"{bname}(negative)"] += 1
            continue
        self_ns = max(0, incl - children)
        t.by_type[bname].add(self_ns, incl)
        tagged = label_for(bname, end_params)
        if tagged:
            dim, key_ = tagged
            {"shader": t.by_shader, "target": t.by_target, "blorp": t.by_blorp}[dim][key_].add(
                self_ns, incl
            )
        # Charge this span to the innermost still-open event that encloses it, so a render
        # pass is not billed for the draws inside it. `idx - 1` is that entry: everything
        # above `idx` began after this one did.
        if idx:
            p = stack[idx - 1]
            stack[idx - 1] = (p[0], p[1], p[2], p[3] + incl)


def summarise(path: Path, max_rows: int) -> Totals:
    t = Totals()
    for frame in stream_frames(path):
        t.frames += 1
        for batch in frame.get("batches", ()):
            t.batches += 1
            evs = batch.get("events") or []
            if evs:
                walk_batch(evs, t)
    t.span_ns = t.last_ns - t.first_ns
    return t


def table(title: str, buckets: dict[str, Bucket], span_ns: int, limit: int, width: int = 46) -> None:
    if not buckets:
        return
    rows = sorted(buckets.items(), key=lambda kv: -kv[1].self_ns)[:limit]
    total_self = sum(b.self_ns for b in buckets.values())
    print(f"\n{title}   ({len(buckets)} distinct, {total_self / 1e6:.1f} ms self total)")
    print(f"  {'key':<{width}} {'self ms':>10} {'%self':>7} {'incl ms':>10} {'count':>9} {'us/ev':>8}")
    print(f"  {'-' * width} {'-' * 10} {'-' * 7} {'-' * 10} {'-' * 9} {'-' * 8}")
    for key, b in rows:
        pct = 100 * b.self_ns / total_self if total_self else 0
        per = b.self_ns / b.count / 1000 if b.count else 0
        print(
            f"  {key[:width]:<{width}} {b.self_ns / 1e6:>10.1f} {pct:>6.1f}% "
            f"{b.incl_ns / 1e6:>10.1f} {b.count:>9,} {per:>8.1f}"
        )


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("json", type=Path, help="u_trace print_json capture")
    ap.add_argument("--top", type=int, default=15, help="rows per table (default 15)")
    args = ap.parse_args()

    t = summarise(args.json, args.top)
    if not t.frames:
        print(f"{args.json}: no frame objects found", file=sys.stderr)
        return 1

    gpu_ns = sum(b.self_ns for b in t.by_type.values())
    print(f"frames        : {t.frames:,}")
    print(f"batches       : {t.batches:,}")
    print(f"events        : {t.events:,}")
    print(f"trace span    : {t.span_ns / 1e9:.2f} s")
    print(f"GPU self time : {gpu_ns / 1e6:.1f} ms = {100 * gpu_ns / t.span_ns:.1f}% of span")
    if t.frames:
        print(f"per frame     : {gpu_ns / t.frames / 1e6:.2f} ms GPU over {t.frames:,} frames")
    if t.unmatched:
        print(f"unmatched     : {sum(t.unmatched.values()):,} events "
              f"({', '.join(f'{k}:{v}' for k, v in t.unmatched.most_common(4))})")
        print("                (expected at a truncated boundary; large counts mean the "
              "capture is damaged)")

    table("BY EVENT TYPE", t.by_type, t.span_ns, args.top, 30)
    table("BY SHADER PAIR (draws)", t.by_shader, t.span_ns, args.top, 46)
    table("BY RENDER TARGET", t.by_target, t.span_ns, args.top, 46)
    table("BY BLORP OP (blit/clear/resolve)", t.by_blorp, t.span_ns, args.top, 46)
    return 0


if __name__ == "__main__":
    sys.exit(main())
