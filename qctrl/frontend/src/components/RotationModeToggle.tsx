import { useState, useEffect } from 'react';
import { useChanges } from '../contexts/ChangesContext';
import { getRotationQueue, updateRotationQueue } from '../lib/api';

interface RotationModeToggleProps {
  currentMode?: 'Sequential' | 'Random';
  onModeChange?: (mode: 'Sequential' | 'Random') => void;
}

export function RotationModeToggle({ currentMode = 'Sequential', onModeChange }: RotationModeToggleProps) {
  const { queueChange, getPendingValue, isDirty } = useChanges();
  const [localMode, setLocalMode] = useState<'Sequential' | 'Random'>(currentMode);

  // Sync local state when currentMode prop changes
  useEffect(() => {
    setLocalMode(currentMode);
  }, [currentMode]);

  const handleModeChange = (newMode: 'Sequential' | 'Random') => {
    if (newMode === localMode) return;
    
    setLocalMode(newMode);
    queueChange({
      type: 'rotationMode',
      pendingValue: newMode,
      description: 'Rotation mode',
    });

    getRotationQueue()
      .then((queue) => {
        return updateRotationQueue(queue.maps, newMode);
      })
      .then(() => {
        onModeChange?.(newMode);
      })
      .catch(() => {});
  };

  const pending = getPendingValue('rotationMode');
  const displayMode = pending ? (pending as 'Sequential' | 'Random') : localMode;
  const isSequential = displayMode === 'Sequential';

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <label className="text-sm font-medium">Rotation Mode</label>
        {isDirty('rotationMode') && (
          <span className="text-xs text-orange-400">(queued)</span>
        )}
      </div>

      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={() => handleModeChange('Sequential')}
          className={`flex-1 px-4 py-2 rounded border transition-colors ${
            isSequential
              ? 'bg-orange-600 border-orange-500 text-white'
              : 'bg-gray-800 border-gray-700 hover:border-gray-600 text-gray-200'
          } ${
            isDirty('rotationMode') && isSequential ? 'ring-2 ring-orange-500/50' : ''
          }`}
        >
          Sequential
        </button>

        <button
          type="button"
          onClick={() => handleModeChange('Random')}
          className={`flex-1 px-4 py-2 rounded border transition-colors ${
            !isSequential
              ? 'bg-orange-600 border-orange-500 text-white'
              : 'bg-gray-800 border-gray-700 hover:border-gray-600 text-gray-200'
          } ${
            isDirty('rotationMode') && !isSequential ? 'ring-2 ring-orange-500/50' : ''
          }`}
        >
          Random
        </button>
      </div>

      <div className="text-sm text-gray-400">
        Current: <span className="font-medium">{displayMode}</span>
      </div>
    </div>
  );
}
