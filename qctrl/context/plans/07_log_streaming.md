# Plan 07 — Real-time Log Streaming

> **Status**: pending  
> **Created**: 2026-06-11  
> **Depends on**: Plan 01 (RCON client), Plan 03 (Frontend scaffolding)  
> **Goal**: Implement server log streaming to frontend via WebSocket or SSE  
> **Agent**: implementation  

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**  
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Stream server console output to frontend in real-time for monitoring.

**Deliverables**:
1. Backend log source (read from server.cfg or RCON polling)
2. WebSocket endpoint for bidirectional streaming
3. Frontend log viewer component with auto-scroll
4. Pause/resume controls
5. Filter by keyword (optional)
6. Unit tests

**Estimated effort**: Medium–Large (1–2 days)

---

## Context

### Requirements
- Real-time log streaming from server
- Auto-scroll to bottom
- Pause for reading
- Mobile-friendly terminal view
- Handle high-frequency log bursts

### Options for Log Source
1. **RCON Polling**: Periodically execute `status` or custom command (not ideal for logs)
2. **File Watching**: Watch server.log file if written to disk
3. **WebSocket from Server**: q2pro may not support this natively
4. **Polling Console**: Repeated `say` or custom command (hacky)

**Recommendation**: Since q2pro doesn't have native log streaming, use RCON polling with a "get recent logs" approach or file watching if server writes logs.

### Alternative Approach
If direct log streaming isn't feasible:
- Poll `status` command and show server state changes
- Show command execution results (what we send via RCON)
- Create a "command log" showing what actions were taken

---

## Step-by-Step Tasks

### T1: Research Log Source Options

**File**: `context/distilled.md` (add findings)

**What to do**:
1. Check `vendor/q2pro` for log file handling
2. Search for existing log streaming solutions for q2pro
3. Determine feasible approach:
   - File watching (if logs written to disk)
   - RCON polling (fallback)
   - Hybrid approach

**Before**: (no log source research)

**After**: Document findings in `context/distilled.md`

**Decision Points**:
- If file watching possible → use `notify` crate
- If only RCON → implement polling with deduplication
- If neither → implement command log only

---

### T2: Create Log Source Module (Backend)

**File**: `crates/api/src/logs.rs`

**What to do**:
1. Define `LogEntry` struct: `timestamp`, `level`, `message`
2. Implement log source trait:
   ```rust
   trait LogSource {
       fn subscribe(&self) -> mpsc::Receiver<LogEntry>;
   }
   ```
3. Implement FileWatcherLogSource (if file watching)
4. Implement PollingLogSource (fallback)
5. Add deduplication to avoid duplicate entries

**Before**: (no log source)

**After**:
```rust
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub message: String,
}

pub trait LogSource: Send + Sync {
    fn subscribe(&self) -> mpsc::Receiver<LogEntry>;
}
```

**Tests**:
- Test log entry parsing
- Test deduplication
- Test error handling

---

### T3: Add WebSocket Endpoint

**File**: `crates/api/src/routes.rs`, `crates/api/src/ws.rs`

**What to do**:
1. Add `GET /logs/ws` WebSocket endpoint
2. Use `axum::extract::ws` for WebSocket handling
3. Stream log entries from source to connected clients
4. Handle multiple concurrent connections
5. Clean up on disconnect

**Before**: (no WebSocket)

**After**:
```rust
#[axum::routing::get("/logs/ws")]
async fn logs_ws(ws: WebSocketUpgrade, State(source): State<LogSource>) -> Response {
    ws.on_upgrade(move |socket| handle_websocket(socket, source.subscribe()))
}

async fn handle_websocket(socket: WebSocket, mut rx: mpsc::Receiver<LogEntry>) {
    while let Some(entry) = rx.recv().await {
        socket.send(json!(entry)).await.ok();
    }
}
```

**Tests**:
- Test WebSocket connection
- Test message delivery
- Test disconnect handling

---

### T4: Create Log Viewer Component

**File**: `frontend/src/components/LogViewer.tsx`

**What to do**:
1. Connect to WebSocket
2. Display log entries in terminal-like view
3. Auto-scroll to bottom on new entries
4. Pause button to stop auto-scroll
5. Clear button to empty view
6. Entry count display

