import { useQuery } from '@tanstack/react-query';
import { getStatus } from '../lib/api';
import { PlayerRow } from './PlayerRow';
import { useState, useMemo } from 'react';

export function PlayerList() {
  const { data: playerList, isLoading, error, refetch } = useQuery({
    queryKey: ['status'],
    queryFn: getStatus,
    refetchInterval: 10000,
  });

  const [sortField, setSortField] = useState<'score' | 'name'>('score');

  const sortedPlayers = useMemo(() => {
    const players = playerList?.players || [];
    const sorted = [...players];
    if (sortField === 'score') {
      sorted.sort((a, b) => {
        const scoreDiff = b.score - a.score;
        return scoreDiff !== 0 ? scoreDiff : a.name.localeCompare(b.name);
      });
    } else {
      sorted.sort((a, b) => a.name.localeCompare(b.name));
    }
    return sorted;
  }, [playerList?.players, sortField]);

  if (isLoading) {
    return <div className="text-gray-400">Loading players...</div>;
  }

  if (error) {
    return <div className="text-red-400">Failed to load players</div>;
  }

  if (sortedPlayers.length === 0) {
    return <div className="text-gray-400">No players connected</div>;
  }

  return (
    <div className="space-y-3">
      <div className="flex justify-between items-center mb-4">
        <h3 className="text-lg font-semibold">
          Players ({sortedPlayers.length})
        </h3>
        <div className="flex gap-2">
          <select
            value={sortField}
            onChange={(e) => setSortField(e.target.value as 'score' | 'name')}
            className="px-3 py-1 bg-gray-700 hover:bg-gray-600 rounded text-sm"
          >
            <option value="score">Sort: Score</option>
            <option value="name">Sort: Name</option>
          </select>
          <button
            type="button"
            onClick={() => refetch()}
            className="px-3 py-1 bg-gray-700 hover:bg-gray-600 rounded text-sm"
          >
            Refresh
          </button>
        </div>
      </div>
      {sortedPlayers.map((player) => (
        <PlayerRow key={player.clientNum} player={player} />
      ))}
    </div>
  );
}
