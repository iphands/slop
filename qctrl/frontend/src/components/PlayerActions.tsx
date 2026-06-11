import { useMutation } from '@tanstack/react-query';
import { executeRcon } from '../lib/api';
import type { Player } from '../lib/api';

interface PlayerActionsProps {
  player: Player;
  onAction?: () => void;
}

export function PlayerActions({ player, onAction }: PlayerActionsProps) {
  const { mutate: kick, isPending: kicking } = useMutation({
    mutationFn: executeRcon,
    onSuccess: onAction,
  });

  const { mutate: ban, isPending: banning } = useMutation({
    mutationFn: executeRcon,
    onSuccess: onAction,
  });

  const handleKick = () => {
    kick(`kick ${player.name}`);
  };

  const handleBan = () => {
    ban(`clientkick ${player.clientNum}`);
  };

  return (
    <div className="flex gap-2">
      <button
        type="button"
        onClick={handleKick}
        disabled={kicking || banning}
        className="px-3 py-1 bg-red-600 hover:bg-red-700 rounded text-sm disabled:opacity-50"
      >
        {kicking ? 'Kicking...' : 'Kick'}
      </button>
      <button
        type="button"
        onClick={handleBan}
        disabled={kicking || banning}
        className="px-3 py-1 bg-orange-600 hover:bg-orange-700 rounded text-sm disabled:opacity-50"
      >
        {banning ? 'Banning...' : 'Ban'}
      </button>
    </div>
  );
}
