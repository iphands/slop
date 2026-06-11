import { useQuery } from '@tanstack/react-query';
import { getStatus } from '../lib/api';

export function CurrentMap() {
  const { data: status, isLoading, error } = useQuery({
    queryKey: ['status'],
    queryFn: getStatus,
  });

  const currentMap = status?.map || 'Unknown';
  const players = status?.players?.length || 0;

  if (isLoading) {
    return (
      <div className="p-4 bg-gray-800 rounded-lg">
        <h2 className="text-lg font-semibold mb-2">Current Map</h2>
        <p className="text-2xl font-bold">Loading...</p>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-4 bg-gray-800 rounded-lg">
        <h2 className="text-lg font-semibold mb-2">Current Map</h2>
        <p className="text-2xl font-bold text-red-400">Error loading</p>
      </div>
    );
  }

  return (
    <div className="p-4 bg-gray-800 rounded-lg">
      <h2 className="text-lg font-semibold mb-2">Current Map</h2>
      <p className="text-2xl font-bold">{currentMap}</p>
      <p className="text-sm text-gray-400 mt-1">
        {players} player{players !== 1 ? 's' : ''} connected
      </p>
    </div>
  );
}
