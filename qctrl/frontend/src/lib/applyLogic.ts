/**
 * Shared apply logic for changes queue
 * This is used by both the frontend and e2e tests
 */

export interface Change {
  type: 'dmflags' | 'timelimit' | 'fraglimit' | 'map';
  pendingValue: number | string;
  description: string;
}

export interface ApplyResult {
  commands: string[];
  success: boolean;
  error?: string;
}

/**
 * Build the command list from queued changes
 * Always adds an implicit map restart if no map change is queued
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
