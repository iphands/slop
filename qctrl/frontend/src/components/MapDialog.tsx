import { useChanges } from '../contexts/ChangesContext';
import type { MapInfo } from '../lib/api';

interface MapDialogProps {
  map: MapInfo | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function MapDialog({ map, open, onOpenChange }: MapDialogProps) {
  const { queueChange } = useChanges();

  const handleQueue = () => {
    if (map) {
      // Queue the map change directly
      queueChange({
        type: 'map',
        pendingValue: map.name,
        description: 'Map change',
      });
      onOpenChange(false);
    }
  };

  if (!open || !map) return null;

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
      <div className="bg-gray-800 rounded-lg max-w-md w-full p-6">
        <h2 className="text-xl font-semibold mb-4">Queue Map Change?</h2>
        <p className="text-gray-300 mb-6">
          <strong className="text-white">{map.name}</strong> will be added to the pending changes queue and applied when you click "Apply Changes" at the top.
        </p>
        <div className="flex gap-3">
          <button
            type="button"
            onClick={() => onOpenChange(false)}
            className="flex-1 py-2 bg-gray-700 hover:bg-gray-600 rounded"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={handleQueue}
            className="flex-1 py-2 bg-orange-600 hover:bg-orange-700 rounded font-medium"
          >
            Queue Change
          </button>
        </div>
      </div>
    </div>
  );
}
