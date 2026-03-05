#!/usr/bin/env python3
"""
Unit tests for plug.py module.
Tests the toggle function with mocked Home Assistant API calls.
"""

import types

import os
import sys
import unittest
from unittest.mock import patch, MagicMock

import requests

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


class TestPlugModule(unittest.TestCase):
    """Test suite for plug.py module."""

    def setUp(self) -> None:
        """Set up environment variables before each test."""
        self._env = {}
        for key in ["HASS_URL", "HASS_TOKEN", "ENTITY_ID"]:
            if key in os.environ:
                self._env[key] = os.environ[key]
            os.environ[key] = "test_value"

    def tearDown(self) -> None:
        """Restore environment after each test."""
        for key in ["HASS_URL", "HASS_TOKEN", "ENTITY_ID"]:
            if key in self._env:
                os.environ[key] = self._env[key]
            elif key in os.environ:
                del os.environ[key]

    def _import_plug(self) -> types.ModuleType:
        """Import plug module after setting up environment."""
        import importlib
        import commands.plug

        importlib.reload(commands.plug)
        return commands.plug

    @patch("requests.post")
    def test_toggle_success(self, mock_post: MagicMock) -> None:
        """Test successful toggle operation."""
        plug = self._import_plug()
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_post.return_value = mock_response

        result = plug.toggle("on", "main")
        self.assertTrue(result)
        mock_post.assert_called_once()

    @patch("requests.post")
    def test_toggle_success_with_custom_params(self, mock_post: MagicMock) -> None:
        """Test successful toggle with custom parameters."""
        plug = self._import_plug()
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_post.return_value = mock_response

        result = plug.toggle(
            "off",
            "bedroom",
            "http://custom.local:8123",
            "custom_token",
            "switch.bedroom",
        )
        self.assertTrue(result)

    @patch("requests.post")
    def test_toggle_failure(self, mock_post: MagicMock) -> None:
        """Test failed toggle operation."""
        plug = self._import_plug()
        mock_response = MagicMock()
        mock_response.status_code = 401
        mock_post.return_value = mock_response

        result = plug.toggle("on", "main")
        self.assertFalse(result)

    @patch("requests.post")
    def test_toggle_request_exception(self, mock_post: MagicMock) -> None:
        """Test toggle when request exception occurs."""
        plug = self._import_plug()
        mock_post.side_effect = requests.exceptions.ConnectionError(
            "Connection refused"
        )

        result = plug.toggle("on", "main")
        self.assertFalse(result)

    @patch("requests.post")
    def test_toggle_general_exception(self, mock_post: MagicMock) -> None:
        """Test toggle when general exception occurs."""
        plug = self._import_plug()
        mock_post.side_effect = Exception("Some error")

        result = plug.toggle("on", "main")
        self.assertFalse(result)

    @patch("requests.post")
    def test_toggle_with_cli_override(self, mock_post: MagicMock) -> None:
        """Test that CLI parameters override environment variables."""
        plug = self._import_plug()
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_post.return_value = mock_response

        result = plug.toggle(
            "on",
            "override_test",
            "http://override.local:8123",
            "override_token",
            "switch.override",
        )
        self.assertTrue(result)

    @patch("requests.post")
    def test_toggle_uses_turn_on_endpoint(self, mock_post: MagicMock) -> None:
        """Test that 'on' action uses turn_on endpoint."""
        plug = self._import_plug()
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_post.return_value = mock_response

        plug.toggle("on", "main")

        call_url = mock_post.call_args[0][0]
        self.assertIn("turn_on", call_url)

    @patch("requests.post")
    def test_toggle_uses_turn_off_endpoint(self, mock_post: MagicMock) -> None:
        """Test that 'off' action uses turn_off endpoint."""
        plug = self._import_plug()
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_post.return_value = mock_response

        plug.toggle("off", "main")

        call_url = mock_post.call_args[0][0]
        self.assertIn("turn_off", call_url)

    def test_toggle_exits_without_token(self) -> None:
        """Test that missing token causes sys.exit(1)."""
        os.environ["HASS_TOKEN"] = ""
        plug = self._import_plug()

        with self.assertRaises(SystemExit) as ctx:
            plug.toggle("on", "main")
        self.assertEqual(ctx.exception.code, 1)

    def test_toggle_exits_without_entity(self) -> None:
        """Test that missing entity causes sys.exit(1)."""
        os.environ["ENTITY_ID"] = ""
        plug = self._import_plug()

        with self.assertRaises(SystemExit) as ctx:
            plug.toggle("on", "main")
        self.assertEqual(ctx.exception.code, 1)

    def test_toggle_exits_without_url(self) -> None:
        """Test that missing URL causes sys.exit(1)."""
        os.environ["HASS_URL"] = ""
        plug = self._import_plug()

        with self.assertRaises(SystemExit) as ctx:
            plug.toggle("on", "main")
        self.assertEqual(ctx.exception.code, 1)

    @patch("requests.post")
    def test_toggle_payload_and_headers(self, mock_post: MagicMock) -> None:
        """Test that correct payload and headers are sent."""
        plug = self._import_plug()
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_post.return_value = mock_response

        plug.toggle("on", "main", token="my_token", entity="switch.test")

        call_kwargs = mock_post.call_args.kwargs
        self.assertEqual(call_kwargs["json"]["entity_id"], "switch.test")
        self.assertIn("Bearer my_token", call_kwargs["headers"]["Authorization"])

    @patch("requests.post")
    def test_toggle_case_insensitive(self, mock_post: MagicMock) -> None:
        """Test that action is case insensitive."""
        plug = self._import_plug()
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_post.return_value = mock_response

        # Test uppercase
        result = plug.toggle("ON", "main")
        self.assertTrue(result)
        call_url = mock_post.call_args[0][0]
        self.assertIn("turn_on", call_url)


if __name__ == "__main__":
    unittest.main()
