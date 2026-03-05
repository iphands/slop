#!/usr/bin/env python3
"""
Main CLI entry point for plug control.
Uses Click for command-line argument parsing.
"""

import click
from commands.plug import toggle


@click.group()
def cli() -> None:
    """Control plugs via Home Assistant API."""
    pass


@cli.command()
@click.argument("action", type=click.Choice(["on", "off"], case_sensitive=False))
@click.argument("id", default="main")
@click.option("--url", "-u", help="Home Assistant URL")
@click.option("--token", "-t", help="Home Assistant API token")
@click.option("--entity", "-e", help="Entity ID to control")
def plug(
    action: str, id: str, url: str | None, token: str | None, entity: str | None
) -> None:
    """Toggle a plug on or off."""
    toggle(action, id, url, token, entity)


if __name__ == "__main__":
    cli()
