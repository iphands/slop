#!/usr/bin/env python3
"""
Plug control module for Home Assistant API integration.
Handles toggling plugs on/off via Home Assistant REST API.
"""

import logging
import os
import sys
import requests

logging.basicConfig(
    level=logging.INFO, format="%(asctime)s - %(levelname)s - %(message)s"
)
logger: logging.Logger = logging.getLogger(__name__)

HASS_URL: str = os.getenv("HASS_URL", "http://homeassistant.local:8123")
HASS_TOKEN: str = os.getenv("HASS_TOKEN", "")
ENTITY_ID: str = os.getenv("ENTITY_ID", "switch.dehumidifierplug")


def toggle(
    action: str,
    id: str = "main",
    url: str | None = None,
    token: str | None = None,
    entity: str | None = None,
) -> bool:
    """
    Toggle a plug on or off via Home Assistant API.

    Args:
        action: Either "on" or "off"
        id: Device/plug identifier (used for logging)
        url: Home Assistant URL (optional, overrides environment)
        token: Home Assistant API token (optional, overrides environment)
        entity: Entity ID to control (optional, overrides environment)

    Returns:
        True if the toggle was successful, False otherwise
    """
    # Use local variables instead of mutating globals
    actual_url = url or HASS_URL
    actual_token = token or HASS_TOKEN
    actual_entity = entity or ENTITY_ID

    # Validate config using local values
    if not actual_token:
        logger.error("HASS_TOKEN environment variable is not set!")
        logger.error("Please set HASS_TOKEN when running the container.")
        sys.exit(1)

    if not actual_entity:
        logger.error("ENTITY_ID environment variable is not set!")
        logger.error("Please set ENTITY_ID when running the container.")
        sys.exit(1)

    if not actual_url:
        logger.error("HASS_URL environment variable is not set!")
        logger.error("Please set HASS_URL when running the container.")
        sys.exit(1)

    try:
        # Use correct endpoint based on action (not toggle!)
        service = "turn_on" if action.lower() == "on" else "turn_off"
        toggle_url = f"{actual_url}/api/services/switch/{service}"
        headers = {"Authorization": f"Bearer {actual_token}"}
        payload = {"entity_id": actual_entity}

        logger.info(f"Setting {id} to {action}")
        response = requests.post(toggle_url, json=payload, headers=headers, timeout=10)

        if response.status_code == 200:
            logger.info(f"Successfully set {id} -> {action.upper()}")
            return True
        else:
            logger.error(
                f"Failed to set {id}: {response.status_code} - {response.text}"
            )
            return False

    except requests.exceptions.RequestException as e:
        logger.error(f"Error connecting to Home Assistant: {e}")
        return False
    except Exception as e:
        logger.error(f"Error executing action: {e}")
        return False


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="Toggle a plug on or off")
    parser.add_argument("action", choices=["on", "off"], help="Action to perform")
    parser.add_argument("id", nargs="?", default="main", help="Plug identifier")
    parser.add_argument("--url", "-u", help="Home Assistant URL")
    parser.add_argument("--token", "-t", help="Home Assistant API token")
    parser.add_argument("--entity", "-e", help="Entity ID to control")

    args: argparse.Namespace = parser.parse_args()
    success: bool = toggle(args.action, args.id, args.url, args.token, args.entity)
    sys.exit(0 if success else 1)
