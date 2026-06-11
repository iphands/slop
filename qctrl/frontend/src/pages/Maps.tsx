import { useQuery } from '@tanstack/react-query';
import { useChanges } from '../contexts/ChangesContext';
import { getMaps, getStatus } from '../lib/api';
import type { MapInfo } from '../lib/api';
import { MapGrid } from '../components/MapGrid';
import { CurrentMap } from '../components/CurrentMap';

export function Maps() {
  const { queueChange } = useChanges();
  const { isLoading: mapsLoading, error: mapsError } = useQuery({
    queryKey: ['maps'],
    queryFn: getMaps,
  });
  
  const { data: status } = useQuery({
    queryKey: ['status'],
    queryFn: getStatus,
    refetchInterval: 2000,
  });

  const handleMapSelect = (map: MapInfo) => {
    queueChange({
      type: 'map',
      pendingValue: map.name,
      description: 'Map change',
    });
  };

  return (
    <div className="space-y-6">
      <CurrentMap />

      <section className="p-4 bg-gray-800 rounded-lg">
        <h2 className="text-lg font-semibold mb-4">Select Map</h2>
        {mapsLoading ? (
          <div className="text-gray-400">Loading maps...</div>
        ) : mapsError ? (
          <div className="text-red-400">Failed to load maps</div>
        ) : (
          <MapGrid
            currentMap={status?.map ?? undefined}
            onSelect={handleMapSelect}
          />
        )}
      </section>
    </div>
  );
}
