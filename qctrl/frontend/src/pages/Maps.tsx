import { useQuery } from '@tanstack/react-query';
import { useChanges } from '../contexts/ChangesContext';
import { getMaps } from '../lib/api';
import type { MapInfo } from '../lib/api';
import { MapGrid } from '../components/MapGrid';
import { CurrentMap } from '../components/CurrentMap';

export function Maps() {
  const { queueChange } = useChanges();
  const { isLoading, error } = useQuery({
    queryKey: ['maps'],
    queryFn: getMaps,
  });

  const handleMapSelect = (map: MapInfo) => {
    // Queue the map change directly without confirmation
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
        {isLoading ? (
          <div className="text-gray-400">Loading maps...</div>
        ) : error ? (
          <div className="text-red-400">Failed to load maps</div>
        ) : (
          <MapGrid
            currentMap="q2dm1"
            onSelect={handleMapSelect}
          />
        )}
      </section>
    </div>
  );
}
