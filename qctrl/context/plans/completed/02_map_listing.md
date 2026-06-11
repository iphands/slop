# Plan 02 — Map Listing API Endpoint

> **Status**: pending  
> **Created**: 2026-06-11  
> **Depends on**: Plan 01  
> **Goal**: Implement API endpoint to list available maps from baseq2/maps/ directory  
> **Agent**: implementation  

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**  
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Add endpoint to scan and return list of `.bsp` map files from the server's baseq2/maps directory.

**Deliverables**:
1. `GET /maps` endpoint returning JSON array of map names
2. File scanner module in `rcon` crate (or new `server` crate)
3. Cache mechanism to avoid repeated filesystem scans
4. Unit tests with mock filesystem

**Estimated effort**: Small–Medium (half day)

---

## Context

### Requirement
Frontend needs to display a dropdown/grid of available maps for the user to select. Maps are stored as `.bsp` files in `/mnt/noir/scratch/games/q2/baseq2/maps/`.

### Key Facts
- Map files: `*.bsp` (e.g., `q2dm1.bsp`, `007_facility.bsp`)
- Display name: filename without `.bsp` extension
- Directory: configured via `paths.baseq2` in config

### API Response Format
```json
{
  "maps": [
    {"name": "q2dm1", "filename": "q2dm1.bsp", "size": 1234567},
    {"name": "007_facility", "filename": "007_facility.bsp", "size": 2345678}
  ]
}
```

---

## Step-by-Step Tasks

### T1: Create Map Scanner Module

**File**: `crates/api/src/maps.rs`

**What to do**:
1. Define `MapInfo` struct with `name`, `filename`, `size`, `modified`
2. Implement `scan_maps(baseq2_path: &str) -> Result<Vec<MapInfo>>`
3. Filter for `.bsp` files only
4. Sort alphabetically by name
5. Handle errors: directory not found, permission denied

**Before**: (no code)

**After**:
```rust
pub struct MapInfo {
    pub name: String,      // "q2dm1"
    pub filename: String,  // "q2dm1.bsp"
    pub size: u64,
    pub modified: SystemTime,
}

pub fn scan_maps(baseq2_path: &str) -> Result<Vec<MapInfo>, MapError> {
    let maps_dir = Path::new(baseq2_path).join("maps");
    // scan and return Vec<MapInfo>
}
```

**Tests**:
- Test scanning valid maps directory
- Test empty directory returns empty list
- Test non-existent directory returns error

---

### T2: Add Map Cache

**File**: `crates/api/src/maps.rs` (extend)

**What to do**:
1. Add `MapCache` struct with `maps: Vec<MapInfo>`, `last_updated: Instant`
2. Implement `refresh_if_stale(&mut self, max_age: Duration)`
3. Use 5-minute cache expiry to avoid repeated scans
4. Thread-safe access with `Mutex` or `RwLock`

**Before**: (no cache)

**After**:
```rust
pub struct MapCache {
    maps: Mutex<Option<(Vec<MapInfo>, Instant)>>,
    baseq2_path: String,
}

impl MapCache {
    pub fn new(baseq2_path: &str) -> Self { ... }
    pub fn get_maps(&self) -> Result<Vec<MapInfo>, MapError> { ... }
}
```

---

### T3: Add `/maps` Endpoint

**File**: `crates/api/src/routes.rs`

**What to do**:
1. Add `GET /maps` route
2. Return `200` with JSON list of maps
3. Return `500` if scan fails
4. Inject `MapCache` via `axum::State`

**Before**:
```rust
// Only has /health, /config, /rcon/execute
```

**After**:
```rust
#[axum::routing::get("/maps")]
async fn list_maps(State(cache): State<MapCache>) -> Result<Json<MapList>, ApiError> {
    let maps = cache.get_maps()?;
    Ok(Json(MapList { maps }))
}
```

**Tests**:
- Test endpoint returns 200 with map list
- Test endpoint handles scan errors gracefully

---

### T4: Integration Test with Real Server

**File**: `crates/api/tests/integration/maps_test.rs`

**What to do**:
1. Start API server in test mode
2. Call `/maps` endpoint
3. Verify response contains expected maps from `/mnt/noir/scratch/games/q2/baseq2/maps/`
4. Verify map names match filesystem

**Tests**:
- Test real filesystem integration
- Verify at least 5 maps found (from user's listing)

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/api/src/maps.rs` | New module | P0 |
| `crates/api/src/routes.rs` | Add /maps route | P0 |
| `crates/api/Cargo.toml` | Add `walkdir` or `glob` | P1 |
| `crates/api/tests/integration/maps_test.rs` | New test file | P1 |

---

## Open Questions / Risks

1. **Symlinks**: Should we follow symlinks in maps directory? (Default: no)
2. **Performance**: Large maps directories may slow initial scan. Consider async scan.
3. **Permissions**: Server may run as different user. Handle permission errors gracefully.

---

## Verification Checklist

- [ ] T1: `scan_maps()` returns correct list from test directory
- [ ] T2: Cache refreshes after 5 minutes
- [ ] T3: `/maps` returns 200 with JSON array
- [ ] T4: Real integration test finds ≥5 maps
- [ ] T5: `cargo clippy` passes with zero warnings
- [ ] T6: All tests pass with ≥85% coverage

---

## Next Steps

After Plan 02 completes:
- Plan 03: Frontend scaffolding (React + TypeScript)
- Plan 04: Deathmatch controls UI (dmflags, timelimit, fraglimit)
