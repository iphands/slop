/**
 * Shared apply logic for changes queue
 * This is used by both the frontend and e2e tests
 */

export interface Change {
  type: 'dmflags' | 'timelimit' | 'fraglimit' | 'map' | 'rotationMode';
  pendingValue: number | string;
  description: string;
}

export interface ApplyResult {
  commands: string[];
  success: boolean;
  error?: string;
}

/**
 * Map names that mean "we don't actually know what's running".
 * Sending `map <one of these>` makes the server load maps/.bsp and die, so every
 * guard in the UI imports this set rather than re-spelling the sentinel.
 */
const UNKNOWN_MAPS = new Set(['', 'unknown', 'Unknown']);

export function isKnownMap(map: string | undefined | null): boolean {
  return map !== undefined && map !== null && !UNKNOWN_MAPS.has(map.trim());
}

/**
 * Build the command list from queued changes
 * Adds an implicit map restart if no map change is queued, but only when the
 * current map is known
 */
export function buildApplyCommands(changes: Change[], currentMap: string): string[] {
  const commands: string[] = [];

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

  // Add the map restart last
  const mapChange = changes.find((c) => c.type === 'map');
  if (mapChange && String(mapChange.pendingValue).trim() !== '') {
    // Use the queued map change
    commands.push(`map ${mapChange.pendingValue}`);
  } else if (isKnownMap(currentMap)) {
    // No map change queued, but we still need to restart to apply other changes
    commands.push(`map ${currentMap}`);
  }
  // Otherwise send the cvar changes without a restart: they take effect on the
  // next map load. `map <empty/unknown>` would kill the server.

  return commands;
}

/**
 * Execute the apply flow: build commands, send them, wait for completion
 */
export async function applyChanges(
  changes: Change[],
  currentMap: string,
  executeCommand: (cmd: string) => Promise<string>
): Promise<ApplyResult> {
  try {
    const commands = buildApplyCommands(changes, currentMap);
    
    // Send all commands and wait for them to complete
    const promises = commands.map((cmd) => executeCommand(cmd));
    await Promise.all(promises);
    
    return {
      commands,
      success: true,
    };
  } catch (error) {
    return {
      commands: [],
      success: false,
      error: error instanceof Error ? error.message : 'Unknown error',
    };
  }
}
