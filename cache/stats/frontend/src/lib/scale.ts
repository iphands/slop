// Pure chart maths: numbers in, SVG path strings out.
//
// No React, no DOM. That is what makes the charts testable without a browser,
// and it is the difference between hand-rolled SVG being a good decision and a
// regrettable one. The components in ../components are dumb wrappers over these.

/** A polyline through `v`, scaled to fit a `w` x `h` viewBox. */
export function linePath(v: number[], w: number, h: number, max?: number): string {
  if (v.length === 0) return '';
  // `1` in the max guards against an all-zero series dividing by zero.
  const hi = Math.max(max ?? 0, ...v, 1);
  const dx = v.length > 1 ? w / (v.length - 1) : 0;
  return v
    .map((y, i) => `${i ? 'L' : 'M'}${(i * dx).toFixed(2)},${(h - (y / hi) * h).toFixed(2)}`)
    .join(' ');
}

/** The same shape, closed to the baseline, for a filled area. */
export function areaPath(v: number[], w: number, h: number, max?: number): string {
  const line = linePath(v, w, h, max);
  return line ? `${line} L${w.toFixed(2)},${h.toFixed(2)} L0,${h.toFixed(2)} Z` : '';
}

export interface StackRect {
  x: number;
  w: number;
  lower: { y: number; h: number };
  upper: { y: number; h: number };
}

/**
 * Two stacked series as rectangles. `a` is the lower band, `b` sits on top.
 *
 * Both share one scale so the bars are comparable across the series, which is
 * the whole point of stacking them.
 */
export function stackRects(a: number[], b: number[], w: number, h: number, gap = 1): StackRect[] {
  if (a.length === 0) return [];
  const hi = Math.max(1, ...a.map((x, i) => x + (b[i] ?? 0)));
  const bw = w / a.length;
  return a.map((av, i) => {
    const bv = b[i] ?? 0;
    const ah = (av / hi) * h;
    const bh = (bv / hi) * h;
    return {
      x: i * bw,
      w: Math.max(0.5, bw - gap),
      lower: { y: h - ah, h: ah },
      upper: { y: h - ah - bh, h: bh },
    };
  });
}

/**
 * Fraction of `w` to fill for a ratio bar. `null` (no data) yields 0 width —
 * the caller renders an em dash for the label rather than "0%".
 */
export function ratioWidth(r: number | null | undefined, w = 100): number {
  if (r === null || r === undefined || !Number.isFinite(r)) return 0;
  return Math.max(0, Math.min(1, r)) * w;
}
