import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { bytes, num, pct, rel } from '../lib/format';
import { fetchClient, type Client } from '../lib/api';
import { RatioBar, Sparkline } from './Primitives';

/**
 * ONE DOM tree, responsive by CSS grid.
 *
 * Deliberately not a <table> in a horizontal-scroll container, and deliberately
 * not two duplicated markup paths (`hidden md:block` / `md:hidden`) — that
 * doubles the bug surface and guarantees the two drift. Below `md` each client
 * is a two-column card with inline labels; at `md`+ the same cells snap into
 * columns and the labels disappear.
 */
const COLS =
  'md:grid-cols-[minmax(0,2fr)_repeat(4,minmax(0,1fr))_minmax(0,2fr)]';

function Cell({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="min-w-0">
      <span className="mr-1 text-xs uppercase tracking-wide text-slate-500 md:hidden">{label}</span>
      {children}
    </div>
  );
}

function Drilldown({ ip }: { ip: string }) {
  // Fetched ON EXPAND only: the snapshot carries just a bounded top-10, because
  // clients x packages explodes on dist-upgrade day.
  const { data, isLoading, error } = useQuery({
    queryKey: ['client', ip],
    queryFn: () => fetchClient(ip),
  });
  if (isLoading) return <div className="p-3 text-sm text-slate-400">loading…</div>;
  if (error) return <div className="p-3 text-sm text-rose-400">could not load {ip}</div>;
  const rows = data?.packages ?? [];
  if (rows.length === 0)
    return <div className="p-3 text-sm text-slate-400">no packages in this window</div>;
  return (
    <div className="space-y-1 bg-slate-950/60 p-3">
      {rows.slice(0, 100).map((p) => (
        <div key={p.path} className="flex items-baseline justify-between gap-3 text-sm">
          <span className="truncate" title={p.path}>
            {p.name} <span className="text-slate-500">{p.version}</span>
          </span>
          <span className="shrink-0 font-mono tabular-nums text-slate-300">
            {bytes(p.bytes_served)} · {num(p.reqs)}x · {pct(p.hit_ratio_bytes)}
          </span>
        </div>
      ))}
    </div>
  );
}

export function ClientTable({ clients }: { clients: Client[] }) {
  const [open, setOpen] = useState<string | null>(null);

  if (clients.length === 0) {
    return (
      <div className="rounded-lg bg-slate-900 p-6 text-center text-slate-400 ring-1 ring-white/5">
        No client traffic in this window yet.
      </div>
    );
  }

  return (
    <div role="table" className="w-full">
      <div
        role="row"
        className={`hidden gap-2 px-3 py-2 text-xs uppercase tracking-wide text-slate-400 md:grid ${COLS}`}
      >
        <div>client</div>
        <div>pkg saved</div>
        <div>pkg hit</div>
        <div>meta hit</div>
        <div>requests</div>
        <div>24h</div>
      </div>

      {clients.map((c) => {
        const saved = c.package.bytes_saved + c.metadata.bytes_saved;
        const reqs = c.package.reqs + c.metadata.reqs;
        const isOpen = open === c.ip;
        return (
          <div key={c.ip} className="border-t border-white/5">
            <div
              role="row"
              tabIndex={0}
              onClick={() => setOpen(isOpen ? null : c.ip)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  setOpen(isOpen ? null : c.ip);
                }
              }}
              className={`grid cursor-pointer grid-cols-2 gap-x-3 gap-y-1 px-3 py-3 odd:bg-white/[0.02] hover:bg-white/5 focus:outline focus:outline-1 focus:outline-emerald-500 md:items-center md:gap-y-0 md:py-2 ${COLS}`}
            >
              <Cell label="client">
                <div className="min-w-0">
                  <div className="truncate font-medium">{c.label ?? c.ip}</div>
                  {/* Always show the IP, even when labelled: a dashboard that
                      only shows friendly names is useless for the machine you
                      have not labelled yet. */}
                  <div className="truncate font-mono text-xs text-slate-500">
                    {c.ip} · {rel(c.last_seen)}
                  </div>
                </div>
              </Cell>
              <Cell label="saved">
                <span className="font-mono tabular-nums text-emerald-400">{bytes(saved)}</span>
              </Cell>
              <Cell label="pkg hit">
                <div>
                  <span className="font-mono text-sm tabular-nums">{pct(c.package.hit_ratio_bytes)}</span>
                  <RatioBar ratio={c.package.hit_ratio_bytes} />
                </div>
              </Cell>
              <Cell label="meta hit">
                <div>
                  <span className="font-mono text-sm tabular-nums">{pct(c.metadata.hit_ratio_bytes)}</span>
                  <RatioBar ratio={c.metadata.hit_ratio_bytes} tone="sky" />
                </div>
              </Cell>
              <Cell label="requests">
                <span className="font-mono tabular-nums">{num(reqs)}</span>
              </Cell>
              <div className="col-span-2 md:col-span-1">
                <Sparkline values={c.spark} />
              </div>
            </div>
            {isOpen && <Drilldown ip={c.ip} />}
          </div>
        );
      })}
    </div>
  );
}
