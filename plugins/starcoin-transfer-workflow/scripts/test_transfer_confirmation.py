#!/usr/bin/env python3
from __future__ import annotations

import unittest

from transfer_controller import (
    TransferAmount,
    TransferController,
    TransferSession,
    describe_confirmation_depth,
    normalize_min_confirmed_blocks,
)


class FakeToolClient:
    def __init__(self, responses: dict[str, list[dict]]):
        self.responses = {name: list(items) for name, items in responses.items()}
        self.calls: list[tuple[str, dict | None]] = []

    def call_tool(self, name: str, arguments: dict | None = None) -> dict:
        self.calls.append((name, arguments))
        queue = self.responses.get(name)
        if not queue:
            raise AssertionError(f"unexpected tool call: {name}")
        return queue.pop(0)


def sample_session() -> TransferSession:
    return TransferSession(
        sender="0x1",
        receiver="0x2",
        wallet_instance_id="wallet-1",
        vm_profile="vm2_only",
        chain_id=254,
        network="dev",
        genesis_hash="0xabc",
        amount=TransferAmount(
            input_amount="1",
            input_unit="stc",
            raw_amount="1000000000",
            display_amount="1 STC",
            token_code="0x1::starcoin_coin::STC",
        ),
        wallet_instances={"wallet_instances": []},
        wallet_accounts={"wallet_instances": []},
        public_key="0xpub",
        prepare_result={
            "transaction_kind": "transfer",
            "raw_txn_bcs_hex": "0xdeadbeef",
            "chain_context": {
                "chain_id": 254,
                "network": "dev",
                "genesis_hash": "0xabc",
                "head_block_hash": "0x1",
                "head_block_number": 42,
                "observed_at": "2026-04-03T00:00:00Z",
            },
            "prepared_at": "2026-04-03T00:00:00Z",
            "simulation_status": "performed",
            "next_action": "sign_transaction",
            "transaction_summary": {
                "token_code": "0x1::starcoin_coin::STC",
            },
        },
        signed_txn_bcs_hex="0xsigned",
    )


class TransferConfirmationTests(unittest.TestCase):
    def test_normalize_min_confirmed_blocks_defaults_and_clamps(self) -> None:
        self.assertEqual(normalize_min_confirmed_blocks(None), 2)
        self.assertEqual(normalize_min_confirmed_blocks(0), 1)
        self.assertEqual(normalize_min_confirmed_blocks(3), 3)

    def test_describe_confirmation_depth_mentions_additional_blocks(self) -> None:
        self.assertEqual(
            describe_confirmation_depth(2),
            "2 blocks (the inclusion block plus 1 more)",
        )

    def test_confirmation_rows_show_effective_default_depth(self) -> None:
        controller = TransferController(
            node_client=FakeToolClient({}),
            wallet_client=FakeToolClient({}),
            chain_id=254,
            network="dev",
            genesis_hash="0xabc",
        )

        self.assertIn(
            ("Confirm Depth", describe_confirmation_depth(2)),
            controller.confirmation_rows(sample_session()),
        )

    def test_submit_threads_min_confirmed_blocks_to_follow_up_watch(self) -> None:
        node_client = FakeToolClient(
            {
                "submit_signed_transaction": [
                    {
                        "txn_hash": "0xhash",
                        "submission_state": "accepted",
                        "submitted": True,
                        "next_action": "watch_transaction",
                        "watch_result": None,
                    }
                ],
                "watch_transaction": [
                    {
                        "txn_hash": "0xhash",
                        "found": True,
                        "confirmed": True,
                        "effective_timeout_seconds": 30,
                        "effective_poll_interval_seconds": 3,
                        "effective_min_confirmed_blocks": 3,
                        "confirmed_blocks": 3,
                        "inclusion_block_number": 42,
                        "transaction_info": {"block_number": 42},
                        "events": [],
                        "status_summary": {"found": True, "confirmed": True},
                    }
                ],
            }
        )
        controller = TransferController(
            node_client=node_client,
            wallet_client=FakeToolClient({}),
            chain_id=254,
            network="dev",
            genesis_hash="0xabc",
        )

        outcome = controller.submit(
            sample_session(),
            timeout_seconds=30,
            min_confirmed_blocks=3,
            blocking=True,
        )

        self.assertTrue(outcome.success)
        self.assertEqual(
            node_client.calls,
            [
                (
                    "submit_signed_transaction",
                    {
                        "signed_txn_bcs_hex": "0xsigned",
                        "prepared_chain_context": sample_session().prepare_result["chain_context"],
                        "blocking": True,
                        "timeout_seconds": 30,
                        "min_confirmed_blocks": 3,
                    },
                ),
                (
                    "watch_transaction",
                    {
                        "txn_hash": "0xhash",
                        "timeout_seconds": 30,
                        "min_confirmed_blocks": 3,
                    },
                ),
            ],
        )


if __name__ == "__main__":
    unittest.main()
