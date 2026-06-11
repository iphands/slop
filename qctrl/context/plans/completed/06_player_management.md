# Plan 06 — Player Management (Kick/Ban)

> **Status**: pending  
> **Created**: 2026-06-11  
> **Depends on**: Plan 02 (Map listing), Plan 07 (Server status - parsing)  
> **Goal**: Implement player list display with kick and ban functionality  
> **Agent**: implementation  

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**  
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Display connected players with kick/ban controls using `status` command output.

**Deliverables**:
1. Player list showing: name, score, IP, ping
2. Kick button per player (by name)
3. Ban button per player (by client number)
4. Auto-refresh player list
5. Confirmation dialogs for destructive actions
6. Unit tests

**Estimated effort**: Medium (1 day)

---

## Context

### Requirements
- Show all connected players
- Kick by name: `kick <player_name>`
- Ban by client number: `clientkick <num>` (more reliable than name)
- Auto-refresh list every 10 seconds
- Mobile-friendly touch targets

### RCON Commands
- `status` - Returns player list with format:
  ```
  num score address           name            ping
   0    15   192.168.1.100:27  PlayerName     45
   1     8   192.168.1.101:27  AnotherPlayer  78
  ```
- `kick <name>` - Remove player by name
- `clientkick <num>` - Ban player by client number

### API Endpoint
- `GET /status` - Parse and return player list
- `POST /rcon/execute` - Send kick/ban commands

---

## Step-by-Step Tasks

### T1: Create Player List Type

**File**: `frontend/src/lib/types.ts`

**What to do**:
1. Define `Player` interface
2. Add parsing function for `status` output
3. Add API type definitions

**Before**: (no player types)

**After**:
```typescript
export interface Player {
  clientNum: number;  // For clientkick
  score: number;
  address: string;    // IP:port
  name: string;
  ping: number;
}

export function parseStatusOutput(output: string): Player[] {
  // Parse "num score address name ping" format
  // Return Player[]
}
```

**Tests**:
- Test parsing valid status output
- Test handling empty list
- Test handling malformed lines

---

### T2: Add Server Status Endpoint (Backend)

**File**: `crates/api/src/routes.rs`

**What to do**:
1. Add `GET /status` endpoint
2. Execute `status` command via RCON
3. Parse output into structured JSON
4. Return `Player[]`

**Before**:
```rust
// Only has /health, /config, /maps, /rcon/execute
```

**After**:
```rust
#[axum::routing::get("/status")]
async fn get_status(State(client): State<RconClient>) -> Result<Json<PlayerList>, ApiError> {
    let output = client.execute("status").await?;
    let players = parse_status_output(&output)?;
    Ok(Json(PlayerList { players }))
}
```

**Tests**:
- Test parsing real status output
- Test empty server returns empty list
- Test error handling

---

### T3: Create Player List Component

**File**: `frontend/src/components/PlayerList.tsx`

**What to do**:
1. Fetch `/status` with auto-refresh (10s)
2. Display as table/list with columns:
   - Name (primary)
   - Score
   - Ping
   - Actions (kick/ban buttons)
3. Sort by score (descending)
4. Show "No players connected" when empty

**Before**: (no player UI)

**After**:
```tsx
function PlayerList() {
  const { data: players, isLoading, refetch } = useQuery({
    queryKey: ['status'],
    queryFn: getStatus,
    refetchInterval: 10000,
  });
  
  if (isLoading) return <LoadingSpinner />;
  if (!players?.length) return <p className="text-gray-500">No players connected</p>;
  
  return (
    <div className="space-y-2">
      {players
        .sort((a, b) => b.score - a.score)
        .map(player => (
          <PlayerRow key={player.clientNum} player={player} />
        ))}
    </div>
  );
}
```

**Tests**:
- List renders players correctly
- Sorting works
- Empty state shows

---

### T4: Create Player Row Component

**File**: `frontend/src/components/PlayerRow.tsx`

**What to do**:
1. Display player info (name, score, ping)
2. Kick button (sends `kick <name>`)
3. Ban button (sends `clientkick <num>`)
4. Loading state per action
5. Success/error feedback

**Before**: (no player row)

