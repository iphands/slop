import { useEffect, useRef, useCallback } from 'react';
import { useQuery } from '@tanstack/react-query';
import { getStatus } from '../lib/api';

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
  /** Callback when a rotation trigger is detected */
  onTrigger?: (event: TimerTriggerEvent) => void;
  /** Custom polling interval in milliseconds (default: 5000) */
  pollingInterval?: number;
}

export interface UseRotationTimerReturn {
  /** Current elapsed time in seconds since map start */
  elapsedSeconds: number;
  /** Current frag count */
  currentFrags: number;
  /** Time limit in minutes */
  timelimit: number;
  /** Frag limit */
  fraglimit: number;
  /** Countdown to next map in seconds (if time limit reached) */
  countdownSeconds: number;
  /** Whether time limit has been reached */
  timeLimitReached: boolean;
  /** Whether frag limit has been reached */
  fragLimitReached: boolean;
  /** Whether timer is active */
  isActive: boolean;
  /** Manual trigger reset (called after map change) */
  reset: () => void;
}

/**
 * Custom hook for polling server status and detecting rotation triggers
 * 
 * Polls /api/status every 5 seconds to track:
 * - Elapsed time since map start
 * - Current frag count
 * - Time limit and frag limit thresholds
 * 
 * Emits trigger events when limits are reached
 */
export function useRotationTimer(
  options: UseRotationTimerOptions = {}
): UseRotationTimerReturn {
  const {
    onTrigger,
    pollingInterval = 5000,
  } = options;

  // Track map start time (reset on map change)
  const mapStartTimeRef = useRef<number | null>(null);
  const lastTriggerRef = useRef<string | null>(null);

  // Fetch server status with custom polling interval
  const { data: status, isLoading } = useQuery({
    queryKey: ['status'],
    queryFn: getStatus,
    refetchInterval: pollingInterval,
  });

  // Extract server state
  const currentMap = status?.map ?? null;
  const timelimit = status?.timelimit ?? 0;
  const fraglimit = status?.fraglimit ?? 0;
  const currentFrags = status?.players?.reduce((sum, p) => sum + p.score, 0) ?? 0;

  // Detect map change and reset timer
  const prevMapRef = useRef<string | null>(null);
  useEffect(() => {
    if (currentMap && currentMap !== prevMapRef.current) {
      // Map changed, reset start time
      mapStartTimeRef.current = Date.now();
      lastTriggerRef.current = null;
      prevMapRef.current = currentMap;
    } else if (!currentMap) {
      // No map yet, clear start time
      mapStartTimeRef.current = null;
      prevMapRef.current = null;
    }
  }, [currentMap]);

  // Calculate elapsed time
  const elapsedSeconds = useCallback((): number => {
    if (!mapStartTimeRef.current || !currentMap) return 0;
    const now = Date.now();
    const elapsedMs = now - mapStartTimeRef.current;
    return Math.floor(elapsedMs / 1000);
  }, [currentMap]);

  // Detect triggers and emit events
  useEffect(() => {
    if (isLoading || !currentMap) return;

    const elapsed = elapsedSeconds();
    const timeLimitReached = timelimit > 0 && elapsed >= timelimit * 60;
    const fragLimitReached = fraglimit > 0 && currentFrags >= fraglimit;

    // Time limit trigger
    if (timeLimitReached && lastTriggerRef.current !== 'time_limit') {
      const event: TimerTriggerEvent = {
        type: 'time_limit',
        message: `Time limit reached: ${timelimit} minutes elapsed`,
        details: {
          elapsed,
          timelimit: timelimit * 60,
        },
      };
      lastTriggerRef.current = 'time_limit';
      onTrigger?.(event);
    }

    // Frag limit trigger
    if (fragLimitReached && lastTriggerRef.current !== 'frag_limit') {
      const event: TimerTriggerEvent = {
        type: 'frag_limit',
        message: `Frag limit reached: ${currentFrags} frags`,
        details: {
          frags: currentFrags,
          fraglimit,
        },
      };
      lastTriggerRef.current = 'frag_limit';
      onTrigger?.(event);
    }
  }, [isLoading, currentMap, timelimit, fraglimit, currentFrags, elapsedSeconds, onTrigger]);

  // Countdown calculation (time until limit reached, negative if exceeded)
  const countdownSeconds = useCallback((): number => {
    if (!timelimit || !mapStartTimeRef.current) return 0;
    const elapsed = elapsedSeconds();
    const limitSeconds = timelimit * 60;
    return limitSeconds - elapsed;
  }, [timelimit, elapsedSeconds]);

  // Reset function for manual trigger after map change
  const reset = useCallback(() => {
    mapStartTimeRef.current = Date.now();
    lastTriggerRef.current = null;
  }, []);

  return {
    elapsedSeconds: elapsedSeconds(),
    currentFrags,
    timelimit,
    fraglimit,
    countdownSeconds: countdownSeconds(),
    timeLimitReached: timelimit > 0 && elapsedSeconds() >= timelimit * 60,
    fragLimitReached: fraglimit > 0 && currentFrags >= fraglimit,
    isActive: !!currentMap && !isLoading,
    reset,
  };
}
