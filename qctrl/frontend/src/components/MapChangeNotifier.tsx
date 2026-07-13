import { useEffect, useRef } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useNotifications } from '../hooks/useNotifications';
import { getStatus } from '../lib/api';

/**
 * Toasts when the map changes.
 *
 * This used to be <RotationController />, which *drove* the rotation: a timer in
 * the browser fired `map <next>` over rcon when the timelimit was up. That made
 * rotation a property of having a tab open. Quake 2 never leaves intermission on
 * its own — the only exit is a connected client pressing a button five seconds in
 * — so an unattended server would reach the timelimit, park in intermission, and
 * sit there until someone loaded the frontend. Rotation now lives in the backend
 * (`crates/api/src/rotator.rs`), which runs headless.
 *
 * What's left here is only the part that genuinely needs a browser: telling the
 * person looking at the screen that the map moved. It observes, it does not act.
 * Renders nothing visible.
 */
export function MapChangeNotifier() {
  const { addNotification } = useNotifications();

  const { data: status } = useQuery({
    queryKey: ['status'],
    queryFn: getStatus,
    refetchInterval: 2000,
  });

  const map = status?.map ?? null;

  // Seed on the first map we see rather than announcing it: opening the page is
  // not a map change.
  const previousMap = useRef<string | null>(null);
  const seeded = useRef(false);

  useEffect(() => {
    if (!map) return;

    if (!seeded.current) {
      seeded.current = true;
      previousMap.current = map;
      return;
    }

    if (map !== previousMap.current) {
      previousMap.current = map;
      addNotification('success', `Map changed to ${map}`);
    }
  }, [map, addNotification]);

  return null;
}
