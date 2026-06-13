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
  const [selectedMaps, setSelectedMaps] = useState<Set<string>>(new Set());
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
      setSelectedMaps(new Set());
    }
  }, [isOpen]);

  const toggleMap = (mapName: string) => {
    setSelectedMaps(prev => {
      const next = new Set(prev);
      if (next.has(mapName)) {
        next.delete(mapName);
      } else {
        next.add(mapName);
      }
      return next;
    });
  };

  const handleAdd = async () => {
    if (selectedMaps.size === 0) return;
    
    setIsAdding(true);
    try {
      const mapsArray = Array.from(selectedMaps);
      for (const mapName of mapsArray) {
        await addMapToQueue(mapName);
      }
      onMapAdded();
      onClose();
    } catch (error) {
      console.error('Failed to add maps to queue:', error);
    } finally {
      setIsAdding(false);
    }
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
      <div className="bg-gray-900 rounded-lg border border-gray-700 w-full max-w-md max-h-[80vh] flex flex-col">
        <div className="p-4 border-b border-gray-700">
          <h3 className="text-lg font-semibold text-gray-100">Add Maps to Queue</h3>
          <p className="text-sm text-gray-400 mt-1">
            {selectedMaps.size > 0 ? `${selectedMaps.size} map(s) selected` : 'Select maps to add'}
          </p>
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

          <div className="max-h-64 overflow-auto border border-gray-700 rounded">
            {filteredMaps.length === 0 ? (
              <p className="text-gray-500 text-sm text-center py-4">
                {search ? 'No maps match your search' : 'No maps available'}
              </p>
            ) : (
              <div className="space-y-1">
                {filteredMaps.map((map: MapInfo) => (
                  <label
                    key={map.name}
                    className={`flex items-center gap-3 p-3 cursor-pointer transition-colors ${
                      selectedMaps.has(map.name)
                        ? 'bg-orange-600/20 border-l-2 border-orange-500'
                        : 'hover:bg-gray-800'
                    }`}
                  >
                    <input
                      type="checkbox"
                      checked={selectedMaps.has(map.name)}
                      onChange={() => toggleMap(map.name)}
                      className="w-4 h-4 rounded bg-gray-800 border-gray-600 text-orange-600 focus:ring-orange-500"
                    />
                    <div className="flex-1">
                      <div className="font-medium text-gray-200">{map.name}</div>
                      <div className="text-xs text-gray-500">{map.filename}</div>
                    </div>
                  </label>
                ))}
              </div>
            )}
          </div>
        </div>

        <div className="p-4 border-t border-gray-700 flex gap-2 justify-between items-center">
          <button
            type="button"
            onClick={() => setSelectedMaps(new Set())}
            disabled={selectedMaps.size === 0 || isAdding}
            className="px-3 py-2 bg-gray-700 hover:bg-gray-600 rounded text-xs transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            Clear Selection ({selectedMaps.size})
          </button>
          <div className="flex gap-2">
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
              disabled={selectedMaps.size === 0 || isAdding}
              className="px-4 py-2 bg-orange-600 hover:bg-orange-700 rounded text-sm font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {isAdding ? 'Adding...' : `Add ${selectedMaps.size} to Queue`}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
