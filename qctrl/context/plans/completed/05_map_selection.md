# Plan 05 — Map Selection UI

> **Status**: pending  
> **Created**: 2026-06-11  
> **Depends on**: Plan 02 (Map listing API), Plan 03 (Frontend scaffolding)  
> **Goal**: Create mobile-friendly map selection interface with grid/toggle options  
> **Agent**: implementation  

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**  
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Build a responsive map picker that displays available maps as selectable buttons (no typing required).

**Deliverables**:
1. Grid of map buttons (2-3 columns on mobile, more on desktop)
2. Search/filter input for large map lists
3. "Current map" indicator at top
4. Tap to change map (with confirmation)
5. Loading and error states
6. Unit tests

**Estimated effort**: Small–Medium (half day)

---

## Context

### Requirements
- No typing: maps displayed as buttons
- Mobile-first: touch targets ≥44px
- Handle 10-50 maps gracefully
- Show current map being played
- Confirm before changing map (destructive action)

### Available Maps (from user's listing)
```
007_facility.bsp
0wnt.bsp
1.bsp
111.bsp
1111.bsp
11111111.bsp
12.bsp
123.bsp
132tree.bsp
... (more)
```

### API Endpoint
- `GET /maps` returns `[{ name, filename, size, modified }]`

---

## Step-by-Step Tasks

### T1: Create Map Grid Component

**File**: `frontend/src/components/MapGrid.tsx`

**What to do**:
1. Fetch maps from API on mount
2. Display as responsive grid (2 cols mobile, 4-6 cols desktop)
3. Each card shows:
   - Map name (truncated if long)
   - File size (optional)
   - "Current" badge if active
4. Tap to select (opens confirmation dialog)

**Before**: (no map UI)

**After**:
```tsx
function MapGrid() {
  const { data: maps, error, isLoading } = useQuery({ queryKey: ['maps'], queryFn: getMaps });
  
  if (isLoading) return <LoadingSpinner />;
  if (error) return <ErrorMessage error={error} />;
  
  return (
    <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
      {maps?.map(map => (
        <Button
          key={map.name}
          variant="outline"
          className="h-20 flex flex-col items-center justify-center"
          onClick={() => setSelectedMap(map)}
        >
          <span className="font-medium truncate">{map.name}</span>
          {map.name === currentMap && <span className="text-xs text-green-500">Current</span>}
        </Button>
      ))}
    </div>
  );
}
```

**Tests**:
- Grid renders maps correctly
- Loading state shows
- Error state shows

---

### T2: Add Search/Filter Input

**File**: `frontend/src/components/MapGrid.tsx` (extend)

**What to do**:
1. Add search input above grid
2. Filter maps by name (case-insensitive)
3. Show "No maps found" when filter matches nothing
4. Debounce input (300ms) for performance

**Before**: (no search)

**After**:
```tsx
const [search, setSearch] = useState("");
const filteredMaps = useMemo(() => 
  maps?.filter(m => m.name.toLowerCase().includes(search.toLowerCase())) ?? [],
  [maps, search]
);

return (
  <div>
    <Input
      placeholder="Search maps..."
      value={search}
      onChange={e => setSearch(e.target.value)}
      className="mb-4"
    />
    <MapGrid maps={filteredMaps} />
  </div>
);
```

**Tests**:
- Search filters correctly
- Empty state shows when no matches
- Debounce works

---

### T3: Add Map Change Confirmation

**File**: `frontend/src/components/MapDialog.tsx`

**What to do**:
1. Use shadcn/ui Dialog component
2. Show selected map name
3. "Cancel" and "Change Map" buttons
4. Show loading state during change
5. Close on success, show error on failure

**Before**: (no confirmation)

**After**:
```tsx
function MapDialog({ map, open, onOpenChange }: ...) {
  const { mutate: execute, isPending } = useMutation({ mutationFn: executeRcon });
  
  const handleChange = () => {
    execute(`map ${map.name}`, {
      onSuccess: () => onOpenChange(false),
    });
  };
  
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>Change Map?</DialogHeader>
        <p>Switching to <strong>{map.name}</strong> will restart the server.</p>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button onClick={handleChange} disabled={isPending}>
            {isPending ? "Changing..." : "Change Map"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
```

**Tests**:
- Dialog opens on map select
- Cancel closes dialog
- Change sends correct command

---

### T4: Add Current Map Display

**File**: `frontend/src/components/CurrentMap.tsx`

**What to do**:
1. Fetch current map from `/rcon status` or cache
2. Display prominently at top of page
3. "Refresh" button to update
4. Show "Unknown" if can't fetch

**Before**: (no current map display)

**After**:
```tsx
function CurrentMap() {
  const { data: status, refetch } = useQuery({ queryKey: ['status'], queryFn: getStatus });
  const currentMap = status?.currentMap ?? "Unknown";
  
  return (
    <Card>
      <CardHeader className="flex flex-row justify-between items-center">
        <CardTitle>Current Map</CardTitle>
        <Button variant="ghost" size="icon" onClick={() => refetch()}>
          <RefreshIcon />
        </Button>
      </CardHeader>
      <CardContent>
        <p className="text-2xl font-bold">{currentMap}</p>
      </CardContent>
    </Card>
  );
}
```

**Tests**:
- Shows current map
- Refresh updates value
- Error state shows

---

### T5: Create Maps Page

**File**: `frontend/src/pages/Maps.tsx`

**What to do**:
1. Combine CurrentMap, MapGrid, and search
2. Add to main navigation
3. Responsive layout

**Before**: (no maps page)

**After**:
```tsx
function Maps() {
  const [selectedMap, setSelectedMap] = useState<MapInfo | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  
  return (
    <div className="space-y-6">
      <CurrentMap />
      <MapGrid onMapSelect={m => { setSelectedMap(m); setDialogOpen(true); }} />
      <MapDialog
        map={selectedMap}
        open={dialogOpen}
        onOpenChange={setDialogOpen}
      />
    </div>
  );
}
```

**Tests**:
- Page renders all components
- Navigation works
- Mobile layout stacks

---

### T6: Add Unit Tests

**File**: `frontend/src/components/Map*.test.tsx`

**What to do**:
1. Test MapGrid rendering
2. Test search filtering
3. Test dialog flow
4. Test error states

**Tests**:
- All tests pass
- ≥85% coverage on new components

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `frontend/src/components/MapGrid.tsx` | New component | P0 |
| `frontend/src/components/MapDialog.tsx` | New component | P0 |
| `frontend/src/components/CurrentMap.tsx` | New component | P0 |
| `frontend/src/pages/Maps.tsx` | New page | P0 |

---

## Open Questions / Risks

1. **Current Map Source**: Need to parse `/rcon status` output or add new endpoint. (Recommend: add `/status` endpoint in Plan 07)
2. **Large Map Lists**: 50+ maps may slow rendering. Consider virtualization if needed.
3. **Map Icons**: Optional enhancement - show map preview images if available.

---

## Verification Checklist

- [ ] T1: Map grid renders all available maps
- [ ] T2: Search filters maps correctly
- [ ] T3: Dialog confirms map change
- [ ] T4: Current map displays at top
- [ ] T5: Maps page accessible via navigation
- [ ] T6: Mobile layout (375px) usable
- [ ] T7: `npm test` passes with ≥85% coverage
- [ ] T8: `npm run lint` passes with zero warnings

---

## Next Steps

After Plan 05 completes:
- Plan 06: Player management (kick/ban)
- Plan 07: Real-time log streaming
- Plan 08: Server status endpoint
