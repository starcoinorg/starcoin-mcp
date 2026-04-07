#!/usr/bin/env python3
from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from doctor import select_socket_candidate


class DoctorSocketSelectionTests(unittest.TestCase):
    def test_select_socket_candidate_prefers_reachable_socket(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            stale_socket = Path(tmpdir) / "preferred.sock"
            live_socket = Path(tmpdir) / "legacy.sock"
            stale_socket.write_text("stale", encoding="utf-8")
            live_socket.write_text("live", encoding="utf-8")

            with patch("doctor.platform.system", return_value="Linux"), patch(
                "doctor.is_unix_socket",
                side_effect=lambda path: path in {stale_socket, live_socket},
            ), patch(
                "doctor.socket_reachable",
                side_effect=lambda path: (
                    (False, "connection refused")
                    if path == stale_socket
                    else (True, "unix socket accepted a connection")
                ),
            ):
                self.assertEqual(
                    select_socket_candidate([stale_socket, live_socket]),
                    live_socket,
                )

    def test_select_socket_candidate_falls_back_to_first_socket_when_none_are_reachable(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            regular_file = Path(tmpdir) / "preferred.txt"
            stale_socket = Path(tmpdir) / "legacy.sock"
            regular_file.write_text("not a socket", encoding="utf-8")
            stale_socket.write_text("stale", encoding="utf-8")

            with patch("doctor.platform.system", return_value="Linux"), patch(
                "doctor.is_unix_socket",
                side_effect=lambda path: path == stale_socket,
            ), patch(
                "doctor.socket_reachable",
                return_value=(False, "connection refused"),
            ):
                self.assertEqual(
                    select_socket_candidate([regular_file, stale_socket]),
                    stale_socket,
                )


if __name__ == "__main__":
    unittest.main()
