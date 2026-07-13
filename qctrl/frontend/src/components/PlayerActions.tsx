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

  /**
   * The backend reports -1 when it cannot say which client number belongs to this
   * player — either the slow rcon identity poll hasn't seen them yet, or two
   * players share a name and matching them would be a guess. `clientkick -1`
   * would act on the wrong player, so refuse rather than guess.
   */
  const identityUnknown = player.clientNum < 0;

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
        disabled={kicking || banning || identityUnknown}
        title={
          identityUnknown
            ? 'Client number not resolved yet (or this name is ambiguous) — banning could hit the wrong player.'
            : undefined
        }
        className="px-3 py-1 bg-orange-600 hover:bg-orange-700 rounded text-sm disabled:opacity-50"
      >
        {banning ? 'Banning...' : 'Ban'}
      </button>
    </div>
  );
}
