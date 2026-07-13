/**
 * Seconds as a match clock: `M:SS`, or `H:MM:SS` past an hour.
 *
 * Negative input clamps to zero — a countdown that has run out reads `0:00`, not
 * `-1:-3`.
 */
export function formatClock(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const secs = total % 60;

  const pad = (n: number) => String(n).padStart(2, '0');

  return hours > 0
    ? `${hours}:${pad(minutes)}:${pad(secs)}`
    : `${minutes}:${pad(secs)}`;
}
