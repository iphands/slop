export interface MapInfo {
  name: string;
  filename: string;
  size: number;
  modified: number;
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
  const res = await fetch('/health');
  if (!res.ok) {
    throw new Error('Failed to fetch health');
  }
  return res.json();
}

export async function getConfig(): Promise<Config> {
  const res = await fetch('/config');
  if (!res.ok) {
    throw new Error('Failed to fetch config');
  }
  return res.json();
}

export async function getMaps(): Promise<{ maps: MapInfo[] }> {
  const res = await fetch('/maps');
  if (!res.ok) {
    throw new Error('Failed to fetch maps');
  }
  return res.json();
}

export async function getStatus(): Promise<PlayerList> {
  const res = await fetch('/status');
  if (!res.ok) {
    throw new Error('Failed to fetch status');
  }
  return res.json();
}

export async function executeRcon(command: string): Promise<string> {
  const res = await fetch('/rcon/execute', {
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
