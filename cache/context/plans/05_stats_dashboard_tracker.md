# Stats Dashboard — Tracker

## Overview
- Status: 0% complete (0 of 5 tasks)
- Start date: *(not started)*
- Depends on: **Plan 04 T2** to start (frozen schema + `fixture.json`); Plan 04 complete to
  verify end-to-end
- Dev loop: `cd stats/frontend && npm run dev` → `http://localhost:5173`, `/api` proxied to
  `http://localhost:8081`
- Node: **22** (`.nvmrc`) — node ≥ 24 SIGSEGVs inside vite/vitest on this host with no output

## Resume Instructions

This plan can run **in parallel with Plan 04 T3–T5** once `fixture.json` is committed. Build
everything against the fixture first; it deliberately contains the awkward cases (zero-traffic
client, IPv6-only client, `parse_errors > 0`, `logs_readable: false`, a single huge MISS that
tanks an otherwise healthy ratio). If it renders those correctly, the live API will be easy.

Read Plan 05's Context section — the "numbers must never lie" rules are not stylistic. A
blended hit ratio or a `0%` where the answer is "no data" defeats the entire point of the
metadata/package split that Plans 02–04 were built around.

Order: T2 (pure maths) before T3 (components) before T4/T5 (layout). The only tests worth
writing here are `scale.ts` and `format.ts` — component smoke tests can wait, and qctrl's
experience suggests they mostly assert that React renders.

**Run the `dataviz` skill before T3**, not after — it validates the palette for contrast in
both tile and chart contexts, and retrofitting a palette across finished components is
tedious.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: scaffold | `frontend/{package.json,vite,tailwind,tsconfig}` | pending | steal qctrl's node-version + node_modules guards |
| 2 | T2: pure chart maths | `src/lib/{scale,format}.ts` | pending | the only genuinely valuable tests here |
| 3 | T3: SVG primitives | `src/components/{Sparkline,StackedBars,Kpi,RatioBar}.tsx` | pending | `vectorEffect` is mandatory; run `dataviz` first |
| 4 | T4: dashboard layout | `src/pages/Dashboard.tsx` | pending | `keepPreviousData`; loud IngestHealth banner |
| 5 | T5: client table + drilldown | `src/components/Client*.tsx` | pending | one DOM tree, not two markup paths |

## Decisions Pending Confirmation

| Question | Default if unanswered | Decide by |
|---|---|---|
| Adopt qctrl's `node_modules.<env>` scheme? | no — only needed if this repo is bind-mounted across host/container | T1 |
| Palette (after `dataviz`) | packages emerald, metadata sky, upstream amber, errors rose | T3 |
| Sort order for the client table | bytes saved, descending | T5 |

## Notes / Deviations

*(none yet)*
