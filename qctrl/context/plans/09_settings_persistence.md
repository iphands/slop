# Plan 09 — Settings Persistence

> **Status**: pending  
> **Created**: 2026-06-11  
> **Depends on**: Plan 01 (Config loading)  
> **Goal**: Allow users to save and load server configuration changes  
> **Agent**: implementation  

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**  
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Persist server.cfg changes and provide UI for configuration management.

**Deliverables**:
1. Backend endpoint to write server.cfg
2. UI to edit and save configuration
3. Config backup/restore functionality
4. Validation for config values
5. Unit tests

**Estimated effort**: Medium (1 day)

---

## Context

### Requirements
- Edit server.cfg from frontend
- Persist changes to disk
- Backup before writing
- Validate values before saving
- Show current config values

### Current Server Config
```
set hostname "HandsNet deathmatch"
set rcon_password ace123
set deathmatch 1
set coop 0
set skill 3
set maxclients 25
set sv_uptime 1
set sv_downloadserver http://home.ahands.org:81/quakeii/
set allow_download "1"
set allow_download_players "1"
set allow_download_models "1"
set allow_download_sounds "1"
set allow_download_maps "1"
set allow_download_textures "1"
set allow_download_others "1"
set port 27910
set dmflags 17424
map q2dm1
```

### RCON Commands for Config
- `set <var> <value>` - Set cvar
- Server reads config on restart or `exec server.cfg`

---

## Step-by-Step Tasks

### T1: Add Config Write Endpoint (Backend)

**File**: `crates/api/src/config.rs`, `crates/api/src/routes.rs`

**What to do**:
1. Add `PUT /config` endpoint
2. Accept `ConfigUpdate` payload
3. Backup current server.cfg before writing
4. Write new config to disk
5. Return success/error

**Before**:
```rust
// Only has GET /config
```

**After**:
```rust
#[axum::routing::put("/config")]
async fn update_config(
    State(state): State<ConfigState>,
    Json(update): Json<ConfigUpdate>
) -> Result<Json<ConfigResponse>, ApiError> {
    // Backup current file
    // Write new config
    // Return result
}
```

**Tests**:
- Test backup creation
- Test config write
- Test error handling

---

### T2: Create Config Editor Component

**File**: `frontend/src/components/ConfigEditor.tsx`

**What to do**:
1. Display editable config fields
2. Input for each cvar (hostname, skill, maxclients, etc.)
3. Save button with confirmation
4. Show current values from server.cfg

**Before**: (no config editor)

**After**:
```tsx
function ConfigEditor() {
  const { data: config } = useQuery({ queryKey: ['config'], queryFn: getConfig });
  const { mutate: save, isPending } = useMutation({ mutationFn: saveConfig });
  
  return (
    <form onSubmit={e => { e.preventDefault(); save(formData); }}>
      <Input label="Hostname" value={config?.hostname} />
      <Input label="Skill" type="number" value={config?.skill} />
      <Input label="Max Clients" type="number" value={config?.maxclients} />
      <Button type="submit" disabled={isPending}>
        {isPending ? "Saving..." : "Save Changes"}
      </Button>
    </form>
  );
}
```

**Tests**:
- Form displays current values
- Save sends correct payload
- Validation works

---

### T3: Add Config Validation

**File**: `frontend/src/lib/validators.ts`

**What to do**:
1. Validate hostname (max 32 chars)
2. Validate skill (0-4)
3. Validate maxclients (1-256)
4. Validate port (1024-65535)
5. Show inline errors

**Before**: (no validation)

**After**:
```typescript
export function validateConfig(config: Config): ConfigError[] {
  const errors: ConfigError[] = [];
  
  if (config.hostname.length > 32) {
    errors.push({ field: 'hostname', message: 'Max 32 characters' });
  }
  
  if (config.skill < 0 || config.skill > 4) {
    errors.push({ field: 'skill', message: 'Must be 0-4' });
  }
  
  return errors;
}
```

**Tests**:
- Test valid config passes
- Test invalid config fails
- Test error messages display

---

### T4: Add Backup/Restore

**File**: `crates/api/src/config.rs`

**What to do**:
1. Create backup before write: `server.cfg.backup`
2. Add `POST /config/backup` to create manual backup
3. Add `POST /config/restore` to restore from backup
4. List available backups

**Before**: (no backup)

**After**:
```rust
async fn backup_config(path: &str) -> Result<String, Error> {
    let backup_path = format!("{}.backup", path);
    fs::copy(path, &backup_path)?;
    Ok(backup_path)
}
```

**Tests**:
- Test backup creation
- Test restore works
- Test backup list

---

### T5: Create Settings Page

**File**: `frontend/src/pages/Settings.tsx`

**What to do**:
1. Combine config editor with backup controls
2. Add to navigation
3. Show backup history

**Before**: (no settings page)

**After**:
```tsx
function Settings() {
  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold">Settings</h1>
      <ConfigEditor />
      <BackupControls />
    </div>
  );
}
```

**Tests**:
- Page renders
- Navigation works
- Backup/restore works

---

### T6: Add Unit Tests

**File**: `crates/api/tests/config_test.rs`, `frontend/src/components/ConfigEditor.test.tsx`

**What to do**:
1. Test config write
2. Test backup creation
3. Test validation
4. Test restore

**Tests**:
- All tests pass
- ≥85% coverage on new code

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/api/src/config.rs` | Add write/backup | P0 |
| `crates/api/src/routes.rs` | Add /config PUT | P0 |
| `frontend/src/components/ConfigEditor.tsx` | New component | P0 |
| `frontend/src/lib/validators.ts` | New module | P0 |
| `frontend/src/pages/Settings.tsx` | New page | P0 |

---

## Open Questions / Risks

1. **File Permissions**: Server may run as different user. Handle permission errors.
2. **Concurrent Writes**: Multiple saves may conflict. Consider locking.
3. **Config Format**: server.cfg syntax may be strict. Validate format.

---

## Verification Checklist

- [ ] T1: Config write endpoint works
- [ ] T2: Config editor displays and saves
- [ ] T3: Validation catches invalid values
- [ ] T4: Backup creates before write
- [ ] T5: Settings page accessible
- [ ] T6: `npm test` passes with ≥85% coverage
- [ ] T7: `cargo test` passes with ≥85% coverage

---

## Next Steps

After Plan 09 completes:
- Plan 10: Final testing and polish
- Plan 11: Deployment setup
