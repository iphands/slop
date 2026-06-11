import { useMutation } from '@tanstack/react-query';
import { executeRcon } from '../lib/api';
import type { MapInfo } from '../lib/api';

interface MapDialogProps {
  map: MapInfo | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function MapDialog({ map, open, onOpenChange }: MapDialogProps) {
  const { mutate: execute, isPending, error } = useMutation({
    mutationFn: executeRcon,
  });

  const handleChange = () => {
    if (map) {
      execute(`map ${map.name}`, {
        onSuccess: () => onOpenChange(false),
      });
    }
  };

  if (!open || !map) return null;

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
      <div className="bg-gray-800 rounded-lg max-w-md w-full p-6">
        <h2 className="text-xl font-semibold mb-4">Change Map?</h2>
        <p className="text-gray-300 mb-6">
          Switching to <strong className="text-white">{map.name}</strong> will restart the server.
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
            className="flex-1 py-2 bg-blue-600 hover:bg-blue-700 rounded disabled:opacity-50"
          >
            {isPending ? 'Changing...' : 'Change Map'}
          </button>
        </div>
      </div>
    </div>
  );
}
