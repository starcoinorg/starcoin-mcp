#!/usr/bin/env python3
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from workflow_audit import WorkflowAuditLogger, summarize_audit_records


class WorkflowAuditTests(unittest.TestCase):
    def test_create_account_audit_records_terminal_address_without_public_key(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            log_path = Path(temp_dir) / "audit.jsonl"
            logger = WorkflowAuditLogger(log_path)
            logger.record_create_account_request_created(
                wallet_instance_id="local-default",
                request={
                    "request_id": "req-1",
                    "client_request_id": "create-account-1",
                    "status": "pending",
                    "expires_at": "2026-04-14T00:00:00Z",
                },
                client_context="starcoin-create-account",
                display_hint="Create local account",
            )
            logger.record_create_account_request_terminal(
                wallet_instance_id="local-default",
                request_id="req-1",
                status={
                    "status": "approved",
                    "result": {
                        "kind": "created_account",
                        "address": "0x1",
                        "public_key": "0xpub-should-not-be-logged",
                        "curve": "ed25519",
                        "is_default": False,
                        "is_locked": True,
                    },
                },
            )
            records = [
                json.loads(line)
                for line in log_path.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]

        self.assertEqual(records[0]["event"], "create_account_request_created")
        self.assertEqual(records[1]["event"], "create_account_request_terminal")
        self.assertEqual(records[1]["created_address"], "0x1")
        self.assertNotIn("public_key", records[1])

    def test_audit_summary_excludes_raw_payload_fields(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            log_path = Path(temp_dir) / "audit.jsonl"
            log_path.write_text(
                "\n".join(
                    [
                        json.dumps(
                            {
                                "recorded_at": "2026-04-14T00:00:00Z",
                                "event": "sign_request_terminal",
                                "request_id": "req-1",
                                "payload_sha256": "abc",
                                "backend_id": "local-default",
                                "terminal_status": "approved",
                                "raw_txn_bcs_hex": "0xraw-should-not-appear",
                                "signed_txn_bcs_hex": "0xsigned-should-not-appear",
                            }
                        )
                    ]
                )
                + "\n",
                encoding="utf-8",
            )

            summary = summarize_audit_records(log_path)

        self.assertEqual(summary[0]["request_id"], "req-1")
        self.assertEqual(summary[0]["payload_sha256"], "abc")
        self.assertNotIn("raw_txn_bcs_hex", summary[0])
        self.assertNotIn("signed_txn_bcs_hex", summary[0])


if __name__ == "__main__":
    unittest.main()
