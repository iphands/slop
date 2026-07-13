import { useEffect, useRef } from 'react';
import { useQuery } from '@tanstack/react-query';
import { getStatus } from '../lib/api';
import { useChanges } from '../contexts/ChangesContext';

/**
 * Syncs server status to the ChangesContext
 * Only updates when server state actually changes to avoid re-render storms
 */
export function ServerStatusSync() {
  const { setServerState } = useChanges();
  const lastStatusRef = useRef<string | null>(null);
  // A poll taken mid-map-change (or against a server that's down) comes back
  // without a map. Remember the last good one instead of degrading to 'unknown'
  // and disabling Apply/Restart over a momentary blip.
  const lastGoodMapRef = useRef<string | null>(null);

  const { data: status } = useQuery({
    queryKey: ['status'],
    queryFn: getStatus,
    refetchInterval: 2000,
  });

  useEffect(() => {
    if (!status) return;

    // Create a stable key to detect actual changes
    const statusKey = `${status.map}-${status.dmflags}-${status.timelimit}-${status.fraglimit}`;
    
    if (status.map && status.map.trim() !== '') {
      lastGoodMapRef.current = status.map;
    }

    // Only update if something actually changed
    if (lastStatusRef.current !== statusKey) {
      lastStatusRef.current = statusKey;
      setServerState({
        dmflags: status.dmflags ?? 17424,
        timelimit: status.timelimit ?? 20,
        fraglimit: status.fraglimit ?? 25,
        currentMap: lastGoodMapRef.current ?? 'unknown',
      });
    }
  }, [status, setServerState]);

  return null;
}
