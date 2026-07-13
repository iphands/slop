import { useEffect, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { getStatus, type ClockQuality } from '../lib/api';

/**
 * A map countdown that is phase-locked to the server rather than free-running.
 *
 * The display needs to tick every second, but a browser-side timer that merely
 * counts drifts: it starts whenever the tab happened to notice the map change,
 * resets on reload, and disagrees between tabs. So the timer here does not own
 * the time — the backend does, and this hook re-derives its anchor from the
 * backend's `elapsed_seconds` on *every* poll:
 *
 *     localAnchor = <time of that response> - elapsed * 1000
 *
 * Between polls it free-runs at 1 Hz for smoothness; each poll snaps it back to
 * server truth. Drift therefore cannot accumulate — the worst case is bounded by
 * one poll interval, not by how long the tab has been open.
 *
 * `known === false` is a real state, not an error: if qctrl was not running when
 * the current map started, the elapsed time is genuinely unrecoverable (Quake 2
 * publishes no map clock). Callers get `null`, not a plausible-looking number.
 */
export interface MapClockState {
  map: string | null;
  /** True when the backend observed the map start. False means elapsed is unknowable. */
  known: boolean;
  /** Null when unknown. */
  elapsedSeconds: number | null;
  /** Null when unknown, when there is no timelimit, or when already overdue. */
  remainingSeconds: number | null;
  timelimitSeconds: number;
  quality: ClockQuality | 'offline';
}

export interface UseMapClockOptions {
  pollingInterval?: number;
}

export function useMapClock(options: UseMapClockOptions = {}): MapClockState {
  const { pollingInterval = 2000 } = options;

  // Shares the ['status'] cache with every other consumer — this adds no network.
  const { data: status, dataUpdatedAt, isError } = useQuery({
    queryKey: ['status'],
    queryFn: getStatus,
    refetchInterval: pollingInterval,
  });

  const clock = status?.clock;

  const known = clock?.anchor === 'exact' && clock.elapsed_seconds !== null;
  const serverElapsed = known ? clock!.elapsed_seconds! : null;

  /**
   * The skew correction: the wall-clock instant the current map started, as
   * implied by the most recent response.
   *
   * Derived from `dataUpdatedAt` rather than from the payload, because an
   * *identical* response is still a fresh reading of server truth and must still
   * re-anchor. Keying on the payload would let a countdown whose elapsed value
   * happened not to change silently stop re-syncing.
   */
  const anchor = useMemo(
    () => (serverElapsed === null ? null : dataUpdatedAt - serverElapsed * 1000),
    [dataUpdatedAt, serverElapsed]
  );

  // Drives the 1 Hz repaint between polls. This is the only thing the timer owns:
  // it does not accumulate time, it just re-reads the wall clock, and elapsed is
  // recomputed from `anchor` below. So a missed or throttled tick cannot make the
  // clock drift — it only makes it repaint late.
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (anchor === null) return;
    const interval = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(interval);
  }, [anchor]);

  const elapsedSeconds = useMemo(() => {
    if (anchor === null) return null;
    // `now` lags by up to one tick right after a poll lands; clamping to the
    // response time makes the freshly-anchored value show immediately rather
    // than briefly rendering the previous second.
    const at = Math.max(now, dataUpdatedAt);
    return Math.max(0, Math.floor((at - anchor) / 1000));
  }, [anchor, now, dataUpdatedAt]);

  const timelimitSeconds = (status?.timelimit ?? 0) * 60;

  let remainingSeconds: number | null = null;
  if (elapsedSeconds !== null && timelimitSeconds > 0) {
    const remaining = timelimitSeconds - elapsedSeconds;
    remainingSeconds = remaining > 0 ? remaining : null;
  }

  let quality: ClockQuality | 'offline' = clock?.quality ?? 'degraded';
  if (isError || status?.server_online === false) {
    quality = 'offline';
  }

  return {
    map: status?.map ?? null,
    known,
    elapsedSeconds,
    remainingSeconds,
    timelimitSeconds,
    quality,
  };
}
