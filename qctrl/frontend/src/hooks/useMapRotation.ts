import { useCallback, useRef } from 'react';
import type { TimerTriggerEvent } from './useRotationTimer';
import { executeRcon, getMaps } from '../lib/api';

export interface MapRotationOptions {
  /** Current rotation mode */
  mode: 'Sequential' | 'Random';
  /** Current queue maps */
  queueMaps: string[];
  /** Current map name */
  currentMap: string | null;
  /** Callback when map change starts */
  onMapChangeStart?: (mapName: string) => void;
  /** Callback when map change completes */
  onMapChangeComplete?: (mapName: string) => void;
  /** Callback when map change fails */
  onMapChangeError?: (error: string) => void;
  /** Callback to reset timer after map change */
  onResetTimer?: () => void;
}

export interface MapRotationReturn {
  /** Execute map rotation when trigger fires */
  handleTrigger: (event: TimerTriggerEvent) => Promise<void>;
  /** Whether a map change is in progress */
  isSwitching: boolean;
  /** Current map being switched to (or null) */
  switchingTo: string | null;
}

/**
 * Custom hook for handling automatic map rotation
 * 
 * Determines next map based on mode:
 * - Sequential: Next map in queue (loop to 0 at end)
 * - Random: Random map from queue
 * - Empty queue: Sequential loops to first, Random picks from all available
 */
export function useMapRotation(
  options: MapRotationOptions
): MapRotationReturn {
  const {
    mode,
    queueMaps,
    currentMap,
    onMapChangeStart,
    onMapChangeComplete,
    onMapChangeError,
    onResetTimer,
  } = options;

  const isSwitchingRef = useRef(false);
  const switchingToRef = useRef<string | null>(null);

  const handleTrigger = useCallback(
    async (_event: TimerTriggerEvent) => {
      if (isSwitchingRef.current) {
        return;
      }

      try {
        isSwitchingRef.current = true;
        switchingToRef.current = await determineNextMap(mode, queueMaps, currentMap);

        const nextMap = switchingToRef.current;

        onMapChangeStart?.(nextMap);

        await executeRcon(`map ${nextMap}`);

        onMapChangeComplete?.(nextMap);

        onResetTimer?.();
      } catch (error) {
        const errorMessage = error instanceof Error ? error.message : 'Unknown error';
        onMapChangeError?.(errorMessage);
      } finally {
        isSwitchingRef.current = false;
        switchingToRef.current = null;
      }
    },
    [mode, queueMaps, currentMap, onMapChangeStart, onMapChangeComplete, onMapChangeError, onResetTimer]
  );

  return {
    handleTrigger,
    isSwitching: isSwitchingRef.current,
    switchingTo: switchingToRef.current,
  };
}

/**
 * Determine the next map based on rotation mode and queue state
 */
export async function determineNextMap(
  mode: 'Sequential' | 'Random',
  queueMaps: string[],
  currentMap: string | null
): Promise<string> {
  if (queueMaps.length === 0) {
    if (mode === 'Sequential') {
      return currentMap ?? 'q2dm1';
    } else {
      const allMaps = await fetchAvailableMaps();
      const randomIndex = Math.floor(Math.random() * allMaps.length);
      return allMaps[randomIndex];
    }
  }

  if (mode === 'Sequential') {
    const currentIndex = queueMaps.indexOf(currentMap ?? '');
    
    if (currentIndex === -1 || currentIndex >= queueMaps.length - 1) {
      return queueMaps[0];
    }
    
    return queueMaps[currentIndex + 1];
  }

  if (queueMaps.length === 1) {
    return queueMaps[0];
  }

  const randomIndex = Math.floor(Math.random() * queueMaps.length);
  return queueMaps[randomIndex];
}

/**
 * Fetch all available maps from backend when queue is empty
 */
export async function fetchAvailableMaps(): Promise<string[]> {
  try {
    const response = await getMaps();
    return response.maps.map((m: { name: string }) => m.name);
  } catch {
    // Fallback to default map
    return ['q2dm1'];
  }
}
