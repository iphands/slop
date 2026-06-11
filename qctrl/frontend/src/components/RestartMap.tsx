import { useMutation } from '@tanstack/react-query';
import { executeRcon } from '../lib/api';

interface RestartMapProps {
  currentMap?: string;
}

export function RestartMap({ currentMap = 'q2dm1' }: RestartMapProps) {
  const { mutate: execute, isPending, error } = useMutation({
    mutationFn: executeRcon,
  });

  const handleRestart = () => {
    execute(`map ${currentMap}`);
  };

  return (
    <div className="space-y-3">
      <button
        type="button"
        onClick={handleRestart}
        disabled={isPending}
        className="w-full py-3 bg-orange-600 hover:bg-orange-700 rounded text-lg font-medium disabled:opacity-50"
      >
        {isPending ? 'Restarting...' : `Restart Current Map (${currentMap})`}
      </button>
      {error && <div className="text-red-400 text-sm">Failed: {error.message}</div>}
    </div>
  );
}
