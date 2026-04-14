#!/usr/bin/env python3
from __future__ import annotations

import unittest

from run_create_account import account_count_for_instance, resolve_wallet_instance


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


if __name__ == "__main__":
    unittest.main()
