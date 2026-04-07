#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from runtime_layout import (
    DEFAULT_WALLET_RUNTIME_DIR,
    platform_daemon_socket_candidates,
    resolve_wallet_daemon_socket_path,
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

    def test_resolve_wallet_daemon_socket_path_uses_legacy_env_alias(self) -> None:
        runtime_dir = Path("/tmp/runtime-from-arg")
        with patch.dict(
            os.environ,
            {"STARMASK_MCP_DAEMON_SOCKET_PATH": "/tmp/legacy-daemon.sock"},
            clear=True,
        ):
            self.assertEqual(
                resolve_wallet_daemon_socket_path(runtime_dir),
                Path("/tmp/legacy-daemon.sock"),
            )

    def test_resolve_wallet_daemon_socket_path_prefers_new_env_alias(self) -> None:
        runtime_dir = Path("/tmp/runtime-from-arg")
        with patch.dict(
            os.environ,
            {
                "STARMASKD_SOCKET_PATH": "/tmp/new-daemon.sock",
                "STARMASK_MCP_DAEMON_SOCKET_PATH": "/tmp/legacy-daemon.sock",
            },
            clear=True,
        ):
            self.assertEqual(
                resolve_wallet_daemon_socket_path(runtime_dir),
                Path("/tmp/new-daemon.sock"),
            )

    def test_resolve_wallet_daemon_socket_path_uses_default_socket_for_default_runtime(self) -> None:
        default_socket_path = Path("/tmp/platform-default.sock")
        with patch.dict(os.environ, {}, clear=True):
            self.assertEqual(
                resolve_wallet_daemon_socket_path(
                    DEFAULT_WALLET_RUNTIME_DIR,
                    default_socket_path=default_socket_path,
                ),
                default_socket_path,
            )

    def test_platform_daemon_socket_candidates_include_legacy_platform_path(self) -> None:
        with patch("runtime_layout.platform.system", return_value="Linux"), patch.object(
            Path,
            "home",
            return_value=Path("/tmp/runtime-layout-home"),
        ), patch.dict(
            os.environ,
            {
                "XDG_STATE_HOME": "/tmp/runtime-layout-state",
                "XDG_RUNTIME_DIR": "/tmp/runtime-layout-run",
            },
            clear=True,
        ):
            self.assertIn(
                Path("/tmp/runtime-layout-state/starcoin-mcp/starmaskd.sock").resolve(),
                platform_daemon_socket_candidates(),
            )


if __name__ == "__main__":
    unittest.main()
