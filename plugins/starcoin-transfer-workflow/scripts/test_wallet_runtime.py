#!/usr/bin/env python3
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from wallet_runtime import (
    backup_wallet_dir,
    resolve_backup_destination,
    resolve_backup_wallet_dir,
    runtime_paths,
)


class WalletRuntimeBackupTests(unittest.TestCase):
    def test_resolve_backup_wallet_dir_prefers_runtime_metadata(self) -> None:
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
                resolve_backup_wallet_dir(runtime_dir, None),
                wallet_dir.resolve(),
            )

    def test_resolve_backup_wallet_dir_falls_back_to_runtime_config(self) -> None:
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
                resolve_backup_wallet_dir(runtime_dir, None),
                wallet_dir.resolve(),
            )

    def test_resolve_backup_destination_appends_timestamped_child_for_existing_parent(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            parent_dir = Path(tmpdir) / "backups"
            parent_dir.mkdir()
            source_wallet_dir = Path(tmpdir) / "wallet-default"
            source_wallet_dir.mkdir()

            with patch("wallet_runtime.time.strftime", return_value="20260414-120000"):
                self.assertEqual(
                    resolve_backup_destination(
                        str(parent_dir),
                        source_wallet_dir=source_wallet_dir,
                    ),
                    parent_dir.resolve() / "wallet-default-backup-20260414-120000",
                )

    def test_backup_wallet_dir_copies_files_and_writes_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            base_dir = Path(tmpdir)
            source_wallet_dir = base_dir / "wallet"
            source_wallet_dir.mkdir()
            (source_wallet_dir / "account.json").write_text("{}", encoding="utf-8")
            destination_dir = base_dir / "backup"
            runtime_dir = base_dir / "runtime"

            manifest = backup_wallet_dir(
                source_wallet_dir=source_wallet_dir,
                destination_dir=destination_dir,
                runtime_dir=runtime_dir,
            )

            self.assertTrue((destination_dir / "account.json").exists())
            self.assertEqual(
                manifest["source_wallet_dir"],
                str(source_wallet_dir),
            )
            self.assertEqual(
                json.loads((destination_dir / "backup-manifest.json").read_text(encoding="utf-8"))[
                    "backup_dir"
                ],
                str(destination_dir),
            )


if __name__ == "__main__":
    unittest.main()