**After**:
```tsx
function PlayerRow({ player }: { player: Player }) {
  const { mutate: kick, isPending: kicking } = useMutation({ mutationFn: executeRcon });
  const { mutate: ban, isPending: banning } = useMutation({ mutationFn: executeRcon });
  
  return (
    <Card>
      <CardContent className="flex justify-between items-center">
        <div>
          <p className="font-medium">{player.name}</p>
          <p className="text-sm text-gray-500">
            Score: {player.score} | Ping: {player.ping}ms
          </p>
        </div>
        <div className="flex gap-2">
          <Button
            variant="destructive"
            size="sm"
            onClick={() => kick(`kick ${player.name}`)}
            disabled={kicking}
          >
            {kicking ? "..." : "Kick"}
          </Button>
          <Button
            variant="destructive"
            size="sm"
            onClick={() => ban(`clientkick ${player.clientNum}`)}
            disabled={banning}
          >
            {banning ? "..." : "Ban"}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}
```

**Tests**:
- Row displays player info
- Kick sends correct command
- Ban sends correct command

---

### T5: Add Confirmation Dialogs

**File**: `frontend/src/components/PlayerActions.tsx`

**What to do**:
1. Kick confirmation: "Kick <player>?"
2. Ban confirmation: "Ban <player> (client #<num>)?"
3. Show warning for ban (harder to undo)
4. Cancel option

**Before**: (no confirmation)

**After**:
```tsx
function KickDialog({ player, open, onOpenChange }: ...) {
  const { mutate: kick, isPending } = useMutation({ mutationFn: executeRcon });
  
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>Kick Player?</DialogHeader>
        <p>Are you sure you want to kick <strong>{player.name}</strong>?</p>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button 
            variant="destructive" 
            onClick={() => kick(`kick ${player.name}`, { onSuccess: () => onOpenChange(false) })}
            disabled={isPending}
          >
            {isPending ? "Kicking..." : "Kick"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
```

**Tests**:
- Dialog opens on button click
- Cancel closes dialog
- Kick sends command and closes

---

### T6: Create Players Page

**File**: `frontend/src/pages/Players.tsx`

**What to do**:
1. Combine PlayerList with header
2. Add manual refresh button
3. Show player count
4. Add to main navigation

**Before**: (no players page)

**After**:
```tsx
function Players() {
  const { data: players } = useQuery({ queryKey: ['status'], queryFn: getStatus });
  
  return (
    <div className="space-y-4">
      <div className="flex justify-between items-center">
        <h2 className="text-xl font-bold">
          Players ({players?.length ?? 0})
        </h2>
        <Button variant="outline" onClick={() => refetch()}>
          Refresh
        </Button>
      </div>
      <PlayerList />
    </div>
  );
}
```

**Tests**:
- Page renders correctly
- Player count updates
- Refresh button works

---

### T7: Add Unit Tests

**File**: `frontend/src/components/Player*.test.tsx`, `crates/api/tests/status_test.rs`

**What to do**:
1. Test status parsing (backend)
2. Test player list rendering
3. Test kick/ban command generation
4. Test dialog flow

**Tests**:
- All tests pass
- ≥85% coverage on new code

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/api/src/routes.rs` | Add /status endpoint | P0 |
| `crates/api/src/status.rs` | New module for parsing | P0 |
| `frontend/src/lib/types.ts` | Add Player type | P0 |
| `frontend/src/components/PlayerList.tsx` | New component | P0 |
| `frontend/src/components/PlayerRow.tsx` | New component | P0 |
| `frontend/src/pages/Players.tsx` | New page | P0 |

---

## Open Questions / Risks

1. **Status Parsing**: `status` output format may vary. Need robust parser.
2. **Name Conflicts**: Multiple players with same name? Use clientNum for ban.
3. **Auto-refresh**: 10s may be too frequent. Consider user preference.

---

## Verification Checklist

- [ ] T1: Status parsing handles real server output
- [ ] T2: Player list displays all connected players
- [ ] T3: Kick button sends correct command
- [ ] T4: Ban button sends correct command
- [ ] T5: Confirmation dialogs work
- [ ] T6: Auto-refresh updates list
- [ ] T7: Mobile layout usable (375px)
- [ ] T8: `npm test` passes with ≥85% coverage
- [ ] T9: `cargo test` passes with ≥85% coverage

---

## Next Steps

After Plan 06 completes:
- Plan 07: Real-time log streaming (WebSocket/SSE)
- Plan 08: Server status dashboard
