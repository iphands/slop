import { useState } from 'react';
import { useChanges } from '../contexts/ChangesContext';
import { useMutation } from '@tanstack/react-query';
import { executeRcon } from '../lib/api';

// Hardcoded current map - TODO: Get from server status endpoint
const CURRENT_MAP = 'q2dm1';

export function ChangesQueueUI() {
  const { state, clearQueue, applyChanges } = useChanges();
  const { mutate: execute, isPending } = useMutation({
    mutationFn: executeRcon,
  });
  const [showAll, setShowAll] = useState(false);

  const handleApply = () => {
    const commands: string[] = [];

    // Build commands based on pending changes (except map)
    state.changes.forEach((change) => {
      if (change.type === 'map') return; // Skip map for now, add it last
      
      switch (change.type) {
        case 'dmflags':
          commands.push(`dmflags ${change.pendingValue}`);
          break;
        case 'timelimit':
          commands.push(`timelimit ${change.pendingValue}`);
          break;
        case 'fraglimit':
          commands.push(`fraglimit ${change.pendingValue}`);
          break;
      }
    });

    // Always add map restart last
    const mapChange = state.changes.find((c) => c.type === 'map');
    if (mapChange) {
      // Use the queued map change
      commands.push(`map ${mapChange.pendingValue}`);
    } else {
      // No map change queued, but we still need to restart to apply other changes
      commands.push(`map ${CURRENT_MAP}`);
    }

    // Send all commands
    commands.forEach((cmd) => {
      execute(cmd);
    });

    // Mark as applied
    if (commands.length > 0) {
      applyChanges();
    }
  };

  const handleCancel = () => {
    clearQueue();
  };

  if (state.changes.length === 0) {
    return null;
  }

  // Show only first 3 items if showAll is false
  const visibleChanges = showAll ? state.changes : state.changes.slice(0, 3);
  const hiddenCount = state.changes.length - visibleChanges.length;

  return (
    <div className="mb-4 p-3 bg-orange-900/20 border border-orange-500/50 rounded">
      <div className="flex items-center justify-between mb-2">
        <div className="flex items-center gap-2">
          <h3 className="text-sm font-semibold text-orange-400">
            Pending Changes ({state.changes.length})
          </h3>
          <span className="text-xs text-gray-400">
            Requires map restart to apply
          </span>
        </div>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={handleCancel}
            disabled={isPending}
            className="px-3 py-1 bg-gray-700 hover:bg-gray-600 rounded text-xs disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={handleApply}
            disabled={isPending}
            className="px-3 py-1 bg-orange-600 hover:bg-orange-700 rounded text-xs font-medium disabled:opacity-50"
          >
            {isPending ? 'Applying...' : 'Apply'}
          </button>
        </div>
      </div>

      <div className="space-y-2">
        {visibleChanges.map((change) => (
          <div
            key={change.id}
            className="p-2 bg-gray-800 rounded border border-orange-500/30 flex items-center justify-between"
          >
            <div className="flex items-center gap-3">
              <span className="text-sm font-medium text-orange-300">
                {change.description}
              </span>
              <span className="text-xs text-gray-400">
                → <span className="text-orange-400 font-mono">{change.pendingValue}</span>
              </span>
            </div>
            <button
              type="button"
              onClick={() => clearQueue()}
              className="text-xs text-gray-400 hover:text-gray-300"
              title="Remove this change"
            >
              ×
            </button>
          </div>
        ))}
      </div>

      {hiddenCount > 0 && (
        <button
          type="button"
          onClick={() => setShowAll(!showAll)}
          className="mt-2 text-xs text-orange-400 hover:text-orange-300"
        >
          {showAll ? `Show less (-${hiddenCount})` : `Show all (+${hiddenCount})`}
        </button>
      )}
    </div>
  );
}
