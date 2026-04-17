#!/usr/bin/env python3
from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from wallet_runtime import (
    ensure_private_wallet_dir,
    resolve_account_export_output_file,
    request_account_export,
    request_account_import,
)


class FakeWalletClient:
    def __init__(self, *, request_result: dict, status_result: dict) -> None:
        self.request_result = request_result
        self.status_result = status_result
        self.calls: list[tuple[str, dict]] = []

    def call_tool(self, name: str, arguments: dict | None = None) -> dict:
        self.calls.append((name, arguments or {}))
        if name in {"wallet_request_export_account", "wallet_request_import_account"}:
            return self.request_result
        if name == "wallet_get_request_status":
            return self.status_result
        raise AssertionError(f"unexpected tool call: {name}")


class WalletRuntimeAccountExportTests(unittest.TestCase):
    def test_ensure_private_wallet_dir_creates_missing_directory(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            wallet_dir = Path(tmpdir) / "local-accounts" / "default"

            ensure_private_wallet_dir(wallet_dir)

            self.assertTrue(wallet_dir.is_dir())
            self.assertEqual(wallet_dir.stat().st_mode & 0o777, 0o700)

    def test_export_account_help_uses_output_file(self) -> None:
        completed = subprocess.run(
            [
                sys.executable,
                str(Path(__file__).with_name("wallet_runtime.py")),
                "export-account",
                "--help",
            ],
            text=True,
            capture_output=True,
            check=False,
        )

        self.assertEqual(completed.returncode, 0)
        self.assertIn("--output-file", completed.stdout)
        self.assertNotIn("--backup-file", completed.stdout)

    def test_legacy_backup_command_is_not_registered(self) -> None:
        completed = subprocess.run(
            [
                sys.executable,
                str(Path(__file__).with_name("wallet_runtime.py")),
                "backup",
                "--help",
            ],
            text=True,
            capture_output=True,
            check=False,
        )

        self.assertNotEqual(completed.returncode, 0)
        self.assertIn("invalid choice: 'backup'", completed.stderr)

    def test_resolve_account_export_output_file_appends_timestamped_child_for_existing_parent(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            parent_dir = Path(tmpdir) / "exports"
            parent_dir.mkdir()

            with patch("wallet_runtime.time.strftime", return_value="20260414-120000"):
                self.assertEqual(
                    resolve_account_export_output_file(
                        str(parent_dir),
                        account_address="0xABCDEF",
                    ),
                    parent_dir.resolve() / "abcdef-private-key-export-20260414-120000.txt",
                )

    def test_import_account_help_is_registered(self) -> None:
        completed = subprocess.run(
            [
                sys.executable,
                str(Path(__file__).with_name("wallet_runtime.py")),
                "import-account",
                "--help",
            ],
            text=True,
            capture_output=True,
            check=False,
        )

        self.assertEqual(completed.returncode, 0)
        self.assertIn("--private-key-file", completed.stdout)

    def test_request_account_export_submits_online_wallet_request(self) -> None:
        client = FakeWalletClient(
            request_result={"request_id": "req-export"},
            status_result={
                "status": "approved",
                "result": {
                    "kind": "exported_account",
                    "address": "0x1",
                    "output_file": "/tmp/account.key",
                },
            },
        )

        with patch("wallet_runtime.new_client_request_id", return_value="client-export"):
            result = request_account_export(
                client,  # type: ignore[arg-type]
                account_address="0x1",
                output_file=Path("/tmp/account.key"),
                wallet_instance_id="local-default",
                force=True,
                ttl_seconds=300,
                wait_timeout_seconds=1,
            )

        self.assertEqual(result["request_id"], "req-export")
        self.assertEqual(client.calls[0][0], "wallet_request_export_account")
        self.assertEqual(client.calls[0][1]["client_request_id"], "client-export")
        self.assertEqual(client.calls[0][1]["account_address"], "0x1")
        self.assertEqual(client.calls[0][1]["output_file"], "/tmp/account.key")
        self.assertTrue(client.calls[0][1]["force"])
        self.assertEqual(client.calls[1], ("wallet_get_request_status", {"request_id": "req-export"}))

    def test_request_account_import_submits_online_wallet_request(self) -> None:
        client = FakeWalletClient(
            request_result={"request_id": "req-import"},
            status_result={
                "status": "approved",
                "result": {
                    "kind": "imported_account",
                    "address": "0x2",
                    "public_key": "0xpub",
                    "curve": "ed25519",
                    "is_default": False,
                    "is_locked": True,
                },
            },
        )

        with patch("wallet_runtime.new_client_request_id", return_value="client-import"):
            result = request_account_import(
                client,  # type: ignore[arg-type]
                private_key_file=Path("/tmp/import.key"),
                account_address=None,
                wallet_instance_id="local-default",
                ttl_seconds=300,
                wait_timeout_seconds=1,
            )

        self.assertEqual(result["request_id"], "req-import")
        self.assertEqual(client.calls[0][0], "wallet_request_import_account")
        self.assertEqual(client.calls[0][1]["client_request_id"], "client-import")
        self.assertEqual(client.calls[0][1]["private_key_file"], "/tmp/import.key")
        self.assertEqual(client.calls[0][1]["wallet_instance_id"], "local-default")
        self.assertIsNone(client.calls[0][1]["account_address"])
        self.assertEqual(client.calls[1], ("wallet_get_request_status", {"request_id": "req-import"}))


if __name__ == "__main__":
    unittest.main()
