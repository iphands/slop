# Plan 11 — Deployment Setup

> **Status**: pending  
> **Created**: 2026-06-11  
> **Depends on**: Plan 10 (Final testing)  
> **Goal**: Set up production deployment with systemd service and reverse proxy  
> **Agent**: implementation  

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**  
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Create deployment scripts and systemd service for production.

**Deliverables**:
1. systemd service file for API
2. nginx reverse proxy config
3. Build scripts for production
4. Environment configuration
5. Monitoring setup (optional)
6. Deployment documentation

**Estimated effort**: Small (2-3 hours)

---

## Context

### Target Environment
- Server: Linux (likely Debian/Ubuntu based on Podman usage)
- API runs on `localhost:3000`
- nginx/proxy exposes to public
- Config at `/etc/qctrl/config.yaml`

### Requirements
- Auto-start on boot
- Restart on failure
- Log rotation
- HTTPS support (optional)

---

## Step-by-Step Tasks

### T1: Create systemd Service

**File**: `deploy/qctrl.service`

**What to do**:
1. Define service unit
2. Set WorkingDirectory
3. Configure Environment for config path
4. Set Restart=always
5. Add After=network.target

**Before**: (no service)

**After**:
```ini
[Unit]
Description=qctrl - Quake 2 Server Controller
After=network.target

[Service]
Type=simple
User=iphands
WorkingDirectory=/opt/qctrl
ExecStart=/opt/qctrl/api
Environment=QCTRL_CONFIG=/etc/qctrl/config.yaml
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

**Verification**:
- [ ] `systemctl daemon-reload` succeeds
- [ ] `systemctl start qctrl` starts service
- [ ] `systemctl status qctrl` shows active

---

### T2: Create nginx Config

**File**: `deploy/nginx.conf`

**What to do**:
1. Set up reverse proxy to localhost:3000
2. Configure WebSocket support
3. Add security headers
4. Enable gzip compression

**Before**: (no proxy)

**After**:
```nginx
server {
    listen 80;
    server_name qctrl.example.com;

    location / {
        proxy_pass http://localhost:3000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

**Verification**:
- [ ] nginx config test passes
- [ ] Proxy responds correctly
- [ ] WebSocket upgrades work

---

### T3: Create Build Script

**File**: `deploy/build.sh`

**What to do**:
1. Build Rust binary in release mode
2. Build frontend
3. Copy to deploy directory
4. Set permissions

**Before**: (no build script)

**After**:
```bash
#!/bin/bash
set -e

echo "Building Rust backend..."
cargo build --release --bin api
cp target/release/api deploy/

echo "Building frontend..."
cd frontend
npm run build
cp -r dist/* ../deploy/frontend/
cd ..

echo "Setting permissions..."
chmod +x deploy/api
chmod -R 755 deploy/frontend/

echo "Build complete!"
```

**Verification**:
- [ ] Script runs without errors
- [ ] Deploy directory populated
- [ ] Binary executable

---

### T4: Create Config Template

**File**: `deploy/config.production.yaml`

**What to do**:
1. Template for production config
2. Document all environment variables
3. Add example values

**Before**: (no production config)

**After**:
```yaml
# qctrl production configuration
server:
  host: noir.lan
  port: 27910
  rcon_password: ${RCON_PASSWORD}  # From environment
paths:
  server_cfg: /mnt/noir/scratch/games/q2/baseq2/server.cfg
  baseq2: /mnt/noir/scratch/games/q2/baseq2
```

**Verification**:
- [ ] Config loads with env vars
- [ ] All fields documented

---

### T5: Add Log Rotation

**File**: `deploy/qctrl.logrotate`

**What to do**:
1. Configure log rotation
2. Daily rotation
3. Keep 7 days
4. Compress old logs

**Before**: (no log rotation)

**After**:
```
/var/log/qctrl/*.log {
    daily
    rotate 7
    compress
    delaycompress
    missingok
    notifempty
    create 0640 iphands iphands
}
```

**Verification**:
- [ ] Logrotate config valid
- [ ] Rotation works manually

---

### T6: Create Deployment Documentation

**File**: `DEPLOYMENT.md`

**What to do**:
1. Step-by-step deployment guide
2. Troubleshooting section
3. Rollback procedure

**Before**: (no deployment docs)

**After**:
```markdown
# Deployment Guide

## Prerequisites
- Rust 1.70+
- Node.js 18+
- systemd
- nginx (optional)

## Quick Deploy
1. Copy files to server
2. Configure `config.yaml`
3. Run `./deploy/build.sh`
4. Install systemd service
5. Start service

## Troubleshooting
- Service not starting: `journalctl -u qctrl`
- Port in use: `lsof -i :3000`
- Config error: Check YAML syntax
```

**Verification**:
- [ ] Docs complete
- [ ] Steps clear
- [ ] Troubleshooting helpful

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `deploy/qctrl.service` | New file | P0 |
| `deploy/nginx.conf` | New file | P0 |
| `deploy/build.sh` | New file | P0 |
| `deploy/config.production.yaml` | New file | P0 |
| `DEPLOYMENT.md` | New file | P0 |

---

## Verification Checklist

- [ ] T1: systemd service installs and starts
- [ ] T2: nginx proxy works
- [ ] T3: Build script creates deployable artifacts
- [ ] T4: Production config loads correctly
- [ ] T5: Log rotation configured
- [ ] T6: Deployment docs complete
- [ ] T7: Service auto-starts on boot

---

## Next Steps

After Plan 11 completes:
- Project complete!
- Tag v1.0 release
