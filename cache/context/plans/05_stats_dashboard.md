# Plan 05 — Stats: Dashboard Frontend

> **Status**: pending
> **Created**: 2026-07-18
> **Depends on**: Plan 04 T2 (frozen payload schema + `fixture.json`) to start; Plan 04 complete to verify end-to-end
> **Goal**: A dark, fast, mobile-responsive dashboard at `:8081` showing global and per-client cache effectiveness, with charts hand-rolled in SVG and no charting dependency.
> **Agent**: implementation agent

---

> **Before writing any config, re-read `context/plans/RULES.md` in full** — especially
> Rule A (prove it runs and caches; there is no compiler here).
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: React + Vite + TypeScript + Tailwind dashboard consuming `GET /api/stats`,
polling every 5s, with all visualization hand-rolled as SVG paths from pure functions.

**Deliverables**:
1. Vite/React/TS/Tailwind scaffold under `stats/frontend/`
2. `lib/scale.ts` — pure chart maths (`linePath`, `areaPath`, `stackRects`), unit-tested
3. KPI tiles, sparklines, stacked time-series bars, ratio bars — all SVG/CSS, no chart lib
4. Responsive client table (cards on phones, columns on desktop) from **one** DOM tree
5. Per-client drilldown hitting `/api/stats/client/{ip}`
6. Ingest-health banner surfacing `logs_readable` / `parse_errors`

**Estimated effort**: Medium (1 day)

---

## Context

The proxy has been unobservable since it shipped. Plans 02–04 produce and serve the
numbers; this plan is the only part a human ever looks at, so "beautiful AND performant"
is the actual requirement, not a nice-to-have.

**This plan can start as soon as Plan 04 T2 lands.** `fixture.json` is a frozen, realistic
payload including the awkward cases, so the whole UI can be built against `npm run dev`
with no backend running. That is the only real concurrency in the 02→05 chain.

### Why no charting library

There is nothing to inherit — qctrl has zero charting deps, zero SVG, zero canvas. Given a
free choice, the charts here are a sparkline, a stacked bar series, and a ratio bar. All
three are a handful of SVG path strings. A library would add 45–100 KB, a styling fight to
match the dark theme, and an API to learn, in exchange for interactions this dashboard
does not need.

The discipline that keeps this maintainable: **the maths lives in pure functions in
`lib/scale.ts` and the components are dumb wrappers.** That is what makes charts testable
with no DOM, and it is the difference between "hand-rolled SVG" being a good decision and a
regrettable one.

### The numbers must never lie

The domain constraint from `CLAUDE.md` and Plan 02 carries all the way to the pixels:

- **Headline is hit ratio by *bytes*, not requests.** Bytes are what the cache exists to
  save.
- **Metadata and packages are always shown separately.** Metadata has a 60s TTL and is
  *supposed* to miss; blending it in makes a healthy cache report ~30%.
- **A `null` ratio renders `—`, never `0%`.** A cold cache with no traffic is not a broken
  cache.
- **Always show absolute bytes next to every percentage.** One 300 MB kernel MISS tanks the
  package byte-ratio in a quiet week; the absolute number and the lifetime total are what
  keep that honest.

qctrl's `useMapClock`/`ClockQuality` types are the house precedent worth imitating here —
it models data *quality* explicitly and refuses to render a number it can't back up.

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| Payload | all 3 windows in one response; window switching is pure client state | Plan 04 T1 |
| Poll interval | 5s, React Query | user decision |
| Ratios | `number \| null`; `null` ⇒ render `—` | Plan 04 T1 |
| Node ≥ 24 SIGSEGVs in vite/vitest on this host | use node 22 | qctrl `justfile`, 2026-07-18 |
| qctrl stack | React 19, Vite, TS, Tailwind **3** (not 4), React Query 5, vitest | explored 2026-07-18 |
| qctrl has no chart code at all | greenfield | explored 2026-07-18 |
| Assets are embedded at build time | `npm run build` must precede `cargo build` | Plan 04 T3 |
| Container engine | **docker** to verify here; **podman** on `noir.lan` (no docker there) | 2026-07-18 |

