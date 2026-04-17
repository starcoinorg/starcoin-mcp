#!/usr/bin/env python3
from __future__ import annotations

import io
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

import node_cli_client
import starmaskd_client


class ClientJsonArgumentsTests(unittest.TestCase):
    def test_node_cli_client_accepts_inline_json_arguments(self) -> None:
        self.assertEqual(
            node_cli_client.read_json_arguments('{"address":"0x1"}'),
            {"address": "0x1"},
        )

    def test_node_cli_client_parses_inline_json_as_call_argument(self) -> None:
        with patch.object(
            sys,
            "argv",
            [
                "node_cli_client.py",
                "call",
                "get_account_overview",
                '{"address":"0x1"}',
            ],
        ):
            args = node_cli_client.parse_args()

        self.assertEqual(args.command, "call")
        self.assertEqual(args.tool, "get_account_overview")
        self.assertEqual(args.arguments_json, '{"address":"0x1"}')

    def test_starmaskd_client_accepts_inline_json_arguments(self) -> None:
        self.assertEqual(
            starmaskd_client.read_json_arguments(
                '{"wallet_instance_id":"local-default","address":"0x1"}'
            ),
            {"wallet_instance_id": "local-default", "address": "0x1"},
        )

    def test_starmaskd_client_parses_inline_json_as_call_argument(self) -> None:
        with patch.object(
            sys,
            "argv",
            [
                "starmaskd_client.py",
                "call",
                "wallet_get_public_key",
                '{"wallet_instance_id":"local-default","address":"0x1"}',
            ],
        ):
            args = starmaskd_client.parse_args()

        self.assertEqual(args.command, "call")
        self.assertEqual(args.tool, "wallet_get_public_key")
        self.assertEqual(
            args.arguments_json,
            '{"wallet_instance_id":"local-default","address":"0x1"}',
        )

    def test_starmaskd_client_maps_account_import_export_tools(self) -> None:
        client = starmaskd_client.StarmaskDaemonClient(socket_path=Path("/tmp/starmaskd.sock"))

        with patch.object(client, "_call", return_value={"request_id": "req-export"}) as call:
            self.assertEqual(
                client.call_tool(
                    "wallet_request_export_account",
                    {
                        "client_request_id": "client-export",
                        "account_address": "0x1",
                        "wallet_instance_id": "local-default",
                        "output_file": "/tmp/account.key",
                        "force": True,
                    },
                ),
                {"request_id": "req-export"},
            )

        call.assert_called_once()
        self.assertEqual(call.call_args.args[0], "request.createExportAccount")
        self.assertEqual(call.call_args.args[1]["account_address"], "0x1")
        self.assertEqual(call.call_args.args[1]["output_file"], "/tmp/account.key")

        with patch.object(client, "_call", return_value={"request_id": "req-import"}) as call:
            self.assertEqual(
                client.call_tool(
                    "wallet_request_import_account",
                    {
                        "client_request_id": "client-import",
                        "wallet_instance_id": "local-default",
                        "private_key_file": "/tmp/import.key",
                    },
                ),
                {"request_id": "req-import"},
            )

        call.assert_called_once()
        self.assertEqual(call.call_args.args[0], "request.createImportAccount")
        self.assertEqual(call.call_args.args[1]["private_key_file"], "/tmp/import.key")

    def test_node_cli_client_still_accepts_stdin_json_arguments(self) -> None:
        with patch("sys.stdin", io.StringIO('{"address":"0x2"}')):
            self.assertEqual(
                node_cli_client.read_json_arguments(),
                {"address": "0x2"},
            )

    def test_starmaskd_client_still_accepts_stdin_json_arguments(self) -> None:
        with patch("sys.stdin", io.StringIO('{"address":"0x3"}')):
            self.assertEqual(
                starmaskd_client.read_json_arguments(),
                {"address": "0x3"},
            )

    def test_inline_arguments_must_be_json_object(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "tool arguments must be a JSON object"):
            node_cli_client.read_json_arguments('["0x1"]')
        with self.assertRaisesRegex(RuntimeError, "tool arguments must be a JSON object"):
            starmaskd_client.read_json_arguments('["0x1"]')


if __name__ == "__main__":
    unittest.main()
