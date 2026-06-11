# Plan 08 — Server Status Dashboard

> **Status**: pending  
> **Created**: 2026-06-11  
> **Depends on**: Plan 01-07 (all features)  
> **Goal**: Create a summary dashboard showing all server stats in one view  
> **Agent**: implementation  

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**  
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Build a home dashboard with quick stats and links to all features.

**Deliverables**:
1. Server status card (online/offline, uptime)
2. Current map display
3. Player count and list preview
4. Current settings (dmflags, timelimit, fraglimit)
5. Quick action buttons (restart, change map)
6. Auto-refresh every 10 seconds
7. Unit tests

**Estimated effort**: Small–Medium (half day)

---

## Context

### Requirements
- Single summary view of all server state
- Quick access to all features
- Auto-refresh for live updates
- Mobile-friendly (stacked cards)
- Desktop-friendly (grid layout)

### Data Sources
- `/health` - Server status
- `/status` - Player list and current map
- `/config` - Current settings
- `/maps` - Available maps (cached)

---

## Step-by-Step Tasks

### T1: Create Status Card Component

**File**: `frontend/src/components/StatusCard.tsx`

**What to do**:
1. Display server connection status
2. Show last updated timestamp
3. Add refresh button
4. Green/red indicator for online/offline

**Before**: (no dashboard)

**After**:
```tsx
function StatusCard() {
  const { data: health, error, refetch } = useQuery({
    queryKey: ['health'],
    queryFn: getHealth,
    refetchInterval: 10000,
  });
  
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex justify-between items-center">
          Server Status
          <Button variant="ghost" size="icon" onClick={() => refetch()}>
            <RefreshIcon />
          </Button>
        </CardTitle>
      </CardHeader>
      <CardContent>
        <div className="flex items-center gap-2">
          <span className={health ? "text-green-500" : "text-red-500"}>
            ●
          </span>
          <span>{health ? "Online" : "Offline"}</span>
        </div>
        <p className="text-sm text-gray-500 mt-2">
          Last updated: {new Date().toLocaleTimeString()}
        </p>
      </CardContent>
    </Card>
  );
}
```

**Tests**:
- Shows online/offline state
- Refresh button works
- Timestamp updates

---

### T2: Create Quick Stats Component

**File**: `frontend/src/components/QuickStats.tsx`

**What to do**:
1. Display in grid (2x2 on mobile, 4x1 on desktop)
2. Stats:
   - Players (current/max)
   - Current map
   - Timelimit
   - Fraglimit
3. Tap to go to detailed view

**Before**: (no quick stats)

**After**:
```tsx
function QuickStats() {
  const { data: status } = useQuery({ queryKey: ['status'], queryFn: getStatus });
  
  return (
    <div className="grid grid-cols-2 gap-4">
      <StatCard title="Players" value={`${status?.players.length ?? 0}/25`} />
      <StatCard title="Map" value={status?.currentMap ?? "Unknown"} />
      <StatCard title="Time Limit" value={`${status?.timelimit} min`} />
      <StatCard title="Frag Limit" value={status?.fraglimit ?? "∞"} />
    </div>
  );
}
```

**Tests**:
- Stats display correctly
- Values update on refresh
- Mobile grid layout works

---

### T3: Create Quick Actions Component

**File**: `frontend/src/components/QuickActions.tsx`

**What to do**:
1. Large buttons for common actions:
   - Restart Map
   - Change Map
   - View Players
   - View Logs
2. Icon + text for clarity
3. Navigate to respective pages

**Before**: (no quick actions)

**After**:
```tsx
function QuickActions() {
  return (
    <div className="grid grid-cols-2 gap-4">
      <Button asChild className="h-20">
        <Link to="/maps">
          <MapIcon />
          Change Map
        </Link>
      </Button>
      <Button asChild variant="destructive" className="h-20">
        <Link to="/deathmatch">
          <RestartIcon />
          Restart Map
        </Link>
      </Button>
      <Button asChild className="h-20">
        <Link to="/players">
          <UsersIcon />
          Players
        </Link>
      </Button>
      <Button asChild className="h-20">
        <Link to="/logs">
          <TerminalIcon />
          Logs
        </Link>
      </Button>
    </div>
  );
}
```

**Tests**:
- Buttons navigate correctly
- Icons render
- Mobile layout stacks

---

### T4: Create Dashboard Page

**File**: `frontend/src/pages/Dashboard.tsx`

**What to do**:
1. Combine all components
2. Add header
3. Responsive layout (stack on mobile, grid on desktop)
4. Set as default route

**Before**: (no dashboard)

**After**:
```tsx
function Dashboard() {
  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold">Server Dashboard</h1>
      <StatusCard />
      <QuickStats />
      <QuickActions />
    </div>
  );
}
```

**Tests**:
- Page renders all components
- Navigation works
- Auto-refresh works

---

### T5: Add Navigation

**File**: `frontend/src/components/Navbar.tsx`

**What to do**:
1. Bottom nav on mobile (thumb-friendly)
2. Top nav on desktop
3. Links: Dashboard, Maps, Players, Logs, Settings
4. Active state indicator

**Before**: (no nav)

**After**:
```tsx
function Navbar() {
  return (
    <nav className="md:hidden fixed bottom-0 left-0 right-0 bg-gray-800 border-t">
      <div className="flex justify-around py-2">
        <NavLink to="/" icon={<HomeIcon />} label="Home" />
        <NavLink to="/maps" icon={<MapIcon />} label="Maps" />
        <NavLink to="/players" icon={<UsersIcon />} label="Players" />
        <NavLink to="/logs" icon={<TerminalIcon />} label="Logs" />
      </div>
    </nav>
  );
}
```

**Tests**:
- Nav renders on mobile
- Nav renders on desktop
- Active state shows

---

### T6: Add Unit Tests

**File**: `frontend/src/pages/Dashboard.test.tsx`

**What to do**:
1. Test dashboard rendering
2. Test auto-refresh
3. Test navigation
4. Test responsive layout

**Tests**:
- All tests pass
- ≥85% coverage on new code

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `frontend/src/components/StatusCard.tsx` | New component | P0 |
| `frontend/src/components/QuickStats.tsx` | New component | P0 |
| `frontend/src/components/QuickActions.tsx` | New component | P0 |
| `frontend/src/components/Navbar.tsx` | New component | P0 |
| `frontend/src/pages/Dashboard.tsx` | New page | P0 |

---

## Open Questions / Risks

1. **Auto-refresh**: 10s may be too frequent. Consider user preference.
2. **Mobile Nav**: Bottom nav takes screen space. Alternative: hamburger menu.
3. **Performance**: Multiple queries may slow initial load. Consider query bundling.

---

## Verification Checklist

- [ ] T1: Status card shows connection state
- [ ] T2: Quick stats display all values
- [ ] T3: Quick actions navigate correctly
- [ ] T4: Dashboard page combines all components
- [ ] T5: Navigation works on mobile and desktop
- [ ] T6: Auto-refresh updates all values
- [ ] T7: Mobile layout usable (375px)
- [ ] T8: `npm test` passes with ≥85% coverage

---

## Next Steps

After Plan 08 completes:
- Plan 09: Settings persistence
- Plan 10: Final testing and polish
- Plan 11: Deployment setup
