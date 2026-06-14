import { useQuery } from '@tanstack/react-query';
import { useRotationTimer } from '../hooks/useRotationTimer';
import { useMapRotation } from '../hooks/useMapRotation';
import { useNotifications } from '../hooks/useNotifications';
import { getRotationQueue } from '../lib/api';

/**
 * Owns automatic map rotation for the whole app.
 *
 * This is mounted at the app root (not on the Rotation page) so rotation keeps
 * running regardless of which page is open. Previously the rotation timer lived
 * inside the `/rotation` page, which meant qctrl never rotated when the user was
 * anywhere else and the server's own (empty-maplist) rotation crashed the
 * server with `maps/.bsp`.
 *
 * It reads the queue from the shared react-query cache (same key QueueList
 * uses) and drives `useRotationTimer` + `useMapRotation`. Rotation events are
 * surfaced through the global notification store. Renders nothing visible.
 */
export function RotationController() {
  const { addNotification } = useNotifications();

  const { data: queue } = useQuery({
    queryKey: ['rotationQueue'],
    queryFn: getRotationQueue,
    refetchInterval: 5000,
  });

  const mode = queue?.mode ?? 'Sequential';
  const queueMaps = queue?.maps ?? [];
  const currentMap = queue?.current_map ?? null;
  const rotationEnabled = queue?.enabled ?? true;

  const { handleTrigger: handleMapRotation } = useMapRotation({
    mode,
    queueMaps,
    currentMap,
    rotationEnabled,
    onMapChangeStart: (mapName) => addNotification('info', `Switching to ${mapName}...`),
    onMapChangeComplete: (mapName) => addNotification('success', `Switched to ${mapName}`),
    onMapChangeError: (error) => addNotification('error', `Failed to switch map: ${error}`),
    onResetTimer: () => {
      // Timer reset is handled by useRotationTimer detecting the map change.
    },
  });

  useRotationTimer({
    onTrigger: async (event) => {
      const which = event.type === 'time_limit' ? 'Time limit' : 'Frag limit';
      addNotification('info', `${which} reached — switching map...`);
      await handleMapRotation(event);
    },
  });

  return null;
}