---

## Step-by-Step Tasks

### T1: Scaffold

**Files**: `stats/frontend/{package.json,vite.config.ts,vitest.config.ts,tailwind.config.js,tsconfig*.json,.nvmrc,eslint.config.js}`

**What to do**: Vite + React 19 + TS + Tailwind 3 + React Query 5 + vitest, matching qctrl's
versions so the two projects don't drift.

**Steal qctrl's two hard-won `justfile` guards** (`/home/iphands/prog/slop/qctrl/justfile`)
— they cost an afternoon each to find:
1. **Node version guard** — node ≥ 24 SIGSEGVs inside vite/vitest on this host with *zero
   output*. Pin via `.nvmrc` = 22 and fail loudly on ≥ 24.
2. **Per-environment `node_modules`** — if this repo is ever bind-mounted across host and
   container, native esbuild/rollup binaries cannot be shared. qctrl installs into
   `node_modules.<env>` and symlinks. Adopt it *only if* this repo is actually used that
   way; note the decision either way. If adopted, `vitest.config.ts` needs an explicit
   `include`/`exclude`, because the default `**/node_modules/**` pattern does not match
   `node_modules.<env>` and vitest will crawl the dependency tree.

Vite dev proxy `/api` → `http://localhost:8081`. Commit `dist/.gitkeep` (Plan 04 T3 needs
the directory to exist for `rust-embed`).

**Verify**: `npm run build` produces `dist/`; `npm run lint` clean; `npm test` runs (even
with no tests yet); node version guard fires on node 24.

**Commit**: `task(T1): frontend scaffold`

---

### T2: Pure chart maths

**File**: `stats/frontend/src/lib/scale.ts` + `__tests__/scale.test.ts`

**What to do**: no React, no DOM — numbers in, SVG path strings out.

```ts
export function linePath(v: number[], w: number, h: number, max?: number): string {
  if (v.length === 0) return '';
  const hi = Math.max(max ?? 0, ...v, 1);          // never divide by zero
  const dx = v.length > 1 ? w / (v.length - 1) : 0;
  return v.map((y, i) =>
    `${i ? 'L' : 'M'}${(i * dx).toFixed(2)},${(h - (y / hi) * h).toFixed(2)}`
  ).join(' ');
}
export function areaPath(v: number[], w: number, h: number, max?: number): string;
export function stackRects(a: number[], b: number[], w: number, h: number, gap?: number);
```

Also `lib/format.ts` — `bytes()` (binary units, 3 significant figures), `pct()` (returns
`'—'` for `null`), `rel()` (relative timestamps), `num()`.

**Verify**: `npm test` — empty array, single point, all-zeros (no NaN, no `Infinity`), a
`null`-safe `pct`, and a snapshot of a known path string. These are the only tests in the
frontend that are genuinely worth writing; component smoke tests can wait.

**Commit**: `task(T2): pure SVG path + formatting helpers`

---

### T3: Primitives

**Files**: `src/components/{Kpi,KpiRow,RatioBar,Sparkline,StackedBars,WindowPicker,KindToggle}.tsx`

**What to do**:

```tsx
export function Sparkline({ values, className = 'h-10 w-full' }: Props) {
  const W = 100, H = 30;
  return (
    <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none"
         className={className} aria-hidden="true">
      <path d={areaPath(values, W, H)} className="fill-emerald-500/15" />
      <path d={linePath(values, W, H)} className="stroke-emerald-400 fill-none"
            strokeWidth={1.5} vectorEffect="non-scaling-stroke" />
    </svg>
  );
}
```

Three details that are not optional:
- `viewBox` + `preserveAspectRatio="none"` + a Tailwind width class gives resize-with-container
  and **never needs a `ResizeObserver`**.
