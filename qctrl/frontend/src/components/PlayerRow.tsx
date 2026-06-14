import type { Player } from '../lib/api';
import { PlayerActions } from './PlayerActions';

interface PlayerRowProps {
  player: Player;
}

export function PlayerRow({ player }: PlayerRowProps) {
  return (
    <div className="p-3 bg-gray-800 rounded-lg border border-gray-700">
      <div className="flex justify-between items-start">
        <div className="flex-1">
          <div className="font-medium">{player.name}</div>
          <div className="text-sm text-gray-400">
            Score: {player.score} | Ping: {player.ping}ms | {player.address}
          </div>
        </div>
        <PlayerActions player={player} />
      </div>
    </div>
  );
}
