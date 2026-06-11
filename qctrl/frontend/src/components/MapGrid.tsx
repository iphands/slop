import { useState, useMemo } from 'react';
import type { MapInfo } from '../lib/api';

interface MapGridProps {
  maps: MapInfo[];
  currentMap?: string;
  onSelect: (map: MapInfo) => void;
}

export function MapGrid({ maps, currentMap, onSelect }: MapGridProps) {
  const [search, setSearch] = useState('');

  const filteredMaps = useMemo(() => {
    if (!search) return maps;
    const query = search.toLowerCase();
    return maps.filter((m) => m.name.toLowerCase().includes(query));
  }, [maps, search]);

  if (maps.length === 0) {
    return <div className="text-gray-400">No maps found</div>;
  }

  return (
    <div className="space-y-4">
      <input
        type="text"
        placeholder="Search maps..."
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        className="w-full p-2 bg-gray-800 border border-gray-700 rounded focus:outline-none focus:border-blue-500"
      />
      {filteredMaps.length === 0 ? (
        <div className="text-gray-400">No maps found</div>
      ) : (
        <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
          {filteredMaps.map((map) => (
            <button
              key={map.name}
              type="button"
              onClick={() => onSelect(map)}
              className={`p-3 rounded border text-left transition-colors min-h-[72px] flex flex-col justify-between ${
                currentMap === map.name
                  ? 'bg-blue-900/50 border-blue-500'
                  : 'bg-gray-800 border-gray-700 hover:border-gray-600'
              }`}
            >
              <span className="font-medium truncate">{map.name}</span>
              {currentMap === map.name && (
                <span className="text-xs text-green-400">Current</span>
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
