#!/usr/bin/env python3
from __future__ import annotations

import unittest
from unittest.mock import patch

from run_create_account import (
    account_count_for_instance,
    resolve_wallet_instance,
    wait_for_terminal_request,
)


class FakeWalletClient:
    def __init__(self, responses: list[dict[str, object]]) -> None:
        self._responses = list(responses)

    def call_tool(self, name: str, payload: dict[str, object]) -> dict[str, object]:
        if name != "wallet_get_request_status":
            raise AssertionError(f"unexpected tool call: {name}")
        if payload.get("request_id") is None:
            raise AssertionError("request_id is required")
        if len(self._responses) > 1:
            return self._responses.pop(0)
        return self._responses[0]


class CreateAccountWorkflowTests(unittest.TestCase):
    def test_resolve_wallet_instance_auto_selects_single_candidate(self) -> None:
        selected = resolve_wallet_instance(
            {
                "wallet_instances": [
                    {
                        "wallet_instance_id": "local-default",
                        "extension_connected": True,
                        "profile_hint": "local_account_dir",
                        "lock_state": "locked",
                    }
                ]
            },
            None,
        )

        self.assertEqual(selected["wallet_instance_id"], "local-default")

    def test_resolve_wallet_instance_requires_explicit_choice_when_ambiguous(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "wallet_instance_id is required"):
            resolve_wallet_instance(
                {
                    "wallet_instances": [
                        {
                            "wallet_instance_id": "local-default",
                            "extension_connected": True,
                            "profile_hint": "local_account_dir",
                            "lock_state": "locked",
                        },
                        {
                            "wallet_instance_id": "extension-main",
                            "extension_connected": True,
                            "profile_hint": "extension",
                            "lock_state": "unlocked",
                        },
                    ]
                },
                None,
            )

    def test_resolve_wallet_instance_explicit_id_selects_from_multiple(self) -> None:
        selected = resolve_wallet_instance(
            {
                "wallet_instances": [
                    {
                        "wallet_instance_id": "local-default",
                        "extension_connected": True,
                        "profile_hint": "local_account_dir",
                        "lock_state": "locked",
                    },
                    {
                        "wallet_instance_id": "extension-main",
                        "extension_connected": True,
                        "profile_hint": "extension",
                        "lock_state": "unlocked",
                    },
                ]
            },
            "extension-main",
        )

        self.assertEqual(selected["wallet_instance_id"], "extension-main")

    def test_resolve_wallet_instance_fails_when_no_connected_wallet_exists(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "no connected wallet instances"):
            resolve_wallet_instance(
                {
                    "wallet_instances": [
                        {
                            "wallet_instance_id": "local-default",
                            "extension_connected": False,
                            "profile_hint": "local_account_dir",
                            "lock_state": "locked",
                        }
                    ]
                },
                None,
            )

    def test_account_count_for_instance_reads_matching_group(self) -> None:
        count = account_count_for_instance(
            {
                "wallet_instances": [
                    {
                        "wallet_instance_id": "local-default",
                        "accounts": [{"address": "0x1"}, {"address": "0x2"}],
                    }
                ]
            },
            "local-default",
        )

        self.assertEqual(count, 2)

    def test_account_count_for_instance_returns_zero_when_not_found(self) -> None:
        count = account_count_for_instance(
            {
                "wallet_instances": [
                    {
                        "wallet_instance_id": "other",
                        "accounts": [],
                    }
                ]
            },
            "missing-id",
        )

        self.assertEqual(count, 0)

    def test_wait_for_terminal_request_returns_terminal_status(self) -> None:
        client = FakeWalletClient(
            [
                {"status": "pending"},
                {"status": "approved", "result": {"address": "0x1"}},
            ]
        )

        with patch("run_create_account.time.sleep", return_value=None):
            status = wait_for_terminal_request(
                client,
                request_id="req-1",
                poll_interval_seconds=0.01,
            )

        self.assertEqual(status["status"], "approved")
        self.assertEqual(status["result"], {"address": "0x1"})

    def test_wait_for_terminal_request_raises_timeout_and_notifies(self) -> None:
        client = FakeWalletClient([{"status": "pending"}])
        status_changes: list[str] = []

        with (
            patch(
                "run_create_account.time.monotonic",
                side_effect=[10.0, 10.2, 10.5],
            ),
            patch("run_create_account.time.sleep", return_value=None),
        ):
            with self.assertRaisesRegex(
                TimeoutError,
                r"request req-timeout did not reach a terminal status within 0.5s; last status=pending",
            ):
                wait_for_terminal_request(
                    client,
                    request_id="req-timeout",
                    poll_interval_seconds=0.01,
                    timeout_seconds=0.5,
                    on_status_change=status_changes.append,
                )

        self.assertEqual(status_changes, ["pending", "timeout"])


if __name__ == "__main__":
    unittest.main()
