#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from runtime_layout import (
    resolve_wallet_runtime_dir,
    wallet_runtime_metadata_path,
    wallet_runtime_socket_path,
)
from starmaskd_client import resolve_socket_path


class RuntimeLayoutTests(unittest.TestCase):
    def test_wallet_runtime_socket_path_uses_runtime_run_subdirectory(self) -> None:
        runtime_dir = Path("/tmp/example-wallet-runtime")
        self.assertEqual(
            wallet_runtime_socket_path(runtime_dir),
            runtime_dir / "run" / "starmaskd.sock",
        )

    def test_resolve_wallet_runtime_dir_honors_environment_override(self) -> None:
        with patch.dict(os.environ, {"STARMASK_WALLET_RUNTIME_DIR": "/tmp/runtime-env"}):
            self.assertEqual(
                resolve_wallet_runtime_dir(None),
                Path("/tmp/runtime-env"),
            )

    def test_resolve_wallet_runtime_dir_prefers_explicit_argument(self) -> None:
        with patch.dict(os.environ, {"STARMASK_WALLET_RUNTIME_DIR": "/tmp/runtime-env"}):
            self.assertEqual(
                resolve_wallet_runtime_dir("/tmp/runtime-arg"),
                Path("/tmp/runtime-arg"),
            )

    def test_resolve_socket_path_prefers_runtime_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            runtime_dir = Path(tmpdir)
            metadata_path = wallet_runtime_metadata_path(runtime_dir)
            metadata_path.write_text(
                json.dumps({"daemon_socket_path": str(runtime_dir / "metadata.sock")}),
                encoding="utf-8",
            )

            self.assertEqual(
                resolve_socket_path(None, str(runtime_dir)),
                runtime_dir / "metadata.sock",
            )

    def test_resolve_socket_path_falls_back_to_runtime_run_socket(self) -> None:
        runtime_dir = Path("/tmp/runtime-from-arg")
        with patch.dict(os.environ, {}, clear=False):
            self.assertEqual(
                resolve_socket_path(None, str(runtime_dir)),
                runtime_dir / "run" / "starmaskd.sock",
            )

    def test_resolve_socket_path_uses_runtime_env_for_fallback(self) -> None:
        with patch.dict(os.environ, {"STARMASK_WALLET_RUNTIME_DIR": "/tmp/runtime-env"}):
            self.assertEqual(
                resolve_socket_path(None, None),
                Path("/tmp/runtime-env") / "run" / "starmaskd.sock",
            )

    def test_resolve_socket_path_prefers_explicit_runtime_argument(self) -> None:
        with patch.dict(os.environ, {"STARMASK_WALLET_RUNTIME_DIR": "/tmp/runtime-env"}):
            self.assertEqual(
                resolve_socket_path(None, "/tmp/runtime-arg"),
                Path("/tmp/runtime-arg") / "run" / "starmaskd.sock",
            )


if __name__ == "__main__":
    unittest.main()
