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

export async function getMaps(): Promise<MapInfo[]> {
  const res = await fetch('/maps');
  if (!res.ok) {
    throw new Error('Failed to fetch maps');
  }
  const data = await res.json();
  return data.maps || [];
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
