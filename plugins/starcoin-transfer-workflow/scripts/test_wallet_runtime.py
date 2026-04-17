#!/usr/bin/env python3
from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from wallet_runtime import (
    export_account_private_key,
    resolve_account_export_chain_id,
    resolve_account_export_output_file,
    resolve_account_export_wallet_dir,
    runtime_paths,
)


class WalletRuntimeAccountExportTests(unittest.TestCase):
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

    def test_resolve_account_export_wallet_dir_prefers_runtime_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            runtime_dir = Path(tmpdir) / "runtime"
            wallet_dir = Path(tmpdir) / "wallet"
            wallet_dir.mkdir()
            metadata_path = runtime_paths(runtime_dir)["metadata_path"]
            metadata_path.parent.mkdir(parents=True, exist_ok=True)
            metadata_path.write_text(
                json.dumps({"wallet_dir": str(wallet_dir)}),
                encoding="utf-8",
            )

            self.assertEqual(
                resolve_account_export_wallet_dir(runtime_dir, None),
                wallet_dir.resolve(),
            )

    def test_resolve_account_export_wallet_dir_falls_back_to_runtime_config(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            runtime_dir = Path(tmpdir) / "runtime"
            wallet_dir = Path(tmpdir) / "wallet"
            wallet_dir.mkdir()
            config_path = runtime_paths(runtime_dir)["config_path"]
            config_path.parent.mkdir(parents=True, exist_ok=True)
            config_path.write_text(
                (
                    '[[wallet_backends]]\n'
                    f'account_dir = "{wallet_dir}"\n'
                ),
                encoding="utf-8",
            )

            self.assertEqual(
                resolve_account_export_wallet_dir(runtime_dir, None),
                wallet_dir.resolve(),
            )

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

    def test_resolve_account_export_chain_id_prefers_runtime_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            runtime_dir = Path(tmpdir) / "runtime"
            metadata_path = runtime_paths(runtime_dir)["metadata_path"]
            metadata_path.parent.mkdir(parents=True, exist_ok=True)
            metadata_path.write_text(json.dumps({"chain_id": 251}), encoding="utf-8")

            self.assertEqual(resolve_account_export_chain_id(runtime_dir, None), 251)

    def test_resolve_account_export_chain_id_falls_back_to_runtime_config(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            runtime_dir = Path(tmpdir) / "runtime"
            config_path = runtime_paths(runtime_dir)["config_path"]
            config_path.parent.mkdir(parents=True, exist_ok=True)
            config_path.write_text(
                "[[wallet_backends]]\nchain_id = 250\n",
                encoding="utf-8",
            )

            self.assertEqual(resolve_account_export_chain_id(runtime_dir, None), 250)

    def test_resolve_account_export_chain_id_uses_explicit_arg(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            self.assertEqual(
                resolve_account_export_chain_id(Path(tmpdir) / "runtime", 249), 249
            )

    def test_export_account_private_key_invokes_export_binary(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            base_dir = Path(tmpdir)
            wallet_dir = base_dir / "wallet"
            wallet_dir.mkdir()
            output_file = base_dir / "account.key"
            completed = subprocess.CompletedProcess(
                args=["local-account-export"],
                returncode=0,
                stdout=json.dumps(
                    {
                        "address": "0x1",
                        "wallet_dir": str(wallet_dir),
                        "output_file": str(output_file),
                    }
                ),
                stderr="",
            )

            with patch(
                "wallet_runtime.launch_command",
                return_value=(["local-account-export"], "/bin/local-account-export"),
            ) as launch_command, patch(
                "wallet_runtime.subprocess.run", return_value=completed
            ) as subprocess_run:
                result = export_account_private_key(
                    wallet_dir=wallet_dir,
                    destination_file=output_file,
                    runtime_dir=base_dir / "runtime",
                    account_address="0x1",
                    chain_id=254,
                    password="secret\n",
                    force=False,
                )

            launch_args = launch_command.call_args.kwargs["program_args"]
            self.assertIn("--address", launch_args)
            self.assertIn("0x1", launch_args)
            self.assertIn("--password-stdin", launch_args)
            self.assertEqual(subprocess_run.call_args.kwargs["input"], "secret\n")
            self.assertEqual(result["export_launch"], "/bin/local-account-export")


if __name__ == "__main__":
    unittest.main()
