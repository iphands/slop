import { useCallback, useRef, useState } from 'react';
import type { TimerTriggerEvent } from './useRotationTimer';
import { executeRcon, getMaps } from '../lib/api';

export interface MapRotationOptions {
  mode: 'Sequential' | 'Random';
  queueMaps: string[];
  currentMap: string | null;
  onMapChangeStart?: (mapName: string) => void;
  onMapChangeComplete?: (mapName: string) => void;
  onMapChangeError?: (error: string) => void;
  onResetTimer?: () => void;
}

export interface MapRotationReturn {
  handleTrigger: (event: TimerTriggerEvent) => Promise<void>;
  isSwitching: boolean;
  switchingTo: string | null;
}

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
  const [isSwitchingState, setIsSwitchingState] = useState(false);
  const [switchingToState, setSwitchingToState] = useState<string | null>(null);

  const handleTrigger = useCallback(
    async () => {
      if (isSwitchingRef.current) {
        return;
      }

      try {
        isSwitchingRef.current = true;
        setIsSwitchingState(true);
        switchingToRef.current = await determineNextMap(mode, queueMaps, currentMap);
        setSwitchingToState(switchingToRef.current);

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
        setIsSwitchingState(false);
        setSwitchingToState(null);
      }
    },
    [mode, queueMaps, currentMap, onMapChangeStart, onMapChangeComplete, onMapChangeError, onResetTimer]
  );

  return {
    handleTrigger,
    isSwitching: isSwitchingState,
    switchingTo: switchingToState,
  };
}

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

export async function fetchAvailableMaps(): Promise<string[]> {
  try {
    const response = await getMaps();
    return response.maps.map((m: { name: string }) => m.name);
  } catch {
    return ['q2dm1'];
  }
}
