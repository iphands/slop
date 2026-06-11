import { useQuery } from '@tanstack/react-query';
import { getMaps } from '../lib/api';

export function MapList() {
  const { data: maps, isLoading, error } = useQuery({
    queryKey: ['maps'],
    queryFn: getMaps,
  });

  if (isLoading) {
    return <div className="text-gray-400">Loading maps...</div>;
  }

  if (error) {
    return <div className="text-red-400">Failed to load maps</div>;
  }

  if (!maps || maps.length === 0) {
    return <div className="text-gray-400">No maps found</div>;
  }

  return (
    <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-2">
      {maps.map((map) => (
        <div
          key={map.name}
          className="p-3 bg-gray-800 rounded border border-gray-700 hover:border-gray-600"
        >
          <div className="font-medium">{map.name}</div>
          <div className="text-xs text-gray-400">{map.filename}</div>
        </div>
      ))}
    </div>
  );
}