- **`vectorEffect="non-scaling-stroke"` is mandatory.** Without it the non-uniform scale
  stretches the stroke into a wedge — thick at one end, thin at the other. It is the single
  most common hand-rolled-SVG bug and it only shows up at wide aspect ratios, i.e. on the
  reviewer's monitor and not yours.
- `RatioBar` is **two divs**, not SVG — use the platform for a one-dimensional bar.

Tooltips are native `<title>` elements inside each `<rect>`: accessible, zero JS, zero
state. Upgrade to a positioned div only if that proves insufficient.

**`tabular-nums` on every number.** Without it a 5s-polling table jitters horizontally on
every refresh — it is the difference between "live dashboard" and "twitching mess".

**Run the `dataviz` skill before writing the first chart component** to validate the
palette for contrast in both tile and chart contexts. Semantic colors: packages emerald,
metadata sky, from-upstream amber, errors rose — and **never encode hit-vs-miss by hue
alone**; pair it with stacking order and a text label so it survives colorblindness and
greyscale.

**Verify**: render each against `fixture.json` in `npm run dev`; check a sparkline at
`w-24` and at `w-full` on a 2560px monitor (the wedge bug); zoom to 200%.

**Commit**: `task(T3): SVG chart primitives`

---

### T4: Dashboard layout

**Files**: `src/pages/Dashboard.tsx`, `src/components/{IngestHealth,TopPaths,RepoBreakdown}.tsx`, `src/lib/api.ts`

**What to do**: top to bottom — ingest-health banner (only when something is wrong), global
KPI row, time-series chart with the window picker, client table, top packages / top
metadata, repo breakdown.

`IngestHealth` must be loud and specific when `logs_readable === false` — that is the uid
mismatch, and the whole point of instrumenting it in Plan 03 was to put a sentence on
screen instead of leaving someone to debug silent zeros. Also surface `parse_errors > 0`
and `lag_seconds > 60`.

Every KPI tile shows: the absolute number, the ratio (or `—`), and a sparkline. Package and
metadata are always adjacent and always labeled — never a single blended figure.

```ts
useQuery({
  queryKey: ['stats'],
  queryFn: fetchStats,
  refetchInterval: 5000,
  staleTime: 4000,
  placeholderData: keepPreviousData,   // ← without this the whole page unmounts every poll
  retry: 2,
});
```
`placeholderData: keepPreviousData` is the one that matters and the cause is non-obvious
when it's missing. Leave `refetchIntervalInBackground` at its default `false` so hidden
tabs stop polling.

`lib/api.ts` holds TS interfaces mirroring the Rust payload plus a plain `fetch` wrapper —
no axios, no codegen (qctrl house style).

**Verify**: against `fixture.json` first (all the awkward cases render correctly), then
against the live API.

**Commit**: `task(T4): dashboard layout, KPIs, health banner`

---

### T5: Responsive client table + drilldown

**Files**: `src/components/{ClientTable,ClientRow,ClientDrilldown}.tsx`

**What to do**: **one** DOM tree — CSS grid plus ARIA roles, cards below `md`, columns at
`md` and up.

```tsx
<div role="row"
     className="grid grid-cols-2 gap-x-3 gap-y-1 rounded-lg px-3 py-3
                odd:bg-white/[0.02] hover:bg-white/5
                md:grid-cols-[minmax(0,2fr)_repeat(5,minmax(0,1fr))_3fr]
                md:items-center md:gap-y-0 md:py-2">
```

Below `md`, each client is a two-column card with inline labels; at `md`+ the same cells
snap into columns and the labels (`md:hidden`) disappear. Sparkline is
`col-span-2 md:col-span-1`.

**Do not** ship a `<table>` in a horizontal-scroll container, and **do not** ship two
duplicated markup paths (`hidden md:block` / `md:hidden`) — that doubles the bug surface
and the two will drift.

Columns: client (label or IP), package reqs, package bytes saved, package hit%, metadata
hit%, 24h sparkline. Clicking a row expands `ClientDrilldown`, which fetches
`/api/stats/client/{ip}` on demand — the snapshot only carries top-10.

