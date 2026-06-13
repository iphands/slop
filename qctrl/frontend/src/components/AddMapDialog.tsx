import { useState, useMemo, useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { addMapToQueue } from '../lib/api';
import type { MapInfo } from '../lib/api';

interface AddMapDialogProps {
  isOpen: boolean;
  onClose: () => void;
  onMapAdded: () => void;
}

export function AddMapDialog({ isOpen, onClose, onMapAdded }: AddMapDialogProps) {
  const [search, setSearch] = useState('');
  const [selectedMap, setSelectedMap] = useState<string | null>(null);
  const [isAdding, setIsAdding] = useState(false);

  const { data: mapsData } = useQuery({
    queryKey: ['maps'],
    queryFn: () => fetch('/api/maps').then((res) => res.json()),
  });

  const allMaps = useMemo(() => mapsData?.maps || [], [mapsData]);

  const filteredMaps = useMemo(() => {
    if (!search) return allMaps;
    const query = search.toLowerCase();
    return allMaps.filter((m: MapInfo) => m.name.toLowerCase().includes(query));
  }, [allMaps, search]);

  useEffect(() => {
    if (!isOpen) {
      setSearch('');
      setSelectedMap(null);
    }
  }, [isOpen]);

  const handleAdd = async () => {
    if (!selectedMap) return;
    
    setIsAdding(true);
    try {
      await addMapToQueue(selectedMap);
      onMapAdded();
      onClose();
    } catch (error) {
      console.error('Failed to add map to queue:', error);
    } finally {
      setIsAdding(false);
    }
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
      <div className="bg-gray-900 rounded-lg border border-gray-700 w-full max-w-md max-h-[80vh] flex flex-col">
        <div className="p-4 border-b border-gray-700">
          <h3 className="text-lg font-semibold text-gray-100">Add Map to Queue</h3>
        </div>

        <div className="p-4 space-y-4 flex-1 overflow-auto">
          <div>
            <label className="block text-sm text-gray-400 mb-2">Search Maps</label>
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Type to filter maps..."
              className="w-full p-2 bg-gray-800 border border-gray-700 rounded focus:outline-none focus:border-orange-500 text-gray-100 placeholder-gray-500"
              autoFocus
            />
          </div>

          <div className="max-h-64 overflow-auto">
            {filteredMaps.length === 0 ? (
              <p className="text-gray-500 text-sm text-center py-4">
                {search ? 'No maps match your search' : 'No maps available'}
              </p>
            ) : (
              <div className="space-y-1">
                {filteredMaps.map((map: MapInfo) => (
                  <button
                    key={map.name}
                    type="button"
                    onClick={() => setSelectedMap(map.name)}
                    className={`w-full p-3 text-left rounded transition-colors ${
                      selectedMap === map.name
                        ? 'bg-orange-600 text-white'
                        : 'bg-gray-800 text-gray-200 hover:bg-gray-700'
                    }`}
                  >
                    <div className="font-medium">{map.name}</div>
                    <div className="text-xs opacity-70">{map.filename}</div>
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>

        <div className="p-4 border-t border-gray-700 flex gap-2 justify-end">
          <button
            type="button"
            onClick={onClose}
            disabled={isAdding}
            className="px-4 py-2 bg-gray-700 hover:bg-gray-600 rounded text-sm transition-colors disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={handleAdd}
            disabled={!selectedMap || isAdding}
            className="px-4 py-2 bg-orange-600 hover:bg-orange-700 rounded text-sm font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {isAdding ? 'Adding...' : 'Add to Queue'}
          </button>
        </div>
      </div>
    </div>
  );
}
