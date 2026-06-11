# Plan 01 — Project Scaffolding & Config System

> **Status**: pending  
> **Created**: 2026-06-11  
> **Depends on**: N/A  
> **Goal**: Set up Rust workspace with REST API, config loading, and basic project structure  
> **Agent**: implementation  

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**  
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Initialize the qctrl project with Rust workspace, backend API skeleton, and config system.

**Deliverables**:
1. Rust workspace with `api` and `rcon` crates
2. Config YAML loading with `serde`
3. Basic REST endpoint to verify server connectivity
4. `config.defaults.yaml` with documented fields
5. All tests passing with ≥85% coverage

**Estimated effort**: Medium (1 day)

---

## Context

### Project Requirements
- Backend: Rust REST API using `axum` or `actix-web`
- Frontend: React TypeScript with Vite (Plan 02)
- Config: YAML file pointing to `server.cfg` path and `baseq2` directory
- RCON target: `noir.lan:27910` (live server)
- Game path: `/mnt/noir/scratch/games/q2/baseq2`

### Key Facts from Vendor
- **RCON Command Format**: `rcon <password> <command>` (see `vendor/quakeiicom.html`)
- **Key Variables**: `rcon_password`, `rcon_address`
- **Server**: Running q2pro on `noir.lan:27910/udp` with password `ace123`

### Config Structure
```yaml
server:
  host: noir.lan
  port: 27910
  rcon_password: ace123
paths:
  server_cfg: /mnt/noir/scratch/games/q2/baseq2/server.cfg
  baseq2: /mnt/noir/scratch/games/q2/baseq2
```

---

## Step-by-Step Tasks

### T1: Initialize Rust Workspace Structure

**File**: `Cargo.toml`, `crates/api/Cargo.toml`, `crates/rcon/Cargo.toml`

**What to do**:
1. Create workspace `Cargo.toml` with members: `api`, `rcon`
2. Initialize `crates/api` with `axum`, `tokio`, `serde`, `serde_yaml`
3. Initialize `crates/rcon` with `tokio`, `bytes`
4. Add dev dependencies: `tokio-test`, `assert_cmd`

**Before**: (empty project)

**After**:
```toml
# Cargo.toml
[workspace]
members = ["crates/api", "crates/rcon"]
resolver = "2"

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_yaml = "0.9"
axum = "0.7"
```

---

### T2: Create Config Module

**File**: `crates/api/src/config.rs`

**What to do**:
1. Define `ServerConfig` struct with `host`, `port`, `rcon_password`
2. Define `PathsConfig` struct with `server_cfg`, `baseq2`
3. Define `Config` struct combining both
4. Implement `Config::load(path: &str) -> Result<Config>`
5. Implement `Config::default()` loading from `config.defaults.yaml`

**Before**: (no code)

**After**:
```rust
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub rcon_password: String,
}

pub struct PathsConfig {
    pub server_cfg: String,
    pub baseq2: String,
}

pub struct Config {
    pub server: ServerConfig,
    pub paths: PathsConfig,
}

impl Config {
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> { ... }
}
```

**Tests**:
- Test loading valid config
- Test missing file error
- Test invalid YAML error

---

### T3: Create Basic RCON Client Skeleton

**File**: `crates/rcon/src/lib.rs`

**What to do**:
1. Define `RconClient` struct with `host`, `port`, `password`
2. Implement `RconClient::new(host, port, password)`
3. Implement `RconClient::execute(command: &str) -> Result<String>`
4. Use UDP socket (Quake 2 RCON uses UDP on port 27910)
5. Add basic timeout (5 seconds)

**Before**: (no code)

**After**:
```rust
pub struct RconClient {
    host: String,
    port: u16,
    password: String,
}

impl RconClient {
    pub fn new(host: &str, port: u16, password: &str) -> Self { ... }
    pub async fn execute(&self, command: &str) -> Result<String, RconError> { ... }
}
```

**Tests**:
- Test connection to live server (integration test with `noir.lan:27910`)
- Test invalid password returns error
- Test command execution returns output

---

### T4: Create Basic REST API

**File**: `crates/api/src/main.rs`, `crates/api/src/routes.rs`

**What to do**:
1. Set up `axum` server on `0.0.0.0:3000`
2. Create `/health` endpoint returning `{"status": "ok"}`
3. Create `/config` endpoint returning current config
4. Create `/rcon/execute` POST endpoint accepting `{"command": "..."}`
5. Load config from `config.yaml` on startup

**Before**: (no code)

**After**:
```rust
#[axum::routing::get("/health")]
async fn health() -> Json<Value> { ... }

#[axum::routing::post("/rcon/execute")]
async fn rcon_execute(
    State(client): State<RconClient>,
    Json(payload): Json<ExecutePayload>
) -> Result<Json<ExecuteResponse>, ApiError> { ... }
```

**Tests**:
- Test `/health` returns 200
- Test `/config` returns loaded config
- Test `/rcon/execute` with valid command

---

### T5: Create Config Defaults File

**File**: `config.defaults.yaml`

**What to do**:
1. Create reference config with documented fields
2. Include example paths for the live server
3. Add comments explaining each field

**Content**:
```yaml
# qctrl configuration
server:
  host: noir.lan        # RCON server hostname
  port: 27910           # RCON UDP port
  rcon_password: ace123 # Server RCON password
paths:
  server_cfg: /mnt/noir/scratch/games/q2/baseq2/server.cfg
  baseq2: /mnt/noir/scratch/games/q2/baseq2
```

---

### T6: Add Unit Tests & Verify Coverage

**What to do**:
1. Write unit tests for config loading
2. Write unit tests for RCON client (mock socket)
3. Write integration tests for API endpoints
4. Run `cargo test -- --test-threads=1`
5. Verify ≥85% coverage with `cargo tarpaulin` (if available) or manual assessment

**Verification**:
- All tests pass
- No clippy warnings
- Build succeeds

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `Cargo.toml` | Workspace setup | P0 |
| `crates/api/Cargo.toml` | API dependencies | P0 |
| `crates/rcon/Cargo.toml` | RCON dependencies | P0 |
| `crates/api/src/config.rs` | Config loading | P0 |
| `crates/rcon/src/lib.rs` | RCON client | P0 |
| `crates/api/src/main.rs` | API entry point | P0 |
| `config.defaults.yaml` | Reference config | P1 |

---

## Open Questions / Risks

1. **UDP vs TCP**: Quake 2 RCON typically uses UDP. Confirm packet format from `vendor/q2pro/src/cl_console.c`
2. **Live Server Access**: Integration tests require network access to `noir.lan:27910`. May need mock for CI.
3. **Config Path**: Determine default config location (`./config.yaml` vs `~/.qctrl/config.yaml`)

---

## Verification Checklist

- [ ] T1: `cargo build` succeeds with no warnings
- [ ] T2: Config loads from valid YAML file
- [ ] T3: Config returns error for missing/invalid file
- [ ] T4: RCON client connects to `noir.lan:27910`
- [ ] T5: RCON client rejects wrong password
- [ ] T6: `/health` endpoint returns 200
- [ ] T7: `/rcon/execute` executes `status` command and returns output
- [ ] T8: `cargo clippy` passes with zero warnings
- [ ] T9: All unit tests pass (`cargo test`)
- [ ] T10: ≥85% test coverage on touched modules

---

## Next Steps

After Plan 01 completes:
- Plan 02: Map listing API endpoint
- Plan 03: Frontend scaffolding (React + TypeScript)
