# Plan 03 — Frontend Scaffolding (React + TypeScript)

> **Status**: pending  
> **Created**: 2026-06-11  
> **Depends on**: Plan 01 (API running)  
> **Goal**: Set up React TypeScript frontend with mobile-responsive layout and basic components  
> **Agent**: implementation  

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**  
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Initialize Vite + React + TypeScript project with TailwindCSS and shadcn/ui for mobile-first design.

**Deliverables**:
1. Vite project in `frontend/` directory
2. TailwindCSS configured with mobile breakpoints
3. shadcn/ui components installed (Button, Card, Select, Input)
4. Basic layout with header and main content area
5. API client module for REST calls
6. All tests passing

**Estimated effort**: Medium (1 day)

---

## Context

### Requirements
- Mobile-responsive (primary use case: phone)
- Touch-friendly (44px+ touch targets)
- TypeScript with strict mode
- TailwindCSS for styling
- shadcn/ui for pre-built components

### Tech Stack
- **Framework**: Vite + React 18 + TypeScript
- **Styling**: TailwindCSS + shadcn/ui
- **State**: React Query (for API calls)
- **Routing**: React Router (if multi-page needed)

### API Endpoints Available
- `GET /health` - Health check
- `GET /config` - Server config
- `GET /maps` - List of available maps
- `POST /rcon/execute` - Execute RCON command

---

## Step-by-Step Tasks

### T1: Initialize Vite Project

**File**: `frontend/` directory

**What to do**:
1. Run `npm create vite@latest frontend -- --template react-ts`
2. Install dependencies: `npm install`
3. Configure `tsconfig.json` with strict mode
4. Add `baseUrl` and `paths` for cleaner imports

**Before**: (no frontend)

**After**:
```
frontend/
├── package.json
├── tsconfig.json
├── vite.config.ts
├── src/
│   ├── main.tsx
│   ├── App.tsx
│   └── index.css
```

**Tests**:
- `npm run dev` starts server
- `npm run build` succeeds
- `npm run lint` passes

---

### T2: Configure TailwindCSS

**File**: `frontend/tailwind.config.js`, `frontend/src/index.css`

**What to do**:
1. Install TailwindCSS: `npm install -D tailwindcss postcss autoprefixer`
2. Run `npx tailwindcss init -p`
3. Configure content paths in `tailwind.config.js`
4. Add Tailwind directives to `index.css`
5. Configure mobile breakpoints (default is fine)

**Before**: (no Tailwind)

**After**:
```js
// tailwind.config.js
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      screens: {
        'xs': '480px',  // Extra small (phones)
      },
    },
  },
  plugins: [],
}
```

**Tests**:
- Tailwind classes work in components
- Mobile layout renders correctly

---

### T3: Install shadcn/ui

**File**: `frontend/`

**What to do**:
1. Initialize shadcn: `npx shadcn-ui@latest init`
2. Install components:
   - `npx shadcn-ui@latest add button`
   - `npx shadcn-ui@latest add card`
   - `npx shadcn-ui@latest add select`
   - `npx shadcn-ui@latest add input`
   - `npx shadcn-ui@latest add textarea`
3. Configure theme colors for dark mode (terminal-like aesthetic)

**Before**: (no UI components)

**After**:
```
frontend/src/components/ui/
├── button.tsx
├── card.tsx
├── select.tsx
├── input.tsx
└── textarea.tsx
```

**Tests**:
- Components import and render
- Default styles apply correctly

---

### T4: Create API Client Module

**File**: `frontend/src/lib/api.ts`

**What to do**:
1. Define API base URL (configurable via env var)
2. Create typed fetch wrappers for each endpoint:
   - `getHealth(): Promise<{status: string}>`
   - `getConfig(): Promise<Config>`
   - `getMaps(): Promise<MapInfo[]>`
   - `executeRcon(command: string): Promise<string>`
3. Add error handling and retry logic with React Query

**Before**: (no API client)

