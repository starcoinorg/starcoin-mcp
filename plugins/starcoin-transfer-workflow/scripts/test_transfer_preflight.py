#!/usr/bin/env python3
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from transfer_controller import TransferAmount, TransferController, TransferSession
from transfer_host import TransferAuditLogger, TransferPreflightReport, TransferRiskLabel


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


def sample_session(
    *,
    raw_amount: str = "1000000000",
    input_amount: str = "1",
    input_unit: str = "stc",
    display_amount: str = "1 STC",
) -> TransferSession:
    return TransferSession(
        sender="0x1",
        receiver="0x2",
        wallet_instance_id="wallet-1",
        vm_profile="vm2_only",
        chain_id=254,
        network="dev",
        genesis_hash="0xabc",
        amount=TransferAmount(
            input_amount=input_amount,
            input_unit=input_unit,
            raw_amount=raw_amount,
            display_amount=display_amount,
            token_code="0x1::starcoin_coin::STC",
        ),
        wallet_instances={"wallet_instances": [{"wallet_instance_id": "wallet-1"}]},
        wallet_accounts={
            "wallet_instances": [
                {
                    "wallet_instance_id": "wallet-1",
                    "accounts": [{"address": "0x1"}],
                }
            ]
        },
        public_key="0xpub",
        prepare_result={
            "transaction_kind": "transfer",
            "raw_txn_bcs_hex": "0xdeadbeef",
            "raw_txn": {
                "sequence_number": "7",
                "gas_unit_price": "2",
                "max_gas_amount": "1000",
                "gas_token_code": "0x1::starcoin_coin::STC",
            },
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
            "simulation": {
                "gas_used": 321,
                "vm_status": "Executed",
                "events": [],
                "write_set_summary": [],
                "raw": {},
            },
            "next_action": "sign_transaction",
            "transaction_summary": {
                "token_code": "0x1::starcoin_coin::STC",
            },
        },
        signed_txn_bcs_hex="0xsigned",
    )


class TransferPreflightTests(unittest.TestCase):
    def test_collect_preflight_report_derives_fee_balance_and_nonce(self) -> None:
        node_client = FakeToolClient(
            {
                "chain_status": [
                    {
                        "chain_id": 254,
                        "network": "dev",
                        "genesis_hash": "0xabc",
                    }
                ],
                "node_health": [
                    {
                        "node_available": True,
                        "warnings": [],
                        "peers_summary": {"count": 3},
                    }
                ],
                "get_account_overview": [
                    {
                        "address": "0x1",
                        "onchain_exists": True,
                        "sequence_number": 7,
                        "next_sequence_number_hint": 7,
                        "balances": [
                            {
                                "name": "0x1::fungible_asset::FungibleStore",
                                "value": {"json": {"balance": 2_000_000_000, "token": "STC"}},
                            }
                        ],
                        "accepted_tokens": ["0x1::starcoin_coin::STC"],
                    },
                    {
                        "address": "0x2",
                        "onchain_exists": True,
                        "balances": [],
                        "accepted_tokens": [],
                    },
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

        report = controller.collect_preflight_report(sample_session())

        self.assertEqual(report.prepared_sequence_number, 7)
        self.assertEqual(report.next_sequence_number_hint, 7)
        self.assertEqual(report.estimated_network_fee, 642)
        self.assertEqual(report.max_network_fee, 2000)
        self.assertEqual(report.sender_token_balance, 2_000_000_000)
        self.assertEqual(report.sender_post_transfer_balance, 1_000_000_000)
        self.assertFalse(controller.has_blocking_risks(report))
        self.assertIn(
            ("Estimated Fee", "642 raw units"),
            controller.preflight_rows(sample_session(), report, min_confirmed_blocks=3),
        )
        self.assertEqual(report.risk_labels, ())

    def test_collect_preflight_report_flags_blocking_risks(self) -> None:
        node_client = FakeToolClient(
            {
                "chain_status": [
                    {
                        "chain_id": 254,
                        "network": "dev",
                        "genesis_hash": "0xabc",
                    }
                ],
                "node_health": [
                    {
                        "node_available": False,
                        "warnings": ["node.info unavailable: timeout"],
                        "peers_summary": {"count": 0},
                    }
                ],
                "get_account_overview": [
                    {
                        "address": "0x1",
                        "onchain_exists": True,
                        "sequence_number": 7,
                        "next_sequence_number_hint": 9,
                        "balances": [
                            {
                                "name": "0x1::fungible_asset::FungibleStore",
                                "value": {"json": {"balance": 10, "token": "STC"}},
                            }
                        ],
                        "accepted_tokens": ["0x1::starcoin_coin::STC"],
                    },
                    {
                        "address": "0x2",
                        "onchain_exists": False,
                        "balances": [],
                        "accepted_tokens": [],
                    },
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

        report = controller.collect_preflight_report(
            sample_session(
                raw_amount="20",
                input_amount="20",
                input_unit="raw",
                display_amount="20 raw units",
            )
        )

        risk_codes = {risk.code for risk in report.risk_labels}
        self.assertTrue(controller.has_blocking_risks(report))
        self.assertIn("rpc_unavailable", risk_codes)
        self.assertIn("insufficient_token_balance", risk_codes)
        self.assertIn("insufficient_balance_for_amount_and_fee", risk_codes)
        self.assertIn("nonce_advanced_after_prepare", risk_codes)
        self.assertIn("receiver_account_not_initialized", risk_codes)

    def test_audit_logger_records_hash_not_payloads(self) -> None:
        report = TransferPreflightReport(
            chain_status={},
            node_health={},
            sender_overview={},
            receiver_overview={},
            token_code="0x1::starcoin_coin::STC",
            gas_token_code="0x1::starcoin_coin::STC",
            sender_visible_in_wallet=True,
            prepared_sequence_number=7,
            next_sequence_number_hint=7,
            gas_unit_price=2,
            max_gas_amount=1000,
            simulation_gas_used=321,
            estimated_network_fee=642,
            max_network_fee=2000,
            sender_token_balance=2_000_000_000,
            sender_gas_balance=2_000_000_000,
            sender_post_transfer_balance=1_000_000_000,
            risk_labels=(
                TransferRiskLabel(
                    code="receiver_account_not_initialized",
                    severity="info",
                    message="Receiver account does not currently exist on-chain.",
                ),
            ),
        )
        session = sample_session()
        session.request = {"request_id": "req-1"}

        with tempfile.TemporaryDirectory() as temp_dir:
            log_path = Path(temp_dir) / "audit.jsonl"
            logger = TransferAuditLogger(log_path)
            logger.record_preflight(session, report)
            logger.record_sign_request_terminal(
                session,
                {
                    "status": "approved",
                    "result": {"signed_txn_bcs_hex": "0xshould-not-be-logged"},
                },
            )
            records = [
                json.loads(line)
                for line in log_path.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]

        self.assertEqual(records[0]["event"], "preflight_preview")
        self.assertEqual(records[1]["event"], "sign_request_terminal")
        self.assertEqual(records[1]["request_id"], "req-1")
        self.assertTrue(records[0]["payload_sha256"])
        self.assertNotIn("raw_txn_bcs_hex", records[0])
        self.assertNotIn("signed_txn_bcs_hex", records[1])


if __name__ == "__main__":
    unittest.main()
