#!/usr/bin/env python3
from __future__ import annotations

import io
import unittest
from unittest.mock import patch

import list_wallets
from list_wallets import (
    flatten_account_rows,
    format_wallet_listing,
    load_wallet_listing,
    render_table,
)


class FakeWalletClient:
    def __init__(self) -> None:
        self.calls: list[tuple[str, dict[str, object]]] = []

    def call_tool(
        self, name: str, arguments: dict[str, object] | None = None
    ) -> dict[str, object]:
        payload = arguments or {}
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
            return {
                "wallet_instances": [
                    {
                        "wallet_instance_id": "local-default",
                        "accounts": [
                            {
                                "address": "0xe7fa003bbcb25547686c7390fad4e0b0",
                                "label": "account-1",
                                "is_default": True,
                                "is_locked": True,
                                "public_key": "0xpub",
                            }
                        ],
                    }
                ]
            }
        raise AssertionError(f"unexpected tool call: {name}")


class ListWalletsTests(unittest.TestCase):
    def test_render_table_aligns_cjk_headers(self) -> None:
        rendered = render_table(
            ["钱包实例", "标签", "地址", "默认", "状态"],
            [
                [
                    "local-default",
                    "account-1",
                    "0xe7fa003bbcb25547686c7390fad4e0b0",
                    "是",
                    "locked",
                ]
            ],
        )

        expected = "\n".join(
            [
                "+---------------+-----------+------------------------------------+------+--------+",
                "| 钱包实例      | 标签      | 地址                               | 默认 | 状态   |",
                "+---------------+-----------+------------------------------------+------+--------+",
                "| local-default | account-1 | 0xe7fa003bbcb25547686c7390fad4e0b0 | 是   | locked |",
                "+---------------+-----------+------------------------------------+------+--------+",
            ]
        )
        self.assertEqual(rendered, expected)

    def test_flatten_account_rows_uses_label_and_status(self) -> None:
        rows = flatten_account_rows(
            wallet_instances=[{"wallet_instance_id": "local-default"}],
            account_groups={
                "local-default": [
                    {
                        "address": "0x1",
                        "label": "account-1",
                        "is_default": True,
                        "is_locked": False,
                    }
                ]
            },
            locale="zh",
            include_public_key=False,
        )

        self.assertEqual(
            rows,
            [
                {
                    "wallet_instance_id": "local-default",
                    "label": "account-1",
                    "address": "0x1",
                    "is_default": True,
                    "status": "unlocked",
                }
            ],
        )

    def test_load_wallet_listing_reads_instances_then_accounts(self) -> None:
        client = FakeWalletClient()

        listing = load_wallet_listing(
            client,  # type: ignore[arg-type]
            wallet_instance_id=None,
            locale="zh",
            include_public_key=True,
        )

        self.assertEqual(listing["rows"][0]["wallet_instance_id"], "local-default")
        self.assertEqual(listing["rows"][0]["public_key"], "0xpub")
        self.assertEqual(
            client.calls,
            [
                ("wallet_list_instances", {}),
                (
                    "wallet_list_accounts",
                    {
                        "wallet_instance_id": "local-default",
                        "include_public_key": True,
                    },
                ),
            ],
        )

    def test_format_wallet_listing_renders_localized_table(self) -> None:
        listing = {
            "rows": [
                {
                    "wallet_instance_id": "local-default",
                    "label": "account-1",
                    "address": "0xe7fa003bbcb25547686c7390fad4e0b0",
                    "is_default": True,
                    "status": "locked",
                }
            ]
        }

        rendered = format_wallet_listing(
            listing,
            locale="zh",
            include_public_key=False,
        )

        self.assertIn("钱包实例", rendered)
        self.assertIn("account-1", rendered)
        self.assertIn("locked", rendered)

    def test_main_prints_human_readable_table(self) -> None:
        client = FakeWalletClient()
        stdout = io.StringIO()

        with patch.object(
            list_wallets,
            "StarmaskDaemonClient",
            return_value=client,
        ), patch.object(
            list_wallets,
            "resolve_socket_path",
            return_value="/tmp/starmaskd.sock",
        ), patch.object(
            list_wallets,
            "parse_args",
            return_value=list_wallets.argparse.Namespace(
                socket_path=None,
                wallet_runtime_dir=None,
                wallet_instance_id=None,
                include_public_key=False,
                locale="zh",
                json=False,
            ),
        ), patch(
            "sys.stdout",
            stdout,
        ):
            self.assertEqual(list_wallets.main(), 0)

        output = stdout.getvalue()
        self.assertIn("钱包实例", output)
        self.assertIn("locked", output)


if __name__ == "__main__":
    unittest.main()
