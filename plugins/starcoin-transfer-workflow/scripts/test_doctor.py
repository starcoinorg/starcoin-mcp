#!/usr/bin/env python3
from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from doctor import json_rpc, live_rpc_checks, redacted_url_repr, select_socket_candidate


class DoctorSocketSelectionTests(unittest.TestCase):
    def test_select_socket_candidate_prefers_reachable_socket(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            primary_socket = Path(tmpdir) / "primary.sock"
            alternate_socket = Path(tmpdir) / "alternate.sock"
            primary_socket.write_text("stale", encoding="utf-8")
            alternate_socket.write_text("live", encoding="utf-8")

            with patch("doctor.platform.system", return_value="Linux"), patch(
                "doctor.is_unix_socket",
                side_effect=lambda path: path in {primary_socket, alternate_socket},
            ), patch(
                "doctor.socket_reachable",
                side_effect=lambda path: (
                    (False, "connection refused")
                    if path == primary_socket
                    else (True, "unix socket accepted a connection")
                ),
            ):
                self.assertEqual(
                    select_socket_candidate([primary_socket, alternate_socket]),
                    alternate_socket,
                )

    def test_select_socket_candidate_falls_back_to_first_socket_when_none_are_reachable(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            regular_file = Path(tmpdir) / "preferred.txt"
            primary_socket = Path(tmpdir) / "primary.sock"
            regular_file.write_text("not a socket", encoding="utf-8")
            primary_socket.write_text("stale", encoding="utf-8")

            with patch("doctor.platform.system", return_value="Linux"), patch(
                "doctor.is_unix_socket",
                side_effect=lambda path: path == primary_socket,
            ), patch(
                "doctor.socket_reachable",
                return_value=(False, "connection refused"),
            ):
                self.assertEqual(
                    select_socket_candidate([regular_file, primary_socket]),
                    primary_socket,
                )

    def test_live_rpc_checks_match_expected_chain_identity(self) -> None:
        def fake_json_rpc(_url: str, method: str):
            if method == "node.info":
                return {"net": "dev"}
            if method == "chain.info":
                return {"chain_id": 254, "genesis_hash": "0xabc"}
            raise AssertionError(f"unexpected method {method}")

        with patch("doctor.json_rpc", side_effect=fake_json_rpc):
            results = live_rpc_checks(
                "http://127.0.0.1:9850",
                expected_chain_id=254,
                expected_network="dev",
                expected_genesis_hash="0xabc",
            )

        self.assertTrue(all(item["ok"] for item in results))

    def test_live_rpc_checks_treat_chain_info_as_optional(self) -> None:
        def fake_json_rpc(_url: str, method: str):
            if method == "node.info":
                return {
                    "net": "dev",
                    "peer_info": {
                        "chain_info": {
                            "chain_id": 254,
                            "genesis_hash": "0xabc",
                        }
                    },
                }
            if method == "chain.info":
                raise RuntimeError("chain.info disabled")
            raise AssertionError(f"unexpected method {method}")

        with patch("doctor.json_rpc", side_effect=fake_json_rpc):
            results = live_rpc_checks(
                "http://127.0.0.1:9850",
                expected_chain_id=254,
                expected_network="dev",
                expected_genesis_hash="0xabc",
            )

        self.assertTrue(all(item["ok"] for item in results))
        self.assertIn("node.info responded", results[0]["detail"])

    def test_live_rpc_checks_reads_network_from_node_chain_info(self) -> None:
        def fake_json_rpc(_url: str, method: str):
            if method == "node.info":
                return {
                    "peer_info": {
                        "chain_info": {
                            "chain_id": 254,
                            "network": "dev",
                            "genesis_hash": "0xabc",
                        }
                    },
                }
            if method == "chain.info":
                raise RuntimeError("chain.info disabled")
            raise AssertionError(f"unexpected method {method}")

        with patch("doctor.json_rpc", side_effect=fake_json_rpc):
            results = live_rpc_checks(
                "http://127.0.0.1:9850",
                expected_chain_id=254,
                expected_network="dev",
                expected_genesis_hash="0xabc",
            )

        self.assertTrue(all(item["ok"] for item in results))

    def test_live_rpc_checks_fail_when_required_identity_is_missing(self) -> None:
        def fake_json_rpc(_url: str, method: str):
            if method == "node.info":
                return {"net": "dev", "peer_info": {"chain_info": {"chain_id": 254}}}
            if method == "chain.info":
                return {}
            raise AssertionError(f"unexpected method {method}")

        with patch("doctor.json_rpc", side_effect=fake_json_rpc):
            results = live_rpc_checks(
                "http://127.0.0.1:9850",
                expected_chain_id=None,
                expected_network=None,
                expected_genesis_hash=None,
            )

        failures = {item["name"] for item in results if not item["ok"]}
        self.assertEqual(failures, {"node rpc chain id", "node rpc network"})

    def test_redacted_url_repr_strips_credentials_query_and_fragment(self) -> None:
        redacted = redacted_url_repr("http://user:secret@node.example:9850/rpc?token=abc#frag")

        self.assertEqual(
            redacted,
            "'http://<redacted>@node.example:9850/rpc?<redacted>#<redacted>'",
        )

    def test_json_rpc_rejects_non_http_urls_before_urlopen(self) -> None:
        with patch("doctor.urlopen") as urlopen:
            with self.assertRaisesRegex(ValueError, "http or https URL"):
                json_rpc("file:///tmp/socket", "node.info")

        urlopen.assert_not_called()

    def test_json_rpc_rejects_missing_hostname_before_urlopen(self) -> None:
        with patch("doctor.urlopen") as urlopen:
            for url in ("http://user:pass@", "http://:9850"):
                with self.subTest(url=url):
                    with self.assertRaisesRegex(ValueError, "http or https URL"):
                        json_rpc(url, "node.info")

        urlopen.assert_not_called()

    def test_redacted_url_repr_strips_credentials_when_url_parse_fails(self) -> None:
        redacted = redacted_url_repr("http://user:secret@[::1/rpc?token=abc#frag")

        self.assertIn("<redacted>", redacted)
        self.assertNotIn("user:secret", redacted)
        self.assertNotIn("token=abc", redacted)
        self.assertNotIn("frag", redacted)

    def test_live_rpc_failure_detail_does_not_echo_sensitive_url(self) -> None:
        secret_url = "http://user:secret@node.local:9850/rpc?token=abc"

        with patch(
            "doctor.json_rpc",
            side_effect=RuntimeError(f"could not reach {secret_url}"),
        ):
            results = live_rpc_checks(
                secret_url,
                expected_chain_id=254,
                expected_network="dev",
                expected_genesis_hash="0xabc",
            )

        detail = results[0]["detail"]
        self.assertIn("error_type=RuntimeError", detail)
        self.assertNotIn("secret", detail)
        self.assertNotIn("token=abc", detail)
        self.assertNotIn("user:", detail)

    def test_live_rpc_checks_report_identity_mismatch(self) -> None:
        def fake_json_rpc(_url: str, method: str):
            if method == "node.info":
                return {"net": "dev"}
            if method == "chain.info":
                return {"chain_id": 254, "genesis_hash": "0xabc"}
            raise AssertionError(f"unexpected method {method}")

        with patch("doctor.json_rpc", side_effect=fake_json_rpc):
            results = live_rpc_checks(
                "http://127.0.0.1:9850",
                expected_chain_id=1,
                expected_network="main",
                expected_genesis_hash="0xdef",
            )

        failures = {item["name"] for item in results if not item["ok"]}
        self.assertEqual(
            failures,
            {"node rpc chain id", "node rpc network", "node rpc genesis hash"},
        )


if __name__ == "__main__":
    unittest.main()
