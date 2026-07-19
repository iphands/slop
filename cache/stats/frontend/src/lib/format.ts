// Display formatting. Kept pure and separate so it can be unit-tested, and so
// the "never show a number you cannot back up" rules live in one place.

const UNITS = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB'];

/** Binary byte sizes, 3 significant figures. */
export function bytes(n: number | null | undefined): string {
  if (n === null || n === undefined || !Number.isFinite(n)) return '—';
  if (n < 1024) return `${Math.round(n)} B`;
  let v = n;
  let u = 0;
  while (v >= 1024 && u < UNITS.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v >= 100 ? v.toFixed(0) : v >= 10 ? v.toFixed(1) : v.toFixed(2)} ${UNITS[u]}`;
}

/**
 * A ratio as a percentage — or an em dash when there is no answer.
 *
 * `null` means "no requests in this window". Rendering it as 0% would make a
 * healthy cold cache look broken, which is the failure the whole
 * metadata/package split exists to prevent.
 */
export function pct(r: number | null | undefined): string {
  if (r === null || r === undefined || !Number.isFinite(r)) return '—';
  return `${(r * 100).toFixed(r >= 0.995 || r === 0 ? 0 : 1)}%`;
}

/** Thousands-separated integer. */
export function num(n: number | null | undefined): string {
  if (n === null || n === undefined || !Number.isFinite(n)) return '—';
  return n.toLocaleString('en-US');
}

/** Compact relative time, e.g. "3m ago". */
export function rel(epochSecs: number, now = Date.now() / 1000): string {
  if (!epochSecs) return 'never';
  const d = Math.max(0, Math.round(now - epochSecs));
  if (d < 60) return `${d}s ago`;
  if (d < 3600) return `${Math.round(d / 60)}m ago`;
  if (d < 86400) return `${Math.round(d / 3600)}h ago`;
  return `${Math.round(d / 86400)}d ago`;
}

/** Hour-of-day label for a series point. */
export function hourLabel(t: number): string {
  return new Date(t * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

export function dayLabel(t: number): string {
  return new Date(t * 1000).toLocaleDateString([], { month: 'short', day: 'numeric' });
}
