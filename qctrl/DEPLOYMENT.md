# qctrl Deployment Guide

## Prerequisites

- Rust 1.70+
- Node.js 18+
- systemd
- nginx (optional, for reverse proxy)

## Quick Deploy

### 1. Build

```bash
cd /home/iphands/prog/slop/qctrl
./deploy/build.sh
```

### 2. Configure

Copy the production config:
```bash
sudo mkdir -p /etc/qctrl
sudo cp deploy/config.production.yaml /etc/qctrl/config.yaml
sudo nano /etc/qctrl/config.yaml  # Edit with your settings
```

### 3. Install Service

```bash
sudo cp deploy/qctrl.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable qctrl
sudo systemctl start qctrl
```

### 4. Verify

```bash
sudo systemctl status qctrl
curl http://localhost:3000/health
```

## nginx Setup (Optional)

```bash
sudo cp deploy/nginx.conf /etc/nginx/sites-available/qctrl
sudo ln -s /etc/nginx/sites-available/qctrl /etc/nginx/sites-enabled/
sudo nginx -t
sudo systemctl reload nginx
```

## Troubleshooting

### Service not starting
```bash
sudo journalctl -u qctrl -f
```

### Port in use
```bash
sudo lsof -i :3000
```

### Config errors
Check YAML syntax:
```bash
python3 -c "import yaml; yaml.safe_load(open('/etc/qctrl/config.yaml'))"
```

## Rollback

To rollback to a previous version:
```bash
sudo systemctl stop qctrl
# Replace binary with previous version
sudo systemctl start qctrl
```

## Logs

Logs are available via journalctl:
```bash
sudo journalctl -u qctrl -f
```
