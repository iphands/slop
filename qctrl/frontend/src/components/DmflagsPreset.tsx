import { useChanges } from '../contexts/ChangesContext';

const PRESETS = [
  { name: "Standard", value: 16, description: "Instant powerups" },
  { name: "Weapons Stay", value: 20, description: "Weapons remain after pickup" },
  { name: "No Armor", value: 2064, description: "No armor, instant powerups" },
  { name: "Full Game", value: 17424, description: "Current server setting" },
];

interface DmflagsPresetProps {
  currentValue: number;
}

export function DmflagsPreset({ currentValue }: DmflagsPresetProps) {
  const { queueChange, getPendingValue, isDirty } = useChanges();

  // Get the pending value from queue, or use current server value
  const pendingDmflags = (getPendingValue('dmflags') as number | undefined) ?? currentValue;

  const handlePresetClick = (value: number) => {
    queueChange({
      type: 'dmflags',
      pendingValue: value,
      description: 'Deathmatch flags',
    });
  };

  return (
    <div className="space-y-4">
      <div className="text-sm text-gray-400">
        Current: <span className="font-mono">{pendingDmflags}</span>
        {isDirty('dmflags') && (
          <span className="ml-2 text-xs text-orange-400">(queued)</span>
        )}
      </div>
      <div className="grid grid-cols-2 gap-3">
        {PRESETS.map((preset) => (
          <button
            key={preset.name}
            type="button"
            onClick={() => handlePresetClick(preset.value)}
            className={`p-3 rounded border text-left transition-colors ${
              pendingDmflags === preset.value
                ? 'bg-blue-900 border-blue-500'
                : 'bg-gray-800 border-gray-700 hover:border-gray-600'
            } ${
              isDirty('dmflags') && pendingDmflags === preset.value ? 'ring-2 ring-orange-500/50' : ''
            }`}
          >
            <div className="font-medium">{preset.name}</div>
            <div className="text-sm text-gray-400">{preset.description}</div>
            <div className="text-xs font-mono mt-1">{preset.value}</div>
          </button>
        ))}
      </div>
    </div>
  );
}
