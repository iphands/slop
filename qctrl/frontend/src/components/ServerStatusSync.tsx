import { useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { getStatus } from '../lib/api';
import { useChanges } from '../contexts/ChangesContext';

/**
 * Syncs server status to the ChangesContext
 * This ensures all components see the real server state
 */
export function ServerStatusSync() {
  const { setServerState } = useChanges();
  
  const { data: status } = useQuery({
    queryKey: ['status'],
    queryFn: getStatus,
    refetchInterval: 2000,
  });

  useEffect(() => {
    if (status) {
      setServerState({
        dmflags: status.dmflags ?? 17424,
        timelimit: status.timelimit ?? 20,
        fraglimit: status.fraglimit ?? 25,
        currentMap: status.map ?? 'unknown',
      });
    }
  }, [status, setServerState]);

  return null;
}
