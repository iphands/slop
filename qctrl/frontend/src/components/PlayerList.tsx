import { useQuery } from '@tanstack/react-query';
import { getStatus } from '../lib/api';
import { PlayerRow } from './PlayerRow';

export function PlayerList() {
  const { data: playerList, isLoading, error, refetch } = useQuery({
    queryKey: ['status'],
    queryFn: getStatus,
    refetchInterval: 10000,
  });

  if (isLoading) {
    return <div className="text-gray-400">Loading players...</div>;
  }

  if (error) {
    return <div className="text-red-400">Failed to load players</div>;
  }

  const players = playerList?.players || [];

  if (players.length === 0) {
    return <div className="text-gray-400">No players connected</div>;
  }

  return (
    <div className="space-y-3">
      <div className="flex justify-between items-center mb-4">
        <h3 className="text-lg font-semibold">
          Players ({players.length})
        </h3>
        <button
          type="button"
          onClick={() => refetch()}
          className="px-3 py-1 bg-gray-700 hover:bg-gray-600 rounded text-sm"
        >
          Refresh
        </button>
      </div>
      {players.map((player) => (
        <PlayerRow key={player.clientNum} player={player} />
      ))}
    </div>
  );
}
