#!/usr/bin/env python3
from __future__ import annotations

import fcntl
import json
import os
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any

from workflow_audit import payload_sha256, session_token_code, utc_now_rfc3339

if TYPE_CHECKING:
    from transfer_controller import TransferSession


STATE_VERSION = 1
DEFAULT_MAX_PREPARED_RECORDS = 128
DEFAULT_MAX_UNRESOLVED_RECORDS = 128


@dataclass(frozen=True)
class UnresolvedSubmissionRecord:
    payload_sha256: str
    txn_hash: str
    recorded_at: str
    submission_state: str | None
    next_action: str | None


def session_payload_sha256(session: TransferSession) -> str:
    raw_txn_bcs_hex = session.prepare_result.get("raw_txn_bcs_hex")
    if not isinstance(raw_txn_bcs_hex, str) or not raw_txn_bcs_hex.strip():
        raise ValueError("prepare_result is missing raw_txn_bcs_hex")
    return payload_sha256(raw_txn_bcs_hex)


def chain_identity(chain_context: dict[str, Any]) -> tuple[str, str, str]:
    return (
        str(chain_context.get("chain_id")),
        str(chain_context.get("network") or "").lower(),
        str(chain_context.get("genesis_hash") or "").lower(),
    )


def state_default() -> dict[str, Any]:
    return {
        "version": STATE_VERSION,
        "prepared_transactions": {},
        "unresolved_submissions": {},
    }


