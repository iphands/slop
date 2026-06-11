import { useMutation } from '@tanstack/react-query';
import { executeRcon } from '../lib/api';

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
  const { mutate: execute, isPending, error } = useMutation({
    mutationFn: executeRcon,
  });

  const handlePresetClick = (value: number) => {
    execute(`dmflags ${value}`);
  };

  return (
    <div className="space-y-4">
      <div className="text-sm text-gray-400">
        Current: <span className="font-mono">{currentValue}</span>
      </div>
      <div className="grid grid-cols-2 gap-3">
        {PRESETS.map((preset) => (
          <button
            key={preset.name}
            type="button"
            onClick={() => handlePresetClick(preset.value)}
            disabled={isPending}
            className={`p-3 rounded border text-left transition-colors ${
              currentValue === preset.value
                ? 'bg-blue-900 border-blue-500'
                : 'bg-gray-800 border-gray-700 hover:border-gray-600'
            }`}
          >
            <div className="font-medium">{preset.name}</div>
            <div className="text-sm text-gray-400">{preset.description}</div>
            <div className="text-xs font-mono mt-1">{preset.value}</div>
          </button>
        ))}
      </div>
      {error && <div className="text-red-400 text-sm">Failed: {error.message}</div>}
      {isPending && <div className="text-blue-400 text-sm">Sending command...</div>}
    </div>
  );
}
