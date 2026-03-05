# Plug Automation Standalone

This is a self-contained Docker container that automates plug control via Home Assistant API.

## Schedule

- **Mon 17:30** → ON
- **Tue 04:00** → OFF
- **Tue 06:00** → ON
- **Tue 08:00** → OFF
- **Wed 17:30** → ON
- **Thu 04:00** → OFF
- **Thu 06:00** → ON
- **Thu 08:00** → OFF
- **Fri 17:30** → ON
- **Sat 04:00** → OFF
- **Sat 06:00** → ON
- **Sat 08:00** → OFF
- **Sun 17:30** → ON

## CLI Usage

```bash
python main.py plug <on|off> [id] [options]
```

### Arguments

- `action`: Either `on` or `off`
- `id`: Plug identifier (default: `main`)

### Options

- `-u, --url`: Home Assistant URL (overrides HASS_URL)
- `-t, --token`: Home Assistant API token (overrides HASS_TOKEN)
- `-e, --entity`: Entity ID to control (overrides ENTITY_ID)

### Examples

```bash
# Toggle main plug on
python main.py plug on main

# Toggle bedroom plug off
python main.py plug off bedroom

# With custom Home Assistant URL
python main.py plug on --url http://homeassistant.local:8123

# Override all settings
python main.py plug on test --url http://custom.local:8123 --token mytoken --entity switch.test
```

## Required Environment Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `HASS_URL` | Home Assistant HTTP API URL | `http://homeassistant.local:8123` |
| `HASS_TOKEN` | Home Assistant API Token | `your_api_token_here` |
| `ENTITY_ID` | The switch entity to control | `switch.dehumidifierplug` |

## Crontab Schedule

The crontab file is installed at `/etc/cron.d/plug` and runs the `run_plug.sh` script which logs output to `/var/log/plug.log`.
