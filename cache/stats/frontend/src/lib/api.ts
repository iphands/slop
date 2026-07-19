// TS mirrors of the Rust payload in crates/stats/src/snapshot.rs.
//
// Kept hand-written rather than generated: the schema is small, and a test on
// the Rust side asserts fixture.json still deserializes into Payload, so drift
// is caught there rather than by codegen here.

/** `null` means "no requests in this window" — render an em dash, never 0%. */
export type Ratio = number | null;

export interface Kpi {
  reqs: number;
  bytes_served: number;
  bytes_upstream: number;
  bytes_saved: number;
  hit_ratio_bytes: Ratio;
  hit_ratio_reqs: Ratio;
}

export interface Bucket {
  bytes_hit: number;
  bytes_miss: number;
  reqs_hit: number;
  reqs_miss: number;
}

export interface Point {
  t: number;
  package: Bucket;
  metadata: Bucket;
}

export interface Series {
  bucket: string;
  points: Point[];
}

export interface TopPath {
  path: string;
  name: string;
  version: string;
  repo: string;
  reqs: number;
  bytes_served: number;
  bytes_saved: number;
  hit_ratio_bytes: Ratio;
}

export interface Client {
  ip: string;
  label: string | null;
  last_seen: number;
  package: Kpi;
  metadata: Kpi;
  repos: string[];
  spark: number[];
  top_packages: TopPath[];
}

export interface Ingest {
  last_tick_at: number;
  lag_seconds: number;
  files_tracked: number;
  lines_ingested: number;
  parse_errors: number;
  logs_readable: boolean;
  db_bytes: number;
}

export interface CacheDisk {
  bytes: number;
  free_bytes: number;
}

export interface Payload {
  generated_at: number;
  ingest: Ingest;
  cache_disk: CacheDisk | null;
  kpis: { window: string; all: Kpi; package: Kpi; metadata: Kpi; lifetime: Kpi };
  series_24h: Series;
  series_7d: Series;
  series_30d: Series;
  clients: Client[];
  top_packages: TopPath[];
  top_metadata: TopPath[];
}

export async function fetchStats(): Promise<Payload> {
  const r = await fetch('/api/stats');
  if (!r.ok) throw new Error(`GET /api/stats -> ${r.status}`);
  return r.json();
}

export interface Drilldown {
  ip: string;
  window: string;
  packages: TopPath[];
  metadata: TopPath[];
}

export async function fetchClient(ip: string, window = '24h'): Promise<Drilldown> {
  const r = await fetch(`/api/stats/client/${encodeURIComponent(ip)}?window=${window}`);
  if (!r.ok) throw new Error(`GET /api/stats/client -> ${r.status}`);
  return r.json();
}
