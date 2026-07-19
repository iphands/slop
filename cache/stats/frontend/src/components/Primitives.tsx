import { areaPath, linePath, ratioWidth, stackRects } from '../lib/scale';
import { bytes, pct } from '../lib/format';
import type { Ratio } from '../lib/api';

/**
 * A sparkline. Resizes with its container and never needs a ResizeObserver:
 * a fixed viewBox plus preserveAspectRatio="none" plus a Tailwind width class
 * does it in CSS.
 *
 * `vectorEffect="non-scaling-stroke"` is MANDATORY. Without it the non-uniform
 * scale stretches the stroke into a wedge — thick at one end, thin at the other
 * — which only shows up at wide aspect ratios, i.e. on the reviewer's monitor
 * and not yours.
 */
export function Sparkline({
  values,
  className = 'h-8 w-full',
  tone = 'emerald',
}: {
  values: number[];
  className?: string;
  tone?: 'emerald' | 'sky';
}) {
  const W = 100;
  const H = 30;
  const stroke = tone === 'emerald' ? 'stroke-emerald-400' : 'stroke-sky-400';
  const fill = tone === 'emerald' ? 'fill-emerald-500/15' : 'fill-sky-500/15';
  return (
    <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" className={className} aria-hidden="true">
      <path d={areaPath(values, W, H)} className={fill} />
      <path
        d={linePath(values, W, H)}
        className={`${stroke} fill-none`}
        strokeWidth={1.5}
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}

/** A one-dimensional bar. Two divs — use the platform; SVG here is overkill. */
export function RatioBar({ ratio, tone = 'emerald' }: { ratio: Ratio; tone?: 'emerald' | 'sky' }) {
  const w = ratioWidth(ratio);
  const bar = tone === 'emerald' ? 'bg-emerald-500' : 'bg-sky-500';
  return (
    <div className="h-1.5 w-full rounded-full bg-slate-700" title={pct(ratio)}>
      <div className={`h-1.5 rounded-full ${bar}`} style={{ width: `${w}%` }} />
    </div>
  );
}

/**
 * Stacked bars for a two-series time chart.
 *
 * Tooltips are native <title> elements: accessible, zero JS, zero state.
 */
export function StackedBars({
  a,
  b,
  labels,
  className = 'h-40 w-full',
}: {
  a: number[];
  b: number[];
  labels: string[];
  className?: string;
}) {
  const W = 300;
  const H = 100;
  const rects = stackRects(a, b, W, H, 1);
  return (
    <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" className={className} role="img">
      {rects.map((r, i) => (
        <g key={i}>
          <rect x={r.x} y={r.lower.y} width={r.w} height={r.lower.h} className="fill-emerald-500" />
          <rect x={r.x} y={r.upper.y} width={r.w} height={r.upper.h} className="fill-amber-500" />
          <rect x={r.x} y={0} width={r.w} height={H} className="fill-transparent">
            <title>{`${labels[i] ?? ''}\nfrom cache ${bytes(a[i])}\nfrom upstream ${bytes(b[i] ?? 0)}`}</title>
          </rect>
        </g>
      ))}
    </svg>
  );
}

/**
 * A KPI tile. Always shows the absolute figure next to the ratio: one 300 MB
 * kernel MISS tanks the package byte-ratio in a quiet week, and the absolute
 * number is what keeps that honest.
 */
export function Kpi({
  label,
  value,
  sub,
  ratio,
  spark,
  tone = 'emerald',
}: {
  label: string;
  value: string;
  sub?: string;
  ratio?: Ratio;
  spark?: number[];
  tone?: 'emerald' | 'sky';
}) {
  return (
    <div className="rounded-lg bg-slate-900 p-3 ring-1 ring-white/5">
      <div className="text-xs uppercase tracking-wide text-slate-400">{label}</div>
      {/* tabular-nums: without it a 5s-polling dashboard jitters horizontally. */}
      <div className="mt-1 font-mono text-2xl tabular-nums">{value}</div>
      {sub && <div className="text-xs tabular-nums text-slate-400">{sub}</div>}
      {ratio !== undefined && (
        <div className="mt-2">
          <div className="mb-1 flex justify-between font-mono text-xs tabular-nums text-slate-300">
            <span>hit ratio</span>
            <span>{pct(ratio)}</span>
          </div>
          <RatioBar ratio={ratio} tone={tone} />
        </div>
      )}
      {spark && <Sparkline values={spark} tone={tone} className="mt-2 h-8 w-full" />}
    </div>
  );
}
