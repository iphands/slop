import { describe, it, expect } from 'vitest';
import { linePath, areaPath, stackRects, ratioWidth } from './scale';
import { bytes, pct, num, rel } from './format';

describe('scale', () => {
  it('returns an empty path for an empty series rather than NaN', () => {
    expect(linePath([], 100, 30)).toBe('');
    expect(areaPath([], 100, 30)).toBe('');
    expect(stackRects([], [], 100, 30)).toEqual([]);
  });

  it('never emits NaN or Infinity for an all-zero series', () => {
    // The divide-by-zero case: a brand new cache with no traffic yet.
    const p = linePath([0, 0, 0], 100, 30);
    expect(p).not.toMatch(/NaN|Infinity/);
    expect(p).toBe('M0.00,30.00 L50.00,30.00 L100.00,30.00');
  });

  it('handles a single point without dividing by zero', () => {
    expect(linePath([5], 100, 30)).toBe('M0.00,0.00');
  });

  it('scales a known series to the viewBox', () => {
    // max=10 -> the peak touches y=0, the zero sits on the baseline.
    expect(linePath([0, 10], 100, 30)).toBe('M0.00,30.00 L100.00,0.00');
  });

  it('closes the area path back to the baseline', () => {
    expect(areaPath([0, 10], 100, 30)).toBe('M0.00,30.00 L100.00,0.00 L100.00,30.00 L0,30.00 Z');
  });

  it('stacks two series against one shared scale', () => {
    const r = stackRects([10, 0], [0, 10], 100, 30, 0);
    expect(r).toHaveLength(2);
    expect(r[0].lower.h).toBeCloseTo(30);
    expect(r[0].upper.h).toBeCloseTo(0);
    expect(r[1].lower.h).toBeCloseTo(0);
    expect(r[1].upper.h).toBeCloseTo(30);
    // Bars must never collapse to zero width, or they vanish entirely.
    expect(r[0].w).toBeGreaterThan(0);
  });

  it('clamps ratio widths and treats null as empty', () => {
    expect(ratioWidth(null)).toBe(0);
    expect(ratioWidth(undefined)).toBe(0);
    expect(ratioWidth(0.5)).toBe(50);
    expect(ratioWidth(2)).toBe(100);
    expect(ratioWidth(-1)).toBe(0);
  });
});

describe('format', () => {
  it('renders an em dash rather than 0% when there is no data', () => {
    // A cold cache is not a broken cache.
    expect(pct(null)).toBe('—');
    expect(pct(undefined)).toBe('—');
    expect(bytes(null)).toBe('—');
    expect(num(null)).toBe('—');
  });

  it('formats real byte magnitudes from production', () => {
    expect(bytes(0)).toBe('0 B');
    expect(bytes(1468)).toBe('1.43 KiB'); // the real linux-image metapackage
    expect(bytes(14185752)).toBe('13.5 MiB'); // mesa-vulkan-drivers
    expect(bytes(53141396)).toBe('50.7 MiB'); // one real apt run
  });

  it('formats percentages without false precision', () => {
    expect(pct(0)).toBe('0%');
    expect(pct(1)).toBe('100%');
    expect(pct(0.7503)).toBe('75.0%');
  });

  it('formats relative times', () => {
    const now = 1_000_000;
    expect(rel(now, now)).toBe('0s ago');
    expect(rel(now - 90, now)).toBe('2m ago');
    expect(rel(now - 7200, now)).toBe('2h ago');
    expect(rel(0, now)).toBe('never');
  });
});
