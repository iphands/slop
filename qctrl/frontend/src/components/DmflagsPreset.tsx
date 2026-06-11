import { useState } from 'react';
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
  const [pendingValue, setPendingValue] = useState(currentValue);
  const { mutate: execute, isPending, error } = useMutation({
    mutationFn: executeRcon,
  });

  const handlePresetClick = (value: number) => {
    setPendingValue(value);
    execute(`dmflags ${value}`);
  };

  return (
    <div className="space-y-4">
      <div className="text-sm text-gray-400">
        Current: <span className="font-mono">{pendingValue}</span>
        {pendingValue !== currentValue && (
          <span className="ml-2 text-xs text-orange-400">(unsaved)</span>
        )}
      </div>
      <div className="grid grid-cols-2 gap-3">
        {PRESETS.map((preset) => {
          const isDirty = preset.value !== currentValue;
          return (
            <button
              key={preset.name}
              type="button"
              onClick={() => handlePresetClick(preset.value)}
              disabled={isPending}
              className={`p-3 rounded border text-left transition-colors ${
                pendingValue === preset.value
                  ? 'bg-blue-900 border-blue-500'
                  : 'bg-gray-800 border-gray-700 hover:border-gray-600'
              } ${
                isDirty && pendingValue === preset.value ? 'ring-2 ring-orange-500/50' : ''
              }`}
            >
              <div className="font-medium">{preset.name}</div>
              <div className="text-sm text-gray-400">{preset.description}</div>
              <div className="text-xs font-mono mt-1">{preset.value}</div>
            </button>
          );
        })}
      </div>
      {error && <div className="text-red-400 text-sm">Failed: {error.message}</div>}
      {isPending && <div className="text-blue-400 text-sm">Sending command...</div>}
    </div>
  );
}