**Before**: (no log viewer)

**After**:
```tsx
function LogViewer() {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [paused, setPaused] = useState(false);
  const endRef = useRef<HTMLDivElement>(null);
  
  useEffect(() => {
    const ws = new WebSocket(`ws://${window.location.host}/logs/ws`);
    ws.onmessage = (event) => {
      const entry = JSON.parse(event.data);
      setLogs(prev => [...prev, entry].slice(-1000)); // Keep last 1000
    };
    return () => ws.close();
  }, []);
  
  useEffect(() => {
    if (!paused) endRef.current?.scrollIntoView();
  }, [logs, paused]);
  
  return (
    <div className="flex flex-col h-screen">
      <div className="flex justify-between items-center p-2 border-b">
        <span>{logs.length} entries</span>
        <Button onClick={() => setPaused(!paused)}>
          {paused ? "Resume" : "Pause"}
        </Button>
      </div>
      <div className="flex-1 overflow-y-auto font-mono text-sm p-2 bg-gray-900 text-green-400">
        {logs.map((log, i) => (
          <div key={i} className="whitespace-pre-wrap">
            {log.timestamp} [{log.level}] {log.message}
          </div>
        ))}
        <div ref={endRef} />
      </div>
    </div>
  );
}
```

**Tests**:
- Test WebSocket connection
- Test auto-scroll
- Test pause/resume

---

### T5: Add Log Filtering (Optional)

**File**: `frontend/src/components/LogViewer.tsx` (extend)

**What to do**:
1. Add search input for filtering
2. Show matching entries only
3. Highlight matches
4. Show "Showing X of Y" count

**Before**: (no filtering)

**After**:
```tsx
const [filter, setFilter] = useState("");
const filteredLogs = logs.filter(log => 
  log.message.toLowerCase().includes(filter.toLowerCase())
);

return (
  <>
    <Input placeholder="Filter..." value={filter} onChange={e => setFilter(e.target.value)} />
    <div>{filteredLogs.map(...)}</div>
  </>
);
```

**Tests**:
- Test filtering works
- Test highlight works

---

### T6: Create Logs Page

**File**: `frontend/src/pages/Logs.tsx`

**What to do**:
1. Full-screen log viewer
2. Add to main navigation
3. Mobile-optimized (full height, large text)

**Before**: (no logs page)

**After**:
```tsx
function Logs() {
  return (
    <div className="h-screen flex flex-col">
      <LogViewer />
    </div>
  );
}
```

**Tests**:
- Page renders
- Navigation works
- Mobile layout usable

---

### T7: Add Unit Tests

**File**: `crates/api/tests/logs_test.rs`, `frontend/src/components/LogViewer.test.tsx`

**What to do**:
1. Test log source implementation
2. Test WebSocket delivery
3. Test log viewer rendering
4. Test pause/resume

**Tests**:
- All tests pass
- ≥85% coverage on new code

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/api/src/logs.rs` | New module | P0 |
| `crates/api/src/ws.rs` | New module | P0 |
| `crates/api/src/routes.rs` | Add /logs/ws | P0 |
| `frontend/src/components/LogViewer.tsx` | New component | P0 |
| `frontend/src/pages/Logs.tsx` | New page | P0 |

---

## Open Questions / Risks

1. **Log Source**: q2pro may not write logs to file. Need to confirm.
2. **Performance**: High-frequency logs may overwhelm browser. Consider throttling.
3. **Memory**: Keep last N entries to avoid memory bloat.

---

## Verification Checklist

- [ ] T1: Log source research documented in `context/distilled.md`
- [ ] T2: Log source module implemented
- [ ] T3: WebSocket endpoint accepts connections
- [ ] T4: Log viewer displays entries
- [ ] T5: Auto-scroll works
- [ ] T6: Pause/resume works
- [ ] T7: Logs page accessible
- [ ] T8: Mobile layout usable (375px)
- [ ] T9: `npm test` passes with ≥85% coverage
- [ ] T10: `cargo test` passes with ≥85% coverage

---

## Next Steps

After Plan 07 completes:
- Plan 08: Server status dashboard (summary view)
- Plan 09: Settings persistence (save config changes)
- Plan 10: Final testing and polish
