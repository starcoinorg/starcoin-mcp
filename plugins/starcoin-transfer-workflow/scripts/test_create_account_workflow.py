#!/usr/bin/env python3
from __future__ import annotations

import argparse
import io
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import run_create_account
from run_create_account import (
    account_count_for_instance,
    created_account_address,
    find_account_for_instance,
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


class CreateAccountFlowWalletClient:
    def __init__(self) -> None:
        self.calls: list[tuple[str, dict[str, object]]] = []

    def call_tool(
        self, name: str, payload: dict[str, object] | None = None
    ) -> dict[str, object]:
        payload = payload or {}
        self.calls.append((name, payload))
        if name == "wallet_list_instances":
            return {
                "wallet_instances": [
                    {
                        "wallet_instance_id": "local-default",
                        "extension_connected": True,
                        "profile_hint": "local_account_dir",
                        "lock_state": "unlocked",
                    }
                ]
            }
        if name == "wallet_list_accounts":
            list_call_count = sum(
                1 for call_name, _payload in self.calls if call_name == name
            )
            accounts = (
                []
                if list_call_count == 1
                else [{"address": "0xabc", "label": "account-1"}]
            )
            return {
                "wallet_instances": [
                    {
                        "wallet_instance_id": "local-default",
                        "accounts": accounts,
                    }
                ]
            }
        if name == "wallet_create_account":
            return {"request_id": "req-1", "status": "created"}
        if name == "wallet_get_request_status":
            return {
                "status": "approved",
                "result": {
                    "address": "0xabc",
                    "is_default": False,
                    "is_locked": False,
                },
            }
        if name == "wallet_set_account_label":
            self._assert_label_payload(payload)
            return {
                "wallet_instance_id": "local-default",
                "account": {
                    "address": "0xabc",
                    "label": "savings",
                    "is_read_only": False,
                },
            }
        raise AssertionError(f"unexpected tool call: {name}")

    def _assert_label_payload(self, payload: dict[str, object]) -> None:
        expected = {
            "wallet_instance_id": "local-default",
            "address": "0xabc",
            "label": "savings",
        }
        if payload != expected:
            raise AssertionError(f"unexpected label payload: {payload!r}")


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

    def test_find_account_for_instance_returns_matching_account(self) -> None:
        account = find_account_for_instance(
            {
                "wallet_instances": [
                    {
                        "wallet_instance_id": "local-default",
                        "accounts": [
                            {"address": "0x1", "label": "account-1"},
                            {"address": "0x2", "label": "savings"},
                        ],
                    }
                ]
            },
            "local-default",
            "0x2",
        )

        self.assertEqual(account, {"address": "0x2", "label": "savings"})

    def test_find_account_for_instance_returns_none_when_missing(self) -> None:
        account = find_account_for_instance(
            {
                "wallet_instances": [
                    {
                        "wallet_instance_id": "local-default",
                        "accounts": [{"address": "0x1", "label": "account-1"}],
                    }
                ]
            },
            "local-default",
            "0x9",
        )

        self.assertIsNone(account)

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

    def test_created_account_address_rejects_missing_address(self) -> None:
        with self.assertRaisesRegex(
            RuntimeError, "approved create-account result did not include an address"
        ):
            created_account_address({"address": None})

    def test_main_sets_custom_account_name_after_approved_create_account(self) -> None:
        client = CreateAccountFlowWalletClient()
        with tempfile.TemporaryDirectory() as tmpdir:
            args = argparse.Namespace(
                socket_path=None,
                wallet_runtime_dir=None,
                wallet_instance_id=None,
                client_request_id="client-create",
                display_hint="Create local account",
                account_name="savings",
                client_context="test-create-account",
                ttl_seconds=300,
                poll_interval_seconds=0.01,
                request_timeout_seconds=None,
                audit_log_path=str(Path(tmpdir) / "audit.jsonl"),
            )
            with (
                patch("run_create_account.parse_args", return_value=args),
                patch(
                    "run_create_account.resolve_socket_path",
                    return_value=Path(tmpdir) / "sock",
                ),
                patch("run_create_account.StarmaskDaemonClient", return_value=client),
                patch("sys.stdout", io.StringIO()) as stdout,
            ):
                exit_code = run_create_account.main()

        self.assertEqual(exit_code, 0)
        self.assertIn(
            (
                "wallet_set_account_label",
                {
                    "wallet_instance_id": "local-default",
                    "address": "0xabc",
                    "label": "savings",
                },
            ),
            client.calls,
        )
        self.assertIn("Account Name:", stdout.getvalue())
        self.assertIn("savings", stdout.getvalue())

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
