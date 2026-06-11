import { useMutation } from '@tanstack/react-query';
import { executeRcon } from '../lib/api';

const FLAGS = [
  { bit: 1, name: 'No Health' },
  { bit: 2, name: 'No Powerups' },
  { bit: 4, name: 'Weapons Stay' },
  { bit: 8, name: 'No Fall Damage' },
  { bit: 16, name: 'Instant Powerups' },
  { bit: 32, name: 'Same Map' },
  { bit: 64, name: 'Teams by Skin' },
  { bit: 128, name: 'Teams by Model' },
  { bit: 256, name: 'No Friendly Fire' },
  { bit: 512, name: 'Spawn Farthest' },
  { bit: 1024, name: 'Force Respawn' },
  { bit: 2048, name: 'No Armor' },
  { bit: 4096, name: 'Allow Exit' },
  { bit: 8192, name: 'Infinite Ammo' },
  { bit: 16384, name: 'Quad Drop' },
  { bit: 32768, name: 'Fixed FOV' },
];

interface DmflagsBitsProps {
  currentValue: number;
}

export function DmflagsBits({ currentValue }: DmflagsBitsProps) {
  const { mutate: execute, isPending, error } = useMutation({
    mutationFn: executeRcon,
  });

  const toggleBit = (bit: number) => {
    const newValue = currentValue ^ bit;
    execute(`dmflags ${newValue}`);
  };

  return (
    <div className="space-y-3">
      <div className="text-sm text-gray-400">
        Combined: <span className="font-mono">{currentValue}</span>
      </div>
      <div className="grid grid-cols-2 gap-2">
        {FLAGS.map((flag) => (
          <label
            key={flag.bit}
            className={`flex items-center gap-2 p-2 rounded cursor-pointer transition-colors ${
              currentValue & flag.bit
                ? 'bg-blue-900/50'
                : 'bg-gray-800 hover:bg-gray-700'
            }`}
          >
            <input
              type="checkbox"
              checked={Boolean(currentValue & flag.bit)}
              onChange={() => toggleBit(flag.bit)}
              disabled={isPending}
              className="rounded"
            />
            <span className="text-sm">{flag.name}</span>
          </label>
        ))}
      </div>
      {error && <div className="text-red-400 text-sm">Failed: {error.message}</div>}
      {isPending && <div className="text-blue-400 text-sm">Sending command...</div>}
    </div>
  );
}
