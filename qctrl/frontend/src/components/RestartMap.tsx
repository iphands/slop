import { useMutation } from '@tanstack/react-query';
import { executeRcon } from '../lib/api';
import { isKnownMap } from '../lib/applyLogic';

interface RestartMapProps {
  currentMap?: string;
}

export function RestartMap({ currentMap = 'q2dm1' }: RestartMapProps) {
  const { mutate: execute, isPending, error } = useMutation({
    mutationFn: executeRcon,
  });

  // The default only covers undefined; '' sails through it, and `map ''` makes
  // the server load maps/.bsp and shut down.
  const known = isKnownMap(currentMap);

  const handleRestart = () => {
    if (!known) return;
    execute(`map ${currentMap}`);
  };

  return (
    <div className="space-y-3">
      <button
        type="button"
        onClick={handleRestart}
        disabled={isPending || !known}
        className="w-full py-3 bg-orange-600 hover:bg-orange-700 rounded text-lg font-medium disabled:opacity-50"
      >
        {isPending
          ? 'Restarting...'
          : known
            ? `Restart Current Map (${currentMap})`
            : 'Restart unavailable (map unknown)'}
      </button>
      {error && <div className="text-red-400 text-sm">Failed: {error.message}</div>}
    </div>
  );
}
