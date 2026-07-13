import { describe, it, expect } from 'vitest';
import { formatClock } from '../time';

describe('formatClock', () => {
  it('formats under an hour as M:SS', () => {
    expect(formatClock(0)).toBe('0:00');
    expect(formatClock(7)).toBe('0:07');
    expect(formatClock(65)).toBe('1:05');
    expect(formatClock(599)).toBe('9:59');
  });

  it('formats an hour or more as H:MM:SS', () => {
    expect(formatClock(3600)).toBe('1:00:00');
    expect(formatClock(3725)).toBe('1:02:05');
  });

  // A countdown that has run out reads 0:00, never a negative clock.
  it('clamps negatives to zero', () => {
    expect(formatClock(-1)).toBe('0:00');
    expect(formatClock(-90)).toBe('0:00');
  });

  it('truncates fractional seconds', () => {
    expect(formatClock(65.9)).toBe('1:05');
  });
});
