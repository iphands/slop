import { useMutation } from '@tanstack/react-query';
import { useChanges } from '../contexts/ChangesContext';
import { executeRcon } from '../lib/api';
import type { MapInfo } from '../lib/api';

interface MapDialogProps {
  map: MapInfo | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function MapDialog({ map, open, onOpenChange }: MapDialogProps) {
  const { queueChange } = useChanges();
  const { isPending, error } = useMutation({
    mutationFn: executeRcon,
  });

  const handleChange = () => {
    if (map) {
      // Queue the map change instead of sending immediately
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
          Switching to <strong className="text-white">{map.name}</strong> will be queued and applied when you click "Apply Changes".
        </p>
        {error && <div className="text-red-400 text-sm mb-4">{error.message}</div>}
        <div className="flex gap-3">
          <button
            type="button"
            onClick={() => onOpenChange(false)}
            disabled={isPending}
            className="flex-1 py-2 bg-gray-700 hover:bg-gray-600 rounded disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={handleChange}
            disabled={isPending}
            className="flex-1 py-2 bg-orange-600 hover:bg-orange-700 rounded disabled:opacity-50"
          >
            {isPending ? 'Queuing...' : 'Queue Change'}
          </button>
        </div>
      </div>
    </div>
  );
}
