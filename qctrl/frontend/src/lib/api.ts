export interface MapInfo {
  name: string;
  filename: string;
  size: number;
  modified: number;
  source: 'Directory' | { Pak: string };
}

export interface Config {
  server: {
    host: string;
    port: number;
    rcon_password: string;
  };
  paths: {
    server_cfg: string;
    baseq2: string;
  };
}

export interface HealthResponse {
  status: string;
}

export interface Player {
  clientNum: number;
  score: number;
  address: string;
  name: string;
  ping: number;
}

/**
 * Whether the backend actually knows when the current map started.
 *
 * A Quake 2 server does not publish elapsed map time — there is no such field in
 * rcon `status`, in the serverinfo string, or anywhere else. qctrl infers the map
 * start by watching for the map name to change. If it was not running when the
 * current map started (or the server has since restarted), that start is
 * unknowable, and `anchor` says so instead of guessing.
 */
export type ClockAnchor = 'exact' | 'unknown';

export type ClockQuality =
  /** Polling is healthy. */
  | 'live'
  /** Polling is failing; elapsed keeps ticking but the map may have changed unseen. */
  | 'degraded'
  /** Past the timelimit with no map change — our model disagrees with the server. */
  | 'overdue';

export type ClockSource = 'observed_edge' | 'own_map_command' | 'none';

export interface MapClock {
  anchor: ClockAnchor;
  /** Null if and only if `anchor === 'unknown'`. There is no honest number to show. */
  elapsed_seconds: number | null;
  quality: ClockQuality;
  source: ClockSource;
  server_uptime_seconds: number | null;
  last_poll_age_seconds: number;
}

export interface StatusResponse {
  map: string | null;
  players: Player[];
  // Server settings - may be undefined until backend exposes them
  dmflags?: number;
  timelimit?: number;
  fraglimit?: number;
  maxclients?: number;
  /** Optional so existing test fixtures that predate the clock still typecheck. */
  clock?: MapClock;
  server_online?: boolean;
}

export interface QueueStatus {
  maps: string[];
  mode: 'Sequential' | 'Random';
  current_map: string | null;
  enabled: boolean;
}

export interface QueueResponse {
  success: boolean;
  message: string;
  queue_size: number;
}

/** Player as the API sends it: snake_case, unlike the camelCase `Player` we expose. */
interface ApiPlayer {
  client_num: number;
  score: number;
  address: string;
  name: string;
  ping: number;
}

export async function getStatus(): Promise<StatusResponse> {
  const res = await fetch('/api/status');
  if (!res.ok) {
    throw new Error(`Failed to fetch status: ${res.status} ${res.statusText}`);
  }
  const data: Omit<StatusResponse, 'players'> & { players?: ApiPlayer[] } = await res.json();
  return {
    ...data,
    players: data.players?.map((p) => ({
      clientNum: p.client_num,
      score: p.score,
      address: p.address,
      name: p.name,
      ping: p.ping,
    })) ?? [],
  };
}

export async function getHealth(): Promise<HealthResponse> {
  const res = await fetch('/api/health');
  if (!res.ok) {
    throw new Error('Failed to fetch health');
  }
  return res.json();
}

export async function getConfig(): Promise<Config> {
  const res = await fetch('/api/config');
  if (!res.ok) {
    throw new Error('Failed to fetch config');
  }
  return res.json();
}

export async function getMaps(): Promise<{ maps: MapInfo[] }> {
  const res = await fetch('/api/maps');
  if (!res.ok) {
    throw new Error('Failed to fetch maps');
  }
  return res.json();
}

export async function getFavorites(): Promise<{ favorites: string[] }> {
  const res = await fetch('/api/favorites');
  if (!res.ok) {
    throw new Error('Failed to fetch favorites');
  }
  return res.json();
}

export async function addFavorite(mapName: string): Promise<{ success: boolean }> {
  const res = await fetch('/api/favorites', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ map_name: mapName }),
  });
  if (!res.ok) {
    throw new Error('Failed to add favorite');
  }
  return res.json();
}

export async function removeFavorite(mapName: string): Promise<{ success: boolean }> {
  const res = await fetch(`/api/favorites/${encodeURIComponent(mapName)}`, {
    method: 'DELETE',
  });
  if (!res.ok) {
    throw new Error('Failed to remove favorite');
  }
  return res.json();
}

export async function executeRcon(command: string): Promise<string> {
  const res = await fetch('/api/rcon/execute', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ command }),
  });
  if (!res.ok) {
    throw new Error('Failed to execute RCON command');
  }
  return res.text();
}

// Rotation queue API
export async function getRotationQueue(): Promise<QueueStatus> {
  const res = await fetch('/api/rotation');
  if (!res.ok) {
    throw new Error('Failed to fetch rotation queue');
  }
  return res.json();
}

export async function addMapToQueue(mapName: string): Promise<QueueResponse> {
  const res = await fetch('/api/rotation', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ map_name: mapName }),
  });
  if (!res.ok) {
    throw new Error('Failed to add map to queue');
  }
  return res.json();
}

export async function updateRotationQueue(maps: string[], mode: 'Sequential' | 'Random'): Promise<QueueResponse> {
  const res = await fetch('/api/rotation', {
    method: 'PUT',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ maps, mode }),
  });
  if (!res.ok) {
    throw new Error('Failed to update rotation queue');
  }
  return res.json();
}

export async function removeMapFromQueue(mapName: string): Promise<QueueResponse> {
  const res = await fetch(`/api/rotation/${encodeURIComponent(mapName)}`, {
    method: 'DELETE',
  });
  if (!res.ok) {
    throw new Error('Failed to remove map from queue');
  }
  return res.json();
}

export async function toggleRotation(): Promise<{ success: boolean; enabled: boolean; message: string }> {
  const res = await fetch('/api/rotation/toggle', {
    method: 'POST',
  });
  if (!res.ok) {
    throw new Error('Failed to toggle rotation');
  }
  return res.json();
}
