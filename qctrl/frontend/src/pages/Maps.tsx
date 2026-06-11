import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { getMaps } from '../lib/api';
import type { MapInfo } from '../lib/api';
import { MapGrid } from '../components/MapGrid';
import { MapDialog } from '../components/MapDialog';
import { CurrentMap } from '../components/CurrentMap';

export function Maps() {
  const [selectedMap, setSelectedMap] = useState<MapInfo | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);

  const { data: data, isLoading, error } = useQuery({
    queryKey: ['maps'],
    queryFn: getMaps,
  });

  const maps = data?.maps || [];

  const handleMapSelect = (map: MapInfo) => {
    setSelectedMap(map);
    setDialogOpen(true);
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
            maps={maps}
            currentMap="q2dm1"
            onSelect={handleMapSelect}
          />
        )}
      </section>

      <MapDialog
        map={selectedMap}
        open={dialogOpen}
        onOpenChange={setDialogOpen}
      />
    </div>
  );
}
