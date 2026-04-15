#!/usr/bin/env python3
from __future__ import annotations

import io
import unittest
from unittest.mock import patch

import node_cli_client
import starmaskd_client


class ClientJsonArgumentsTests(unittest.TestCase):
    def test_node_cli_client_accepts_inline_json_arguments(self) -> None:
        self.assertEqual(
            node_cli_client.read_json_arguments('{"address":"0x1"}'),
            {"address": "0x1"},
        )

    def test_starmaskd_client_accepts_inline_json_arguments(self) -> None:
        self.assertEqual(
            starmaskd_client.read_json_arguments(
                '{"wallet_instance_id":"local-default","address":"0x1"}'
            ),
            {"wallet_instance_id": "local-default", "address": "0x1"},
        )

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


if __name__ == "__main__":
    unittest.main()
