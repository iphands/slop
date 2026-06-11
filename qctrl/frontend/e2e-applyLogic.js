/**
 * Shared apply logic for e2e tests
 * Mirrors the TypeScript logic from applyLogic.ts
 */

/**
 * Build the command list from queued changes
 * Always adds an implicit map restart if no map change is queued
 */
export function buildApplyCommands(changes, currentMap) {
  const commands = [];

  // Build commands based on pending changes (except map)
  changes.forEach((change) => {
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
  const mapChange = changes.find((c) => c.type === 'map');
  if (mapChange) {
    // Use the queued map change
    commands.push(`map ${mapChange.pendingValue}`);
  } else {
    // No map change queued, but we still need to restart to apply other changes
    commands.push(`map ${currentMap}`);
  }

  return commands;
}
