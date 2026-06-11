import { useChanges } from '../contexts/ChangesContext';
import { useMutation } from '@tanstack/react-query';
import { executeRcon } from '../lib/api';

export function ChangesQueueUI() {
  const { state, clearQueue, applyChanges } = useChanges();
  const { mutate: execute, isPending } = useMutation({
    mutationFn: executeRcon,
  });

  const handleApply = () => {
    const commands: string[] = [];

    // Build commands based on pending changes
    state.changes.forEach((change) => {
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
        case 'map':
          // Map change will be added last
          break;
      }
    });

    // Add map restart last if there's a map change or if there are other changes
    const mapChange = state.changes.find((c) => c.type === 'map');
    if (mapChange) {
      commands.push(`map ${mapChange.pendingValue}`);
    } else if (state.changes.some((c) => c.type === 'dmflags')) {
      // If only dmflags changed, still need to restart to apply
      // For now, we'll skip the map restart - user can use RestartMap button
      // TODO: Get actual current map from server status
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

  return (
    <div className="fixed bottom-0 left-0 right-0 bg-gray-900 border-t border-orange-500/50 p-4 shadow-lg z-50">
      <div className="max-w-6xl mx-auto">
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-3">
            <h3 className="text-lg font-semibold text-orange-400">
              Pending Changes ({state.changes.length})
            </h3>
            <span className="text-sm text-gray-400">
              Requires map restart to apply
            </span>
          </div>
          <div className="flex gap-2">
            <button
              type="button"
              onClick={handleCancel}
              disabled={isPending}
              className="px-4 py-2 bg-gray-700 hover:bg-gray-600 rounded text-sm disabled:opacity-50"
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={handleApply}
              disabled={isPending}
              className="px-4 py-2 bg-orange-600 hover:bg-orange-700 rounded text-sm font-medium disabled:opacity-50"
            >
              {isPending ? 'Applying...' : 'Apply Changes'}
            </button>
          </div>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
          {state.changes.map((change) => (
            <div
              key={change.id}
              className="p-3 bg-gray-800 rounded border border-orange-500/30"
            >
              <div className="flex items-center justify-between mb-1">
                <span className="text-sm font-medium text-orange-300">
                  {change.description}
                </span>
                <button
                  type="button"
                  onClick={() => clearQueue()}
                  className="text-xs text-gray-400 hover:text-gray-300"
                  title="Remove this change"
                >
                  ×
                </button>
              </div>
              <div className="text-xs text-gray-400">
                → <span className="text-orange-400 font-mono">{change.pendingValue}</span>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