Show the IP when a label exists too (a dashboard that only shows friendly names is useless
for the machine you haven't labeled yet). Flag it in the UI if two clients resolve to the
same label — that is the IPv4/IPv6 split from Plan 03.

**Verify**: 375px, 768px, 1440px, 2560px; keyboard-navigable expand/collapse; a client with
zero traffic renders `—`; drilldown fetches only on expand (check the network tab).

**Commit**: `task(T5): responsive client table + drilldown`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `stats/frontend/src/lib/scale.ts` | pure chart maths (the testable core) | P0 |
| `stats/frontend/src/pages/Dashboard.tsx` | layout | P0 |
| `stats/frontend/src/components/ClientTable.tsx` | one-DOM-tree responsive table | P0 |
| `stats/frontend/src/lib/api.ts` | types mirroring the Rust payload | P0 |
| `stats/frontend/src/components/IngestHealth.tsx` | the silent-zeros safety net | P1 |
| `stats/frontend/src/components/Sparkline.tsx` | `vectorEffect` + `preserveAspectRatio` | P1 |
| `stats/frontend/tailwind.config.js`, `index.css` | dark theme, palette | P1 |
| `README.md` | screenshot + what the dashboard shows | P2 |

---

## Open Questions / Risks

1. **The wedge-stroke bug** appears only at wide aspect ratios. — *Mitigation:*
   `vectorEffect="non-scaling-stroke"`, checked explicitly at 2560px in T3.
2. **Palette contrast is unvalidated** until the `dataviz` skill runs. — *Mitigation:* run
   it before T3, not after.
3. **A blended hit ratio could still creep in** via a "simplification". — *Mitigation:* no
   component ever receives a pre-blended figure; package and metadata are separate props
   all the way down.
4. **Node version** — vite/vitest SIGSEGV on ≥ 24 with no output, which reads as a broken
   project. — *Mitigation:* `.nvmrc` + a guard that fails with an explanation.
5. **Fixture drift.** The UI is built against `fixture.json`; if the real payload changes,
   the UI breaks at integration time. — *Mitigation:* Plan 04 T2's test asserts the fixture
   deserializes into the Rust type, so drift is caught in CI-equivalent, not by hand.
6. **50+ clients** would make the table long and the payload large. — *Mitigation:* not a
   homelab problem today; if it arrives, sort by bytes and paginate.

---

## Verification Checklist

- [ ] T1: `npm run build` + `npm run lint` clean; node guard fires on node ≥ 24
- [ ] T2: `npm test` green — empty/single/all-zero inputs produce no `NaN` or `Infinity`
- [ ] T3: `dataviz` skill run before the first chart component
- [ ] T3: sparkline stroke is uniform at `w-24` **and** full width on a 2560px display
- [ ] T3: every number uses `tabular-nums` (no jitter across a 5s poll)
- [ ] T3: hit-vs-miss is distinguishable in greyscale (not hue alone)
- [ ] T4: `logs_readable: false` in the fixture renders a loud, specific banner
- [ ] T4: `parse_errors > 0` is surfaced, not swallowed
- [ ] T4: the page does **not** flash empty on each 5s poll (`keepPreviousData`)
- [ ] T4: package and metadata figures are always adjacent and labeled; no blended ratio
- [ ] T4: every percentage has its absolute byte figure beside it
- [ ] T5: correct at 375 / 768 / 1440 / 2560 px; body never scrolls horizontally
- [ ] T5: one DOM tree — no duplicated mobile/desktop markup
- [ ] T5: drilldown fetches only on expand
- [ ] T5: a zero-traffic client renders `—`, not `0%`
- [ ] All: numbers on screen match `curl :8081/api/stats` for the same window
- [ ] All: works end-to-end from the built container (embedded assets, not `npm run dev`)
- [ ] All: `README.md` updated; findings harvested (Rule D)
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
