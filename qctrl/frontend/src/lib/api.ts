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
 * A Quake 2 server does not publish elapsed map time on any channel qctrl speaks — not
 * rcon `status`, not the serverinfo string. So by default qctrl *infers* the map start by
 * watching for the map name to change, and if it wasn't running when the current map
 * started, that start is unknowable and `anchor` says so rather than guessing.
 *
 * A qbots beacon removes the guesswork entirely — see `ClockSource.server_frame`.
 */
export type ClockAnchor = 'exact' | 'unknown';

export type ClockQuality =
  /** Polling is healthy. */
  | 'live'
  /** Polling is failing; elapsed keeps ticking but the map may have changed unseen. */
  | 'degraded'
  /** Past the timelimit with no map change — our model disagrees with the server. */
  | 'overdue';

/**
 * How the map clock was anchored.
 *
 * `observed_edge` and `own_map_command` are *inferences* — they anchor at the moment we
 * noticed something. `server_frame` is a *measurement*: the Q2 server zeroes its frame counter
 * on every map spawn and ticks it at 10 Hz, so `serverframe / 10` is the exact age of the map.
 * qctrl can't see that number, but a connected client can, and a qbots fleet relays it
 * (backend Plan 13). It outranks the other two.
 */
export type ClockSource =
  | 'observed_edge'
  | 'own_map_command'
  | 'server_frame'
  | 'none';

export interface MapClock {
  anchor: ClockAnchor;
  /** Null if and only if `anchor === 'unknown'`. There is no honest number to show. */
  elapsed_seconds: number | null;
  quality: ClockQuality;
  source: ClockSource;
  last_poll_age_seconds: number;
  /** The server's own frame counter, when a qbots beacon is feeding us. Diagnostic. */
  server_frame?: number | null;
  /** Age of the last beacon. Grows once the bot fleet stops; null if there never was one. */
  beacon_age_seconds?: number | null;
  /** Bots currently feeding the beacon. */
  beacon_bots?: number | null;
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
