import { useEffect, useRef, useCallback, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { getStatus } from '../lib/api';

/**
 * Advance the map this many seconds before the server's real timelimit. qctrl's
 * clock starts when it detects the map change (lagging the server by up to one
 * poll) and ticks on a coarse interval, so this margin lets qctrl reliably win
 * the race against the server's authoritative end-of-match rotation. The
 * server's `sv_maplist` (kept in sync by the backend) is the fallback if qctrl
 * still loses.
 */
const EARLY_FIRE_SECONDS = 45;

export interface TimerTriggerEvent {
  type: 'time_limit' | 'frag_limit';
  message: string;
  details: {
    elapsed?: number;
    timelimit?: number;
    frags?: number;
    fraglimit?: number;
  };
}

export interface UseRotationTimerOptions {
  onTrigger?: (event: TimerTriggerEvent) => void;
  pollingInterval?: number;
}

export interface UseRotationTimerReturn {
  elapsedSeconds: number;
  currentFrags: number;
  timelimit: number;
  fraglimit: number;
  countdownSeconds: number;
  timeLimitReached: boolean;
  fragLimitReached: boolean;
  isActive: boolean;
  reset: () => void;
}

export function useRotationTimer(
  options: UseRotationTimerOptions = {}
): UseRotationTimerReturn {
  const {
    onTrigger,
    pollingInterval = 5000,
  } = options;

  const mapStartTimeRef = useRef<number | null>(null);
  const lastTriggerRef = useRef<string | null>(null);
  const [elapsedSeconds, setElapsedSeconds] = useState(0);

  const { data: status, isLoading } = useQuery({
    queryKey: ['status'],
    queryFn: getStatus,
    refetchInterval: pollingInterval,
  });

  const currentMap = status?.map ?? null;
  const timelimit = status?.timelimit ?? 0;
  const fraglimit = status?.fraglimit ?? 0;
  const currentFrags = status?.players?.reduce((sum, p) => sum + p.score, 0) ?? 0;

  const prevMapRef = useRef<string | null>(null);
  const shouldResetRef = useRef(false);

  useEffect(() => {
    if (currentMap && currentMap !== prevMapRef.current) {
      mapStartTimeRef.current = Date.now();
      lastTriggerRef.current = null;
      prevMapRef.current = currentMap;
      shouldResetRef.current = true;
    } else if (!currentMap) {
      mapStartTimeRef.current = null;
      prevMapRef.current = null;
      shouldResetRef.current = true;
    }
  }, [currentMap]);

  useEffect(() => {
    if (shouldResetRef.current) {
      setElapsedSeconds(0);
      shouldResetRef.current = false;
    }
  }, []);

  useEffect(() => {
    if (!currentMap || !mapStartTimeRef.current) return;

    const updateElapsed = () => {
      if (!mapStartTimeRef.current) return;
      const now = Date.now();
      const elapsedMs = now - mapStartTimeRef.current;
      setElapsedSeconds(Math.floor(elapsedMs / 1000));
    };

    updateElapsed();
    const interval = setInterval(updateElapsed, 1000);
    return () => clearInterval(interval);
  }, [currentMap]);

  useEffect(() => {
    if (isLoading || !currentMap) return;

    // Fire before the server's authoritative timelimit so qctrl owns the
    // rotation instead of racing (and losing to) the server's end-of-match
    // rotation.
    const timeLimitReached =
      timelimit > 0 && elapsedSeconds >= timelimit * 60 - EARLY_FIRE_SECONDS;

    if (timeLimitReached && lastTriggerRef.current !== 'time_limit') {
      const event: TimerTriggerEvent = {
        type: 'time_limit',
        message: `Time limit reached: ${timelimit} minutes elapsed`,
        details: {
          elapsed: elapsedSeconds,
          timelimit: timelimit * 60,
        },
      };
      lastTriggerRef.current = 'time_limit';
      onTrigger?.(event);
    }

    // Frag-limit rotation is intentionally NOT triggered here. The server runs
    // its end-of-match logic the same frame frags hit the limit, so qctrl can't
    // preempt it — the server's `sv_maplist` handles frag-triggered rotation.
  }, [isLoading, currentMap, timelimit, elapsedSeconds, onTrigger]);

  const countdownSeconds = useMemo(() => {
    if (!timelimit) return 0;
    const limitSeconds = timelimit * 60;
    return limitSeconds - elapsedSeconds;
  }, [timelimit, elapsedSeconds]);

  const reset = useCallback(() => {
    mapStartTimeRef.current = Date.now();
    lastTriggerRef.current = null;
    setElapsedSeconds(0);
  }, []);

  const timeLimitReached =
    timelimit > 0 && elapsedSeconds >= timelimit * 60 - EARLY_FIRE_SECONDS;
  const fragLimitReached = fraglimit > 0 && currentFrags >= fraglimit;

  return {
    elapsedSeconds,
    currentFrags,
    timelimit,
    fraglimit,
    countdownSeconds,
    timeLimitReached,
    fragLimitReached,
    isActive: !!currentMap && !isLoading,
    reset,
  };
}
