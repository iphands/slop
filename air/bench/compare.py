#!/usr/bin/env -S python3 -I
"""Summarize one bench.sh run, or compare two (before after).

  ./compare.py results/<run>                 single-run summary
  ./compare.py results/<before> results/<after> [--svg out.svg]
Args may be a results dir, or a substring of one under results/ (e.g. "stock").

Throttle onset (long phases): first second where the rolling-mean throughput stays below
THR x peak for HOLD s. peak = mean of the first PEAK_S s, sustained = mean of the last SUS_S s.
"""
import argparse, csv, glob, os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
LONG = ("p4-long", "mt-long")
SCORED = ("st", "mt-short", "p4-long", "mt-long")
TEMPS = (("t_nand_mc", "nand"), ("t_chg_mc", "chg-reg"), ("t_wifi_mc", "wifi"), ("t_bat_mc", "bat-hotspot"), ("t_nvme_mc", "nvme"))


def find(arg):
    if os.path.isdir(arg):
        return arg
    hits = sorted(glob.glob(os.path.join(HERE, "results", f"*{arg}*")))
    if not hits:
        sys.exit(f"no run matches {arg!r}")
    if len(hits) > 1:
        print(f"note: {arg!r} matches {len(hits)} runs, using newest: {os.path.basename(hits[-1])}", file=sys.stderr)
    return hits[-1]


def load(d):
    r = {"dir": d, "name": os.path.basename(d.rstrip("/")), "meta": {}, "phases": {}, "samples": {}, "series": {}, "extra": {}}
    for line in open(os.path.join(d, "meta.txt")):
        k, _, v = line.rstrip("\n").partition("=")
        r["meta"][k] = v
    for row in csv.DictReader(open(os.path.join(d, "phases.csv"))):
        r["phases"][row["phase"]] = row
    for row in csv.DictReader(open(os.path.join(d, "samples.csv"))):
        if any(v == "" for v in row.values()):
            continue
        r["samples"].setdefault(row["phase"], []).append({k: (int(v) if k != "phase" else v) for k, v in row.items()})
    for ph in r["phases"]:
        p = os.path.join(d, ph + ".log")
        txt = open(p).read() if os.path.exists(p) else ""
        pat = r"\[\s*\d+s \].*?eps: ([\d.]+)" if ph != "mem" else r"\[\s*\d+s \]\s+([\d.]+) MiB/sec"
        r["series"][ph] = [float(x) for x in re.findall(pat, txt)]
        m = re.search(r"events per second:\s+([\d.]+)", txt)
        if m:
            r["extra"][ph] = float(m.group(1))
        if ph == "7z":
            t = re.findall(r"^Tot:\s+\d+\s+(\d+)\s+(\d+)", txt, re.M)
            if t:
                r["extra"]["7z_ru"], r["extra"]["7z_rating"] = float(t[-1][0]), float(t[-1][1])
    return r


def mean(x):
    return sum(x) / len(x) if x else float("nan")


def onset(series, peak, thr, hold, win):
    """first t (s, 1-based end of window) where the rolling mean is < thr*peak for hold consecutive s; None if never."""
    run = 0
    for i in range(win - 1, len(series)):
        if mean(series[i - win + 1:i + 1]) < thr * peak:
            run += 1
            if run >= hold:
                return i + 1 - hold + 1
        else:
            run = 0
    return None


