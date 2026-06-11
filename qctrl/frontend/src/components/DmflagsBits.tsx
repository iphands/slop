import { useChanges } from '../contexts/ChangesContext';

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
  const { queueChange, getPendingValue, isDirty } = useChanges();

  // Get the pending value from queue, or use current server value
  const pendingDmflags = (getPendingValue('dmflags') as number | undefined) ?? currentValue;

  const toggleBit = (bit: number) => {
    const newValue = pendingDmflags ^ bit;
    queueChange({
      type: 'dmflags',
      pendingValue: newValue,
      description: 'Deathmatch flags',
    });
  };

  return (
    <div className="space-y-3">
      <div className="text-sm text-gray-400">
        Combined: <span className="font-mono">{pendingDmflags}</span>
        {isDirty('dmflags') && (
          <span className="ml-2 text-xs text-orange-400">(queued)</span>
        )}
      </div>
      <div className="grid grid-cols-2 gap-2">
        {FLAGS.map((flag) => {
          const isChecked = Boolean(pendingDmflags & flag.bit);
          
          return (
            <label
              key={flag.bit}
              className={`flex items-center gap-2 p-2 rounded cursor-pointer transition-colors ${
                isChecked
                  ? 'bg-blue-900/50'
                  : 'bg-gray-800 hover:bg-gray-700'
              } ${
                isDirty('dmflags') ? 'ring-2 ring-orange-500/50' : ''
              }`}
            >
              <input
                type="checkbox"
                checked={isChecked}
                onChange={() => toggleBit(flag.bit)}
                className="rounded"
              />
              <span className="text-sm">{flag.name}</span>
            </label>
          );
        })}
      </div>
    </div>
  );
}
