import { useState } from 'react';
import { useChanges } from '../contexts/ChangesContext';
import { useQuery, useMutation } from '@tanstack/react-query';
import { executeRcon, getStatus } from '../lib/api';
import { applyChanges } from '../lib/applyLogic';

export function ChangesQueueUI() {
  const { state, clearQueue, applyChanges: clearApply } = useChanges();
  const { data: status } = useQuery({
    queryKey: ['status'],
    queryFn: getStatus,
    refetchInterval: 2000,
  });
  const { mutateAsync: execute, isPending } = useMutation({
    mutationFn: executeRcon,
  });
  const [showAll, setShowAll] = useState(false);

  const handleApply = async () => {
    const currentMap = status?.map || 'unknown';
    const result = await applyChanges(
      state.changes,
      currentMap,
      execute
    );

    if (result.success) {
      // Only clear queue after all commands have been sent
      clearApply();
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
