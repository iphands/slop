import { useState } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { fetchStats, type Payload, type Series, type TopPath } from '../lib/api';
import { bytes, dayLabel, hourLabel, num, pct, rel } from '../lib/format';
import { Kpi, StackedBars } from '../components/Primitives';
import { ClientTable } from '../components/ClientTable';
import { IngestHealth } from '../components/IngestHealth';

type Window = '24h' | '7d' | '30d';

function seriesFor(d: Payload, w: Window): Series {
  return w === '24h' ? d.series_24h : w === '7d' ? d.series_7d : d.series_30d;
}

function TopList({ title, rows }: { title: string; rows: TopPath[] }) {
  if (rows.length === 0) return null;
  return (
    <section className="rounded-lg bg-slate-900 p-4 ring-1 ring-white/5">
      <h2 className="mb-3 text-sm uppercase tracking-wide text-slate-400">{title}</h2>
      <div className="space-y-1.5">
        {rows.slice(0, 15).map((p) => (
          <div key={p.path} className="flex items-baseline justify-between gap-3 text-sm">
            {/* The parsed name, not the 60-110 char path -- which would destroy
                the column layout. Full path stays in the tooltip. */}
            <span className="truncate" title={p.path}>
              {p.name} <span className="text-slate-500">{p.version}</span>
            </span>
            <span className="shrink-0 font-mono text-xs tabular-nums text-slate-300">
              {bytes(p.bytes_served)} · {pct(p.hit_ratio_bytes)}
            </span>
          </div>
        ))}
      </div>
    </section>
  );
}

export function Dashboard() {
  const [win, setWin] = useState<Window>('24h');

  const { data, error, isPending } = useQuery({
    queryKey: ['stats'],
    queryFn: fetchStats,
    refetchInterval: 5000,
    staleTime: 4000,
    // Without this the whole dashboard unmounts and flashes empty on every
    // 5s poll. The cause is non-obvious when it is missing.
    placeholderData: keepPreviousData,
    retry: 2,
  });

  if (isPending) return <div className="p-6 text-slate-400">loading…</div>;
  if (error || !data)
    return <div className="p-6 text-rose-400">cannot reach /api/stats — is the service up?</div>;

  const s = seriesFor(data, win);
  // Window switching is pure client state: all three series ship in every
  // payload, so there is no refetch and no latency.
  const hit = s.points.map((p) => p.package.bytes_hit + p.metadata.bytes_hit);
  const miss = s.points.map((p) => p.package.bytes_miss + p.metadata.bytes_miss);
  const labels = s.points.map((p) => (s.bucket === 'hour' ? hourLabel(p.t) : dayLabel(p.t)));

  const k = data.kpis;
  const cache = data.cache_disk;
  const CAP = 100 * 1024 ** 3; // proxy_cache_path max_size=100g
  const full = cache ? cache.bytes / CAP : 0;

  return (
    <div className="mx-auto max-w-6xl space-y-4 p-3 sm:p-6">
      <header className="flex flex-wrap items-baseline justify-between gap-2">
        <h1 className="text-xl font-semibold">
          pkgcache <span className="text-slate-500">stats</span>
        </h1>
        <div className="font-mono text-xs tabular-nums text-slate-500">
          updated {rel(data.generated_at)} · {num(data.ingest.lines_ingested)} lines ingested
        </div>
      </header>

      <IngestHealth ingest={data.ingest} />

      {/* Package and metadata are ALWAYS separate. Metadata has a 60s TTL and is
          supposed to miss; blending them makes a healthy cache report ~30%. */}
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Kpi
          label="saved (24h)"
          value={bytes(k.all.bytes_saved)}
          sub={`${bytes(k.all.bytes_served)} served`}
        />
        <Kpi
          label="packages"
          value={bytes(k.package.bytes_saved)}
          sub={`${num(k.package.reqs)} requests`}
          ratio={k.package.hit_ratio_bytes}
        />
        <Kpi
          label="metadata"
          value={bytes(k.metadata.bytes_saved)}
          // "low is correct" only when it IS low. Metadata has a 60s TTL so it
          // usually misses, but asserting that next to a 100% ratio reads as a
          // bug in the dashboard rather than an explanation.
          sub={
            k.metadata.hit_ratio_bytes !== null && k.metadata.hit_ratio_bytes < 0.5
              ? `${num(k.metadata.reqs)} requests · a low ratio is correct here`
              : `${num(k.metadata.reqs)} requests`
          }
          ratio={k.metadata.hit_ratio_bytes}
          tone="sky"
        />
        {cache ? (
          <Kpi
            label="cache on disk"
            value={bytes(cache.bytes)}
            sub={`of ${bytes(CAP)} max_size`}
            ratio={full}
            ratioLabel="full"
            tone={full > 0.85 ? 'sky' : 'emerald'}
          />
        ) : (
          <Kpi label="lifetime saved" value={bytes(k.lifetime.bytes_saved)} sub="since first run" />
        )}
      </div>

      {full > 0.85 && (
        <div className="rounded-lg border border-amber-500/40 bg-amber-500/10 p-3 text-sm text-amber-200">
          The cache is {pct(full)} of its {bytes(CAP)} cap. Past it nginx starts evicting, which
          quietly turns HITs back into MISSes.
        </div>
      )}

      <section className="rounded-lg bg-slate-900 p-4 ring-1 ring-white/5">
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm uppercase tracking-wide text-slate-400">
            bytes served — <span className="text-emerald-400">cache</span> vs{' '}
            <span className="text-amber-400">upstream</span>
          </h2>
          <div className="flex gap-1">
            {(['24h', '7d', '30d'] as Window[]).map((w) => (
              <button
                key={w}
                onClick={() => setWin(w)}
                className={`rounded px-2 py-1 font-mono text-xs ${
                  win === w ? 'bg-emerald-600 text-white' : 'bg-slate-800 text-slate-300'
                }`}
              >
                {w}
              </button>
            ))}
          </div>
        </div>
        <StackedBars a={hit} b={miss} labels={labels} />
        <div className="mt-1 flex justify-between font-mono text-xs text-slate-500">
          <span>{labels[0]}</span>
          <span>{labels[labels.length - 1]}</span>
        </div>
      </section>

      <section className="rounded-lg bg-slate-900 p-4 ring-1 ring-white/5">
        <h2 className="mb-2 text-sm uppercase tracking-wide text-slate-400">
          clients — click a row for what it pulled
        </h2>
        <ClientTable clients={data.clients} />
      </section>

      <div className="grid gap-4 lg:grid-cols-2">
        <TopList title="top packages (24h)" rows={data.top_packages} />
        <TopList title="top metadata (24h)" rows={data.top_metadata} />
      </div>

      <footer className="pb-6 font-mono text-xs text-slate-600">
        lifetime: {bytes(k.lifetime.bytes_saved)} saved of {bytes(k.lifetime.bytes_served)} served ·
        db {bytes(data.ingest.db_bytes)} · {num(data.ingest.files_tracked)} log files tracked
      </footer>
    </div>
  );
}