**After**:
```typescript
export interface MapInfo {
  name: string;
  filename: string;
  size: number;
}

export async function getMaps(): Promise<MapInfo[]> {
  const res = await fetch('/api/maps');
  return res.json();
}

export async function executeRcon(command: string): Promise<string> {
  const res = await fetch('/api/rcon/execute', {
    method: 'POST',
    body: JSON.stringify({ command }),
  });
  return res.text();
}
```

**Tests**:
- Mock API calls in unit tests
- Error handling tested

---

### T5: Create Basic Layout

**File**: `frontend/src/App.tsx`, `frontend/src/components/Layout.tsx`

**What to do**:
1. Create `Layout` component with header and main content
2. Header: "qctrl" title, server status indicator
3. Main: responsive container with padding
4. Footer: version info, server connection status

**Before**: (empty App)

**After**:
```tsx
function Layout({ children }: { children: React.ReactNode }) {
  return (
    <div className="min-h-screen bg-gray-900 text-white">
      <header className="p-4 border-b border-gray-700">
        <h1 className="text-xl font-bold">qctrl</h1>
      </header>
      <main className="p-4">{children}</main>
    </div>
  );
}
```

**Tests**:
- Layout renders correctly on mobile (375px)
- Layout renders correctly on desktop (1920px)

---

### T6: Add Home Page with Health Check

**File**: `frontend/src/pages/Home.tsx`

**What to do**:
1. Create home page showing server status
2. Display "Connected" or "Disconnected" based on `/health`
3. Add refresh button
4. Show last updated timestamp

**Before**: (no pages)

**After**:
```tsx
function Home() {
  const { data: health, error } = useQuery({ queryKey: ['health'], queryFn: getHealth });
  
  return (
    <Card>
      <CardHeader>Server Status</CardHeader>
      <CardContent>
        {health?.status === 'ok' ? (
          <div className="flex items-center gap-2">
            <span className="text-green-500">●</span> Connected
          </div>
        ) : (
          <div className="text-red-500">Disconnected</div>
        )}
      </CardContent>
    </Card>
  );
}
```

**Tests**:
- Shows connected state when API is up
- Shows disconnected state when API is down

---

### T7: Add Unit Tests

**File**: `frontend/src/**/*.test.tsx`

**What to do**:
1. Install testing library: `npm install -D @testing-library/react @testing-library/jest-dom`
2. Create test for Layout component
3. Create test for Home page
4. Create test for API client (with mocks)
5. Run `npm test`

**Tests**:
- All tests pass
- ≥85% coverage on src/

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `frontend/package.json` | Dependencies | P0 |
| `frontend/tailwind.config.js` | Tailwind config | P0 |
| `frontend/src/lib/api.ts` | API client | P0 |
| `frontend/src/components/Layout.tsx` | Layout component | P0 |
| `frontend/src/pages/Home.tsx` | Home page | P0 |
| `frontend/src/App.tsx` | Main app | P0 |

---

## Open Questions / Risks

1. **API Proxy**: Vite dev server needs proxy to `localhost:3000`. Configure in `vite.config.ts`.
2. **CORS**: Backend must allow CORS from frontend origin.
3. **Environment Variables**: Use `VITE_API_URL` for API base URL.

---

## Verification Checklist

- [ ] T1: `npm run dev` starts Vite dev server
- [ ] T2: Tailwind classes work in components
- [ ] T3: shadcn/ui components import and render
- [ ] T4: API client makes successful calls to backend
- [ ] T5: Layout renders on mobile (375px) and desktop
- [ ] T6: Home page shows server status
- [ ] T7: `npm test` passes with ≥85% coverage
- [ ] T8: `npm run build` succeeds
- [ ] T9: `npm run lint` passes with zero warnings

---

## Next Steps

After Plan 03 completes:
- Plan 04: Deathmatch controls UI (dmflags, timelimit, fraglimit)
- Plan 05: Map selection UI
- Plan 06: Player management (kick/ban)
