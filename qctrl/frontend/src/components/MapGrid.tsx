import { useState, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { getMaps, getFavorites, addFavorite, removeFavorite } from '../lib/api';
import type { MapInfo } from '../lib/api';

interface MapGridProps {
  currentMap?: string;
  onSelect: (map: MapInfo) => void;
}

export function MapGrid({ currentMap, onSelect }: MapGridProps) {
  const [search, setSearch] = useState('');

  const { data: mapsData } = useQuery({
    queryKey: ['maps'],
    queryFn: getMaps,
  });

  const { data: favoritesData } = useQuery({
    queryKey: ['favorites'],
    queryFn: getFavorites,
  });

  const maps = mapsData?.maps || [];
  const favorites = favoritesData?.favorites || [];

  const filteredMaps = useMemo(() => {
    if (!search) return maps;
    const query = search.toLowerCase();
    return maps.filter((m) => m.name.toLowerCase().includes(query));
  }, [maps, search]);

  const favoriteMaps = useMemo(() => {
    return filteredMaps.filter((m) => favorites.includes(m.name));
  }, [filteredMaps, favorites]);

  const allOtherMaps = useMemo(() => {
    return filteredMaps.filter((m) => !favorites.includes(m.name));
  }, [filteredMaps, favorites]);

  if (maps.length === 0) {
    return <div className="text-gray-400">No maps found</div>;
  }

  const handleFavoriteToggle = async (e: React.MouseEvent, map: MapInfo) => {
    e.stopPropagation();
    try {
      if (favorites.includes(map.name)) {
        await removeFavorite(map.name);
      } else {
        await addFavorite(map.name);
      }
    } catch (error) {
      console.error('Failed to toggle favorite:', error);
    }
  };

  const MapCard = ({ map }: { map: MapInfo }) => (
    <button
      type="button"
      onClick={() => onSelect(map)}
      className={`p-3 rounded border text-left transition-colors min-h-[72px] flex flex-col justify-between group relative ${
        currentMap === map.name
          ? 'bg-blue-900/50 border-blue-500'
          : 'bg-gray-800 border-gray-700 hover:border-gray-600'
      }`}
    >
      <span className="font-medium truncate">{map.name}</span>
      {currentMap === map.name && (
        <span className="text-xs text-green-400">Current</span>
      )}
      <button
        type="button"
        onClick={(e) => handleFavoriteToggle(e, map)}
        className="absolute top-2 right-2 text-lg"
        title={favorites.includes(map.name) ? 'Remove from favorites' : 'Add to favorites'}
      >
        {favorites.includes(map.name) ? '★' : '☆'}
      </button>
    </button>
  );

  return (
    <div className="space-y-6">
      <input
        type="text"
        placeholder="Search maps..."
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        className="w-full p-2 bg-gray-800 border border-gray-700 rounded focus:outline-none focus:border-blue-500"
      />

      {favorites.length > 0 && (
        <section>
          <h3 className="text-md font-semibold mb-3 flex items-center gap-2">
            <span>★</span> Favorites
          </h3>
          <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
            {favoriteMaps.map((map) => (
              <MapCard key={map.name} map={map} />
            ))}
          </div>
        </section>
      )}

      <section>
        <h3 className="text-md font-semibold mb-3">All Maps</h3>
        <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
          {allOtherMaps.map((map) => (
            <MapCard key={map.name} map={map} />
          ))}
        </div>
      </section>
    </div>
  );
}