def metrics(r, a):
    """-> ordered list of (key, label, value, fmt, kind) ; kind: 'pct' (relative delta), 'abs', 'time' (higher is better)"""
    out = []
    pk = a.peak_s
    maxp = int(r["meta"].get("max_khz_P", 0)) or 1
    for ph in SCORED:
        s = r["series"].get(ph)
        if not s:
            continue
        out.append((f"{ph}.mean", f"{ph}: mean events/s", mean(s), "{:,.0f}", "pct"))
        if ph in LONG:
            n = len(s)
            peak = mean(s[:min(pk, max(3, n // 4))])
            sus = mean(s[-min(a.sus_s, max(3, n // 3)):])
            out.append((f"{ph}.peak", f"{ph}: peak (first {pk}s)", peak, "{:,.0f}", "pct"))
            out.append((f"{ph}.sus", f"{ph}: sustained (last {a.sus_s}s)", sus, "{:,.0f}", "pct"))
            out.append((f"{ph}.drop", f"{ph}: drop peak->sustained", 100 * (1 - sus / peak), "{:.1f}%", "abs_lower"))
            for thr in sorted({a.thr, 0.9, 0.8}, reverse=True):
                nm = f"{int(round(thr*100))}%"
                o = onset(s, peak, thr, a.hold, a.win)
                out.append((f"{ph}.t{nm}", f"{ph}: time to <{nm} of peak", o if o is not None else float("inf"), "{}", "time"))
    for ph in LONG:
        smp = r["samples"].get(ph)
        if not smp:
            continue
        n = len(smp)
        e, l = smp[:min(pk, max(3, n // 4))], smp[-min(a.sus_s, max(3, n // 3)):]
        out.append((f"{ph}.fp_e", f"{ph}: P-cluster MHz early", mean([x["f_p_khz"] for x in e]) / 1000, "{:,.0f}", "pct"))
        out.append((f"{ph}.fp_l", f"{ph}: P-cluster MHz late", mean([x["f_p_khz"] for x in l]) / 1000, "{:,.0f}", "pct"))
        out.append((f"{ph}.fmax", f"{ph}: % time at max P freq", 100 * mean([x["f_p_khz"] >= maxp for x in smp]), "{:.0f}%", "pct"))
        out.append((f"{ph}.pw_e", f"{ph}: heatpipe W early", mean([x["p_heat_uw"] for x in e]) / 1e6, "{:.1f}", "pct"))
        out.append((f"{ph}.pw_l", f"{ph}: heatpipe W late", mean([x["p_heat_uw"] for x in l]) / 1e6, "{:.1f}", "pct"))
        for col, nm in TEMPS:
            out.append((f"{ph}.{col}", f"{ph}: {nm} C end (+rise)", (smp[-1][col] / 1000, (smp[-1][col] - smp[0][col]) / 1000), "{0[0]:.1f} (+{0[1]:.1f})", "temp"))
    if "7z_rating" in r["extra"]:
        out.append(("7z.rating", "7z: total rating MIPS (hot)", r["extra"]["7z_rating"], "{:,.0f}", "pct"))
    if r["series"].get("mem"):
        out.append(("mem.mean", "mem: write MiB/s", mean(r["series"]["mem"]), "{:,.0f}", "pct"))
    return out


def warnings(ra, rb):
    w = []
    ma, mb = ra["meta"], rb["meta"]
    for k, why in (("ac_online", "AC state differs"), ("battery_status", "battery charge state differs (charging adds heat)"),
                   ("governor", "governor differs"), ("max_khz_P", "P max freq differs"), ("max_khz_E", "E max freq differs"),
                   ("kernel", "kernel differs"), ("flags", "bench flags differ")):
        if ma.get(k) != mb.get(k):
            w.append(f"{why}: {ma.get(k)} vs {mb.get(k)}")
    for k in ("nand", "chg", "wifi", "nvme"):
        va, vb = int(ma.get(f"start_t_{k}_mc", 0)), int(mb.get(f"start_t_{k}_mc", 0))
        if abs(va - vb) > 3000:
            w.append(f"start temp {k} differs by {abs(va-vb)/1000:.1f} C ({va/1000:.1f} vs {vb/1000:.1f}); a warmer start throttles earlier")
    for r in (ra, rb):
        bad = [p for p, row in r["phases"].items() if row["cooled_ok"] == "0"]
        if bad:
            w.append(f"{r['name']}: cooldown timed out before {', '.join(bad)} (started warm)")
    return w


def fmt(v, f):
    if v == float("inf"):
        return "never"
    return f.format(v)


def table(rows, head):
    wid = [max(len(str(r[i])) for r in rows + [head]) for i in range(len(head))]
    line = lambda r: "| " + " | ".join(str(c).ljust(wid[i]) if i == 0 else str(c).rjust(wid[i]) for i, c in enumerate(r)) + " |"
    return "\n".join([line(head), "|" + "|".join("-" * (w + 2) for w in wid) + "|"] + [line(r) for r in rows])


def delta(va, vb, kind):
    if kind == "temp":
        return f"{vb[0]-va[0]:+.1f} C"
    if va == float("inf") and vb == float("inf"):
        return ""
    if va == float("inf") or vb == float("inf"):
        return "never->%s" % fmt(vb, "{}s") if va == float("inf") else "%s->never" % fmt(va, "{}s")
    if kind == "pct":
        return f"{100*(vb-va)/va:+.1f}%" if va else ""
    if kind == "time":
        return f"{vb-va:+d}s"
    return f"{vb-va:+.1f} pt"


def svg(path, runs):
    cols = ("#1f77b4", "#d95f02")
    panels = [("events/s", lambda r, ph: r["series"].get(ph, []), 1),
              ("P-cluster MHz", lambda r, ph: [x["f_p_khz"] / 1000 for x in r["samples"].get(ph, [])], 1),
              ("heatpipe W", lambda r, ph: [x["p_heat_uw"] / 1e6 for x in r["samples"].get(ph, [])], 1),
              ("wifi/bt module C", lambda r, ph: [x["t_wifi_mc"] / 1000 for x in r["samples"].get(ph, [])], 1)]
    phs = [p for p in LONG if any(r["series"].get(p) for r in runs)]
    W, H, ml, mt, pw, ph_ = 330, 150, 60, 28, 270, 100
    tw, th = len(phs) * W + 20, len(panels) * H + 50
    o = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{tw}" height="{th}" font-family="sans-serif" font-size="11">',
         f'<rect width="{tw}" height="{th}" fill="#fff"/>']
    for k, r in enumerate(runs):
        o.append(f'<line x1="{20+k*230}" y1="12" x2="{44+k*230}" y2="12" stroke="{cols[k]}" stroke-width="2"/><text x="{50+k*230}" y="16" fill="#222">{r["name"][:30]}</text>')
    for i, (title, get, _) in enumerate(panels):
        for j, p in enumerate(phs):
            ys = [get(r, p) for r in runs]
            allv = [v for y in ys for v in y]
            if not allv:
                continue
            lo, hi = min(allv), max(allv)
            pad = (hi - lo) * 0.08 or 1
            lo, hi = lo - pad, hi + pad
            n = max(len(y) for y in ys)
            x0, y0 = j * W + ml, 34 + i * H + mt
            o.append(f'<text x="{x0}" y="{y0-10}" fill="#222" font-weight="bold">{p}: {title}</text>')
            o.append(f'<rect x="{x0}" y="{y0}" width="{pw}" height="{ph_}" fill="none" stroke="#bbb"/>')
            for frac in (0, 0.5, 1):
                v = lo + (hi - lo) * frac
                yy = y0 + ph_ - ph_ * frac
                o.append(f'<text x="{x0-4}" y="{yy+4}" text-anchor="end" fill="#555">{v:,.0f}</text>' if hi - lo > 20 else f'<text x="{x0-4}" y="{yy+4}" text-anchor="end" fill="#555">{v:.1f}</text>')
            o.append(f'<text x="{x0+pw}" y="{y0+ph_+12}" text-anchor="end" fill="#555">{n}s</text>')
            for k, y in enumerate(ys):
                if len(y) < 2:
                    continue
                pts = " ".join(f"{x0+pw*t/(n-1):.1f},{y0+ph_-ph_*(v-lo)/(hi-lo):.1f}" for t, v in enumerate(y))
                o.append(f'<polyline points="{pts}" fill="none" stroke="{cols[k]}" stroke-width="1.5"/>')
    o.append("</svg>")
    open(path, "w").write("\n".join(o))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("runs", nargs="+")
    ap.add_argument("--svg")
    ap.add_argument("--thr", type=float, default=0.95, help="onset threshold fraction of peak (default 0.95)")
    ap.add_argument("--hold", type=int, default=10, help="seconds the rolling mean must stay below thr (default 10)")
    ap.add_argument("--win", type=int, default=10, help="rolling-mean window, s (default 10)")
    ap.add_argument("--peak-s", type=int, default=20, dest="peak_s")
    ap.add_argument("--sus-s", type=int, default=60, dest="sus_s")
    a = ap.parse_args()
    if len(a.runs) > 2:
        ap.error("1 or 2 runs")
    runs = [load(find(x)) for x in a.runs]
    ms = [metrics(r, a) for r in runs]
    if len(runs) == 1:
        print(f"# {runs[0]['name']}\n")
        print(table([[lab, f.format(v) if kind == "temp" else fmt(v, f)] for _, lab, v, f, kind in ms[0]], ["metric", "value"]))
    else:
        da, db = ({k: (lab, v, f, kind) for k, lab, v, f, kind in m} for m in ms)
        print(f"# before: {runs[0]['name']}\n# after:  {runs[1]['name']}\n")
        w = warnings(*runs)
        if w:
            print("WARNINGS (comparison may not be apples-to-apples):\n" + "\n".join(f"  - {x}" for x in w) + "\n")
        rows = []
        for k, (lab, va, f, kind) in da.items():
            if k not in db:
                continue
            vb = db[k][1]
            s = lambda v: f.format(v) if kind == "temp" else fmt(v, f)
            rows.append([lab, s(va), s(vb), delta(va, vb, kind)])
        print(table(rows, ["metric", "before", "after", "delta"]))
        print("\nreading it: higher events/s, MHz, W-late and time-to-<x% are better; lower drop% and temp rise are better.")
        print("`st` and `mem` are controls: they should not move with a cooling mod (if they do, that's noise/conditions).")
    if a.svg:
        svg(a.svg, runs)
        print(f"\nwrote {a.svg}")


main()
