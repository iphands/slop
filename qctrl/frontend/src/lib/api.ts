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

export interface PlayerList {
  players: Player[];
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

export async function getStatus(): Promise<PlayerList> {
  const res = await fetch('/api/status');
  if (!res.ok) {
    throw new Error('Failed to fetch status');
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