class TransferStateStore:
    def __init__(
        self,
        path: Path,
        *,
        max_prepared_records: int = DEFAULT_MAX_PREPARED_RECORDS,
        max_unresolved_records: int = DEFAULT_MAX_UNRESOLVED_RECORDS,
    ):
        self.path = Path(path).expanduser().resolve()
        self.lock_path = self.path.with_name(f"{self.path.name}.lock")
        self.max_prepared_records = max_prepared_records
        self.max_unresolved_records = max_unresolved_records
        self.path.parent.mkdir(parents=True, exist_ok=True)
        if not self.path.exists():
            self._write_unlocked(state_default())
        if not self.lock_path.exists():
            self.lock_path.touch(mode=0o600)
        self._chmod_private(self.path)
        self._chmod_private(self.lock_path)

    def record_prepared(self, session: TransferSession) -> None:
        payload_hash = session_payload_sha256(session)
        prepare_result = session.prepare_result
        record = {
            "payload_sha256": payload_hash,
            "recorded_at": utc_now_rfc3339(),
            "prepared_at": prepare_result.get("prepared_at"),
            "simulation_status": prepare_result.get("simulation_status"),
            "transaction_kind": prepare_result.get("transaction_kind"),
            "chain_context": prepare_result.get("chain_context"),
            "backend_id": session.wallet_instance_id,
            "sender": session.sender,
            "receiver": session.receiver,
            "token_code": session_token_code(session),
            "raw_amount": session.amount.raw_amount,
            "display_amount": session.amount.display_amount,
        }
        with self._locked_state() as state:
            prepared = self._prepared_records(state)
            prepared[payload_hash] = record
            prune_records(prepared, self.max_prepared_records)

    def require_prepared_for_submit(self, session: TransferSession) -> None:
        payload_hash = session_payload_sha256(session)
        state = self.read()
        record = self._prepared_records(state).get(payload_hash)
        if record is None:
            raise RuntimeError(
                "no persisted prepared-transfer attestation was found for this payload; "
                "prepare and dry-run the transfer again before requesting a signature"
            )
        simulation_status = record.get("simulation_status")
        if simulation_status != "performed":
            raise RuntimeError(
                "persisted prepared-transfer attestation is not signable: "
                f"simulation_status={simulation_status!r}"
            )
        stored_context = record.get("chain_context")
        current_context = session.prepare_result.get("chain_context")
        if not isinstance(stored_context, dict) or not isinstance(current_context, dict):
            raise RuntimeError("persisted prepared-transfer attestation is missing chain_context")
        if chain_identity(stored_context) != chain_identity(current_context):
            raise RuntimeError(
                "persisted prepared-transfer attestation does not match the current "
                "prepared chain context"
            )
        expected_fields = {
            "backend_id": session.wallet_instance_id,
            "sender": session.sender,
            "receiver": session.receiver,
            "token_code": session_token_code(session),
            "raw_amount": session.amount.raw_amount,
        }
        mismatched_fields = [
            name for name, expected in expected_fields.items() if record.get(name) != expected
        ]
        if mismatched_fields:
            fields = ", ".join(sorted(mismatched_fields))
            raise RuntimeError(
                "persisted prepared-transfer attestation does not match the current "
                f"session metadata: {fields}"
            )

    def unresolved_for_session(
        self, session: TransferSession
    ) -> UnresolvedSubmissionRecord | None:
        payload_hash = session_payload_sha256(session)
        record = self._unresolved_records(self.read()).get(payload_hash)
        if not isinstance(record, dict):
            return None
        txn_hash = record.get("txn_hash")
        if not isinstance(txn_hash, str) or not txn_hash.strip():
            return None
        return UnresolvedSubmissionRecord(
            payload_sha256=payload_hash,
            txn_hash=txn_hash,
            recorded_at=str(record.get("recorded_at") or ""),
            submission_state=optional_str(record.get("submission_state")),
            next_action=optional_str(record.get("next_action")),
        )

    def record_unresolved_submission(
        self, session: TransferSession, submit_result: dict[str, Any]
    ) -> None:
        payload_hash = session_payload_sha256(session)
        txn_hash = submit_result.get("txn_hash")
        if not isinstance(txn_hash, str) or not txn_hash.strip():
            return
        with self._locked_state() as state:
            unresolved = self._unresolved_records(state)
            unresolved[payload_hash] = {
                "payload_sha256": payload_hash,
                "txn_hash": txn_hash,
                "recorded_at": utc_now_rfc3339(),
                "submission_state": submit_result.get("submission_state"),
                "next_action": submit_result.get("next_action"),
            }
            prune_records(unresolved, self.max_unresolved_records)

    def clear_unresolved_submission(self, session: TransferSession) -> None:
        payload_hash = session_payload_sha256(session)
        with self._locked_state() as state:
            self._unresolved_records(state).pop(payload_hash, None)

    def read(self) -> dict[str, Any]:
        try:
            payload = json.loads(self.path.read_text(encoding="utf-8"))
        except (FileNotFoundError, json.JSONDecodeError):
            return state_default()
        if not isinstance(payload, dict):
            return state_default()
        payload.setdefault("version", STATE_VERSION)
        payload.setdefault("prepared_transactions", {})
        payload.setdefault("unresolved_submissions", {})
        return payload

    def _write_unlocked(self, state: dict[str, Any]) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        payload = json.dumps(state, ensure_ascii=True, indent=2, sort_keys=True) + "\n"
        temp_path: Path | None = None
        try:
            with tempfile.NamedTemporaryFile(
                "w",
                encoding="utf-8",
                dir=self.path.parent,
                prefix=f".{self.path.name}.",
                suffix=".tmp",
                delete=False,
            ) as handle:
                temp_path = Path(handle.name)
                handle.write(payload)
                handle.flush()
                os.fsync(handle.fileno())
            self._chmod_private(temp_path)
            os.replace(temp_path, self.path)
            temp_path = None
            self._fsync_directory(self.path.parent)
        finally:
            if temp_path is not None:
                try:
                    temp_path.unlink(missing_ok=True)
                except OSError:
                    pass
        self._chmod_private(self.path)

    def _locked_state(self) -> LockedState:
        return LockedState(self)

    def _prepared_records(self, state: dict[str, Any]) -> dict[str, Any]:
        value = state.setdefault("prepared_transactions", {})
        if not isinstance(value, dict):
            value = {}
            state["prepared_transactions"] = value
        return value

    def _unresolved_records(self, state: dict[str, Any]) -> dict[str, Any]:
        value = state.setdefault("unresolved_submissions", {})
        if not isinstance(value, dict):
            value = {}
            state["unresolved_submissions"] = value
        return value

    @staticmethod
    def _chmod_private(path: Path) -> None:
        try:
            path.chmod(0o600)
        except OSError:
            pass

    @staticmethod
    def _fsync_directory(path: Path) -> None:
        flags = os.O_RDONLY
        if hasattr(os, "O_DIRECTORY"):
            flags |= os.O_DIRECTORY
        try:
            fd = os.open(path, flags)
        except OSError:
            return
        try:
            os.fsync(fd)
        except OSError:
            pass
        finally:
            os.close(fd)


class LockedState:
    def __init__(self, store: TransferStateStore):
        self.store = store
        self.lock_handle = None
        self.state: dict[str, Any] | None = None

    def __enter__(self) -> dict[str, Any]:
        self.lock_handle = self.store.lock_path.open("a", encoding="utf-8")
        fcntl.flock(self.lock_handle.fileno(), fcntl.LOCK_EX)
        self.state = self.store.read()
        return self.state

    def __exit__(self, exc_type: object, exc: object, traceback: object) -> None:
        if self.lock_handle is None:
            raise RuntimeError("LockedState exited without lock_handle")
        try:
            if exc_type is None:
                if self.state is None:
                    raise RuntimeError("LockedState state is unexpectedly None on clean exit")
                self.store._write_unlocked(self.state)
        finally:
            fcntl.flock(self.lock_handle.fileno(), fcntl.LOCK_UN)
            self.lock_handle.close()


def optional_str(value: Any) -> str | None:
    if isinstance(value, str):
        return value
    return None


def prune_records(records: dict[str, Any], max_records: int) -> None:
    if max_records <= 0:
        records.clear()
        return
    if len(records) <= max_records:
        return
    ordered = sorted(
        records.items(),
        key=lambda item: str((item[1] or {}).get("recorded_at") or ""),
    )
    for key, _ in ordered[: len(records) - max_records]:
        records.pop(key, None)
