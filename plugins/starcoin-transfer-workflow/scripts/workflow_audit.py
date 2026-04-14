#!/usr/bin/env python3
from __future__ import annotations

import fcntl
import hashlib
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from transfer_controller import TransferSession, TransferSubmitOutcome
    from transfer_host import TransferPreflightReport


def utc_now_rfc3339() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def payload_sha256(raw_txn_bcs_hex: str) -> str:
    normalized = raw_txn_bcs_hex.strip()
    if normalized.startswith("0x"):
        normalized = normalized[2:]
    if len(normalized) % 2 == 0:
        try:
            payload = bytes.fromhex(normalized)
        except ValueError:
            payload = raw_txn_bcs_hex.encode("utf-8")
    else:
        payload = raw_txn_bcs_hex.encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def session_token_code(session: TransferSession) -> str:
    facts = session.prepare_result.get("execution_facts")
    if isinstance(facts, dict):
        transfer_token_code = facts.get("transfer_token_code")
        if isinstance(transfer_token_code, str) and transfer_token_code.strip():
            return transfer_token_code.strip()
    transaction_summary = session.prepare_result.get("transaction_summary")
    if isinstance(transaction_summary, dict):
        token_code = transaction_summary.get("token_code")
        if isinstance(token_code, str) and token_code.strip():
            return token_code.strip()
    return session.amount.token_code


class WorkflowAuditLogger:
    def __init__(self, path: Path):
        self.path = Path(path).expanduser().resolve()
        self.lock_path = self.path.with_name(f"{self.path.name}.lock")
        self.path.parent.mkdir(parents=True, exist_ok=True)
        if not self.path.exists():
            self.path.touch(mode=0o600)
        if not self.lock_path.exists():
            self.lock_path.touch(mode=0o600)
        try:
            self.path.chmod(0o600)
        except OSError:
            pass
        try:
            self.lock_path.chmod(0o600)
        except OSError:
            pass

    def record_preflight(
        self,
        session: TransferSession,
        report: TransferPreflightReport,
    ) -> None:
        self._append(
            {
                **self._session_metadata(session),
                "event": "preflight_preview",
                "risk_codes": [risk.code for risk in report.risk_labels],
                "blocking_risk_count": report.blocking_risk_count,
                "warning_risk_count": report.warning_risk_count,
                "info_risk_count": report.info_risk_count,
            }
        )

    def record_host_decision(
        self,
        session: TransferSession,
        *,
        decision: str,
        reason: str,
        report: TransferPreflightReport | None = None,
    ) -> None:
        payload = {
            **self._session_metadata(session),
            "event": "host_decision",
            "decision": decision,
            "reason": reason,
        }
        if report is not None:
            payload["risk_codes"] = [risk.code for risk in report.risk_labels]
        self._append(payload)

    def record_sign_request_created(
        self,
        session: TransferSession,
        request: dict[str, Any],
    ) -> None:
        self._append(
            {
                **self._session_metadata(session),
                "event": "sign_request_created",
                "request_id": request.get("request_id"),
                "request_status": request.get("status"),
                "backend_id": session.wallet_instance_id,
            }
        )

    def record_sign_request_terminal(
        self,
        session: TransferSession,
        status: dict[str, Any],
    ) -> None:
        self._append(
            {
                **self._session_metadata(session),
                "event": "sign_request_terminal",
                "request_id": (session.request or {}).get("request_id"),
                "terminal_status": status.get("status"),
                "error_code": status.get("error_code"),
                "error_message": status.get("error_message"),
            }
        )

    def record_submission(
        self,
        session: TransferSession,
        outcome: TransferSubmitOutcome,
    ) -> None:
        watch_result = outcome.watch_result or {}
        self._append(
            {
                **self._session_metadata(session),
                "event": "submission_result",
                "request_id": (session.request or {}).get("request_id"),
                "txn_hash": outcome.submit_result.get("txn_hash"),
                "submission_state": outcome.submit_result.get("submission_state"),
                "submission_next_action": outcome.submit_result.get("next_action"),
                "confirmed": bool(watch_result.get("confirmed")),
                "watch_source": outcome.watch_source,
                "guidance": outcome.guidance,
            }
        )

    def record_create_account_request_created(
        self,
        *,
        wallet_instance_id: str,
        request: dict[str, Any],
        client_context: str | None = None,
        display_hint: str | None = None,
    ) -> None:
        self._append(
            {
                "event": "create_account_request_created",
                "backend_id": wallet_instance_id,
                "request_id": request.get("request_id"),
                "request_status": request.get("status"),
                "client_request_id": request.get("client_request_id"),
                "client_context": client_context,
                "display_hint": display_hint,
                "expires_at": request.get("expires_at"),
            }
        )

    def record_create_account_request_terminal(
        self,
        *,
        wallet_instance_id: str,
        request_id: str | None,
        status: dict[str, Any],
    ) -> None:
        result = status.get("result")
        created_address = None
        is_default = None
        is_locked = None
        if isinstance(result, dict):
            created_address = result.get("address")
            is_default = result.get("is_default")
            is_locked = result.get("is_locked")
        self._append(
            {
                "event": "create_account_request_terminal",
                "backend_id": wallet_instance_id,
                "request_id": request_id,
                "terminal_status": status.get("status"),
                "created_address": created_address,
                "is_default": is_default,
                "is_locked": is_locked,
                "error_code": status.get("error_code"),
                "error_message": status.get("error_message"),
            }
        )

    def _session_metadata(self, session: TransferSession) -> dict[str, Any]:
        raw_txn_bcs_hex = session.prepare_result.get("raw_txn_bcs_hex")
        if not isinstance(raw_txn_bcs_hex, str) or not raw_txn_bcs_hex.strip():
            raise ValueError("prepare_result is missing raw_txn_bcs_hex for audit logging")
        return {
            "backend_id": session.wallet_instance_id,
            "network": session.network,
            "chain_id": session.chain_id,
            "sender": session.sender,
            "receiver": session.receiver,
            "token_code": session_token_code(session),
            "raw_amount": session.amount.raw_amount,
            "display_amount": session.amount.display_amount,
            "prepared_at": session.prepare_result.get("prepared_at"),
            "payload_sha256": payload_sha256(raw_txn_bcs_hex),
        }

    def _append(self, record: dict[str, Any]) -> None:
        payload = {"recorded_at": utc_now_rfc3339(), **record}
        with self.lock_path.open("a", encoding="utf-8") as lock_handle:
            fcntl.flock(lock_handle.fileno(), fcntl.LOCK_EX)
            try:
                with self.path.open("a", encoding="utf-8") as handle:
                    json.dump(payload, handle, ensure_ascii=True, separators=(",", ":"))
                    handle.write("\n")
                    handle.flush()
            finally:
                fcntl.flock(lock_handle.fileno(), fcntl.LOCK_UN)


TransferAuditLogger = WorkflowAuditLogger
