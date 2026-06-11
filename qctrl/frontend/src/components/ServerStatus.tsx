import { useQuery } from '@tanstack/react-query';
import { getHealth } from '../lib/api';

export function ServerStatus() {
  const { data: health, refetch } = useQuery({
    queryKey: ['health'],
    queryFn: getHealth,
    refetchInterval: 10000,
  });

  return (
    <div className="flex items-center gap-2">
      {health?.status === 'ok' ? (
        <>
          <span className="text-green-500 text-lg">●</span>
          <span className="text-green-400">Connected</span>
        </>
      ) : (
        <>
          <span className="text-red-500 text-lg">●</span>
          <span className="text-red-400">Disconnected</span>
        </>
      )}
      <button
        onClick={() => refetch()}
        className="ml-2 px-2 py-1 text-sm bg-gray-700 hover:bg-gray-600 rounded"
      >
        Refresh
      </button>
    </div>
  );
}
