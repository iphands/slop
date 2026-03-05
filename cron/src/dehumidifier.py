#!/usr/bin/env python3
"""
Standalone Dehumidifier Automation Script

This script runs via cron on a separate machine.
It does NOT require Agent Zero or an LLM.
It contains the schedule logic directly.

Schedule:
- Monday 17:30 -> ON
- Tuesday 04:00 -> OFF, 06:00 -> ON, 08:00 -> OFF
- Wednesday 17:30 -> ON
- Thursday 04:00 -> OFF, 06:00 -> ON, 08:00 -> OFF
- Friday 17:30 -> ON
- Saturday 04:00 -> OFF, 06:00 -> ON, 08:00 -> OFF
- Sunday 17:30 -> ON
"""

import os
import sys
from datetime import datetime
import requests

# Configuration - Read from environment variables
HASS_URL: str = os.getenv("HASS_URL", "http://homeassistant.local:8123")
HASS_TOKEN: str = os.getenv("HASS_TOKEN", "")
ENTITY_ID: str = os.getenv("ENTITY_ID", "switch.dehumidifierplug")

# Validate configuration
if not HASS_TOKEN:
    print("ERROR: HASS_TOKEN environment variable is not set!")
    print("Please set HASS_TOKEN when running the container.")
    sys.exit(1)

if not ENTITY_ID:
    print("ERROR: ENTITY_ID environment variable is not set!")
    print("Please set ENTITY_ID when running the container.")
    sys.exit(1)

if not HASS_URL:
    print("ERROR: HASS_URL environment variable is not set!")
    print("Please set HASS_URL when running the container.")
    sys.exit(1)

# Schedule data: {weekday: [(hour, minute, action), ...]}
# weekday: 0=Mon, 1=Tue, ..., 6=Sun
SCHEDULE = {
    0: [(17, 30, "on")],  # Monday
    1: [(4, 0, "off"), (6, 0, "on"), (8, 0, "off")],  # Tuesday
    2: [(17, 30, "on")],  # Wednesday
    3: [(4, 0, "off"), (6, 0, "on"), (8, 0, "off")],  # Thursday
    4: [(17, 30, "on")],  # Friday
    5: [(4, 0, "off"), (6, 0, "on"), (8, 0, "off")],  # Saturday
    6: [(17, 30, "on")],  # Sunday
}


def get_schedule_for_day(dt: datetime) -> list[tuple[datetime, str]]:
    """
    Returns a list of (datetime, action) tuples for the current day.
    Action is either "on" or "off".
    """
    day_of_week = dt.weekday()
    schedule = []

    for hour, minute, action in SCHEDULE.get(day_of_week, []):
        scheduled_time = dt.replace(hour=hour, minute=minute, second=0, microsecond=0)
        schedule.append((scheduled_time, action))

    return schedule


def execute_action(action: str) -> bool:
    """Execute the action on Home Assistant."""
    try:
        # Use correct endpoint based on action (not toggle!)
        service = "turn_on" if action.lower() == "on" else "turn_off"
        url = f"{HASS_URL}/api/services/switch/{service}"
        headers = {"Authorization": f"Bearer {HASS_TOKEN}"}
        payload = {"entity_id": ENTITY_ID}

        response = requests.post(url, json=payload, headers=headers, timeout=10)

        if response.status_code == 200:
            print(f"Successfully set {ENTITY_ID} -> {action.upper()}")
            return True
        else:
            print(
                f"Failed to set {ENTITY_ID}: {response.status_code} - {response.text}"
            )
            return False

    except Exception as e:
        print(f"Error executing action: {e}")
        return False


def main() -> None:
    now = datetime.now()
    print(f"[INFO] Current time: {now.strftime('%Y-%m-%d %H:%M:%S')}")
    print(f"[INFO] Day of week: {now.strftime('%A')} (weekday={now.weekday()})")

    schedule = get_schedule_for_day(now)

    # Find the next scheduled action that is in the future or exactly now
    next_action = None
    for scheduled_time, action in schedule:
        if scheduled_time >= now:
            next_action = (scheduled_time, action)
            break

    if not next_action:
        print("[INFO] No upcoming scheduled actions for today.")
        return

    scheduled_time, action = next_action
    print(f"[INFO] Next action: {action.upper()} at {scheduled_time.strftime('%H:%M')}")
    print(f"[INFO] Entity: {ENTITY_ID}")
    print(f"[INFO] Home Assistant URL: {HASS_URL}")

    # Execute the action
    execute_action(action)


if __name__ == "__main__":
    main()
