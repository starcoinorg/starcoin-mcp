#!/usr/bin/env python3
from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from test_transfer_confirmation import FakeToolClient, sample_session
from transfer_controller import TransferController
from transfer_state import TransferStateStore, session_payload_sha256, state_default


class TransferStateTests(unittest.TestCase):
    def test_prepared_attestation_is_required_before_submit(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            store = TransferStateStore(Path(temp_dir) / "transfer-state.json")
            session = sample_session()
            controller = TransferController(
                node_client=FakeToolClient({}),
                wallet_client=FakeToolClient({}),
                chain_id=254,
                network="dev",
                genesis_hash="0xabc",
                state_store=store,
            )

            with self.assertRaisesRegex(
                RuntimeError, "no persisted prepared-transfer attestation"
            ):
                controller.submit(session, timeout_seconds=30)

    def test_records_prepared_attestation_without_raw_payload(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            store = TransferStateStore(Path(temp_dir) / "transfer-state.json")
            session = sample_session()

            store.record_prepared(session)
            store.require_prepared_for_submit(session)

            payload_hash = session_payload_sha256(session)
            record = store.read()["prepared_transactions"][payload_hash]
            self.assertEqual(record["simulation_status"], "performed")
            self.assertEqual(record["chain_context"]["chain_id"], 254)
            self.assertNotIn("raw_txn_bcs_hex", record)
            self.assertNotIn("signed_txn_bcs_hex", record)

    def test_prepared_attestation_must_match_session_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            store = TransferStateStore(Path(temp_dir) / "transfer-state.json")
            session = sample_session()
            store.record_prepared(session)
            session.receiver = "0x3"

            with self.assertRaisesRegex(RuntimeError, "session metadata: receiver"):
                store.require_prepared_for_submit(session)

    def test_submit_unknown_records_unresolved_submission(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            store = TransferStateStore(Path(temp_dir) / "transfer-state.json")
            session = sample_session()
            store.record_prepared(session)
            node_client = FakeToolClient(
                {
                    "submit_signed_transaction": [
                        {
                            "txn_hash": "0xhash",
                            "submission_state": "unknown",
                            "submitted": False,
                            "next_action": "reconcile_by_txn_hash",
                            "error_code": "submission_unknown",
                            "watch_result": None,
                        }
                    ],
                    "watch_transaction": [
                        {
                            "txn_hash": "0xhash",
                            "found": False,
                            "confirmed": False,
                            "events": [],
                            "status_summary": {"found": False, "confirmed": False},
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
                state_store=store,
            )

            outcome = controller.submit(session, timeout_seconds=30)

            self.assertFalse(outcome.success)
            unresolved = store.unresolved_for_session(session)
            self.assertIsNotNone(unresolved)
            assert unresolved is not None
            self.assertEqual(unresolved.txn_hash, "0xhash")

    def test_existing_unresolved_submission_reconciles_before_resubmit(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            store = TransferStateStore(Path(temp_dir) / "transfer-state.json")
            session = sample_session()
            store.record_prepared(session)
            store.record_unresolved_submission(
                session,
                {
                    "txn_hash": "0xhash",
                    "submission_state": "unknown",
                    "next_action": "reconcile_by_txn_hash",
                },
            )
            node_client = FakeToolClient(
                {
                    "watch_transaction": [
                        {
                            "txn_hash": "0xhash",
                            "found": True,
                            "confirmed": True,
                            "events": [],
                            "status_summary": {"found": True, "confirmed": True},
                        }
                    ]
                }
            )
            controller = TransferController(
                node_client=node_client,
                wallet_client=FakeToolClient({}),
                chain_id=254,
                network="dev",
                genesis_hash="0xabc",
                state_store=store,
            )

            outcome = controller.submit(session, timeout_seconds=30)

            self.assertTrue(outcome.success)
            self.assertEqual(outcome.watch_source, "pre-submit reconcile")
            self.assertEqual(outcome.submit_result["submission_state"], "accepted")
            self.assertIsNone(outcome.submit_result["next_action"])
            self.assertEqual(session.submit_result, outcome.submit_result)
            self.assertEqual(session.watch_result, outcome.watch_result)
            self.assertEqual(
                node_client.calls,
                [
                    (
                        "watch_transaction",
                        {
                            "txn_hash": "0xhash",
                            "timeout_seconds": 30,
                            "min_confirmed_blocks": 2,
                        },
                    )
                ],
            )
            self.assertIsNone(store.unresolved_for_session(session))

    def test_failed_state_replace_keeps_existing_state_file(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            state_path = Path(temp_dir) / "transfer-state.json"
            store = TransferStateStore(state_path)
            original = state_path.read_text(encoding="utf-8")

            with patch("transfer_state.os.replace", side_effect=OSError("replace failed")):
                with self.assertRaisesRegex(OSError, "replace failed"):
                    store._write_unlocked(
                        {
                            **state_default(),
                            "prepared_transactions": {"payload": {"txn": "prepared"}},
                        }
                    )

            self.assertEqual(state_path.read_text(encoding="utf-8"), original)
            self.assertEqual(list(state_path.parent.glob(f".{state_path.name}.*.tmp")), [])


if __name__ == "__main__":
    unittest.main()
