#!/usr/bin/env python3
from __future__ import annotations

import fcntl
import hashlib
import json
import re
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from transfer_controller import TransferSession, TransferSubmitOutcome


DEFAULT_GAS_TOKEN_CODE = "0x1::starcoin_coin::STC"
VM1_STC_TOKEN_CODE = "0x1::STC::STC"
HEX_ADDRESS_RE = re.compile(r"0x[0-9a-fA-F]+")
CANONICAL_STC_CODES = frozenset(
    code.lower()
    for code in (
        DEFAULT_GAS_TOKEN_CODE,
        VM1_STC_TOKEN_CODE,
    )
)
SEVERITY_ORDER = {
    "block": 0,
    "warn": 1,
    "info": 2,
}


@dataclass(frozen=True)
class TransferRiskLabel:
    code: str
    severity: str
    message: str


@dataclass(frozen=True)
class TransferPreflightReport:
    chain_status: dict[str, Any]
    node_health: dict[str, Any]
    sender_overview: dict[str, Any]
    receiver_overview: dict[str, Any]
    token_code: str
    gas_token_code: str
    sender_visible_in_wallet: bool
    prepared_sequence_number: int | None
    next_sequence_number_hint: int | None
    gas_unit_price: int | None
    max_gas_amount: int | None
    simulation_gas_used: int | None
    estimated_network_fee: int | None
    max_network_fee: int | None
    sender_token_balance: int | None
    sender_gas_balance: int | None
    sender_post_transfer_balance: int | None
    risk_labels: tuple[TransferRiskLabel, ...]

    @property
    def blocking_risk_count(self) -> int:
        return sum(1 for risk in self.risk_labels if risk.severity == "block")

    @property
    def warning_risk_count(self) -> int:
        return sum(1 for risk in self.risk_labels if risk.severity == "warn")

    @property
    def info_risk_count(self) -> int:
        return sum(1 for risk in self.risk_labels if risk.severity == "info")


def utc_now_rfc3339() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def coerce_int(value: Any) -> int | None:
    if value is None or isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, str):
        normalized = value.strip()
        if normalized.isdigit():
            return int(normalized)
    return None


def prefer_int(primary: int | None, fallback: int | None) -> int | None:
    if primary is not None:
        return primary
    return fallback


def extract_nested_int(value: Any, *paths: tuple[str, ...]) -> int | None:
    for path in paths:
        current = value
        for key in path:
            if not isinstance(current, dict):
                current = None
                break
            current = current.get(key)
        converted = coerce_int(current)
        if converted is not None:
            return converted
    return None


def extract_nested_str(value: Any, *paths: tuple[str, ...]) -> str | None:
    for path in paths:
        current = value
        for key in path:
            if not isinstance(current, dict):
                current = None
                break
            current = current.get(key)
        if isinstance(current, str) and current.strip():
            return current.strip()
    return None


def normalize_hex_address(match: re.Match[str]) -> str:
    hex_digits = match.group(0)[2:].lstrip("0") or "0"
    return f"0x{hex_digits.lower()}"


def normalize_token_code(token_code: str) -> str:
    return HEX_ADDRESS_RE.sub(normalize_hex_address, token_code.strip())


def is_stc_like_token(token_code: str) -> bool:
    return normalize_token_code(token_code).lower() in CANONICAL_STC_CODES


def token_codes_match(left: str, right: str) -> bool:
    normalized_left = normalize_token_code(left).lower()
    normalized_right = normalize_token_code(right).lower()
    if normalized_left == normalized_right:
        return True
    return normalized_left in CANONICAL_STC_CODES and normalized_right in CANONICAL_STC_CODES


def token_present_in_accepted_tokens(token_code: str, accepted_tokens: list[Any]) -> bool:
    normalized = normalize_token_code(token_code)
    for candidate in accepted_tokens:
        if isinstance(candidate, str) and token_codes_match(normalized, candidate):
            return True
    return False


def balance_entry_matches_token(entry: Any, token_code: str) -> bool:
    if not isinstance(entry, dict):
        return False
    normalized_token = normalize_token_code(token_code).lower()
    entry_name = normalize_token_code(str(entry.get("name") or "")).lower()
    if normalized_token and normalized_token in entry_name:
        return True
    token_label = extract_nested_str(
        entry,
        ("value", "json", "token"),
        ("json", "token"),
    )
    token_label_value = str(token_label or "").strip().lower()
    return (
        normalized_token in CANONICAL_STC_CODES
        and "fungible_asset::fungiblestore" in entry_name
        and token_label_value in ("", "stc")
    )


def extract_balance_amount(entry: Any) -> int | None:
    return extract_nested_int(
        entry,
        ("value", "json", "balance"),
        ("value", "json", "coin", "value"),
        ("json", "balance"),
        ("json", "coin", "value"),
    )


def find_balance_for_token(balances: list[Any], token_code: str) -> int | None:
    for balance in balances:
        if balance_entry_matches_token(balance, token_code):
            amount = extract_balance_amount(balance)
            if amount is not None:
                return amount
    return None


def sender_visible_in_wallet(wallet_accounts: dict[str, Any], sender: str) -> bool:
    normalized_sender = sender.lower()
    for group in wallet_accounts.get("wallet_instances", []):
        for account in group.get("accounts", []):
            if str(account.get("address") or "").lower() == normalized_sender:
                return True
    return False


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


def format_raw_units(value: int | None) -> str:
    if value is None:
        return "unknown"
    return f"{value} raw units"


def execution_facts(session: TransferSession) -> dict[str, Any]:
    value = session.prepare_result.get("execution_facts")
    return value if isinstance(value, dict) else {}


def selected_token_code(session: TransferSession) -> str:
    facts = execution_facts(session)
    transfer_token_code = facts.get("transfer_token_code")
    if isinstance(transfer_token_code, str) and transfer_token_code.strip():
        return normalize_token_code(transfer_token_code)
    transaction_summary = session.prepare_result.get("transaction_summary") or {}
    token_code = transaction_summary.get("token_code") or session.amount.token_code
    return normalize_token_code(str(token_code))


def selected_gas_token_code(session: TransferSession) -> str:
    facts = execution_facts(session)
    gas_token = facts.get("gas_token_code")
    if isinstance(gas_token, str) and gas_token.strip():
        return normalize_token_code(gas_token)
    raw_txn = session.prepare_result.get("raw_txn") or {}
    gas_token = extract_nested_str(
        raw_txn,
        ("gas_token_code",),
        ("gasTokenCode",),
        ("gas_token",),
        ("gasToken",),
    )
    if gas_token:
        return normalize_token_code(gas_token)
    token_code = selected_token_code(session)
    if is_stc_like_token(token_code):
        return token_code
    return DEFAULT_GAS_TOKEN_CODE


def sorted_risk_labels(risk_labels: list[TransferRiskLabel]) -> tuple[TransferRiskLabel, ...]:
    return tuple(
        sorted(
            risk_labels,
            key=lambda risk: (SEVERITY_ORDER.get(risk.severity, 99), risk.code),
        )
    )


def analyze_preflight(
    session: TransferSession,
    *,
    chain_status: dict[str, Any],
    node_health: dict[str, Any],
    sender_overview: dict[str, Any],
    receiver_overview: dict[str, Any],
) -> TransferPreflightReport:
    raw_amount = int(session.amount.raw_amount)
    token_code = selected_token_code(session)
    gas_token_code = selected_gas_token_code(session)
    facts = execution_facts(session)
    raw_txn = session.prepare_result.get("raw_txn") or {}
    simulation = session.prepare_result.get("simulation") or {}
    sender_visible = sender_visible_in_wallet(session.wallet_accounts, session.sender)
    prepared_sequence_number = prefer_int(
        coerce_int(facts.get("sequence_number")),
        extract_nested_int(
            raw_txn,
            ("sequence_number",),
            ("sequenceNumber",),
        ),
    )
    next_sequence_number_hint = coerce_int(sender_overview.get("next_sequence_number_hint"))
    gas_unit_price = prefer_int(
        coerce_int(facts.get("gas_unit_price")),
        extract_nested_int(
            raw_txn,
            ("gas_unit_price",),
            ("gasUnitPrice",),
        ),
    )
    max_gas_amount = prefer_int(
        coerce_int(facts.get("max_gas_amount")),
        extract_nested_int(
            raw_txn,
            ("max_gas_amount",),
            ("maxGasAmount",),
        ),
    )
    simulation_gas_used = extract_nested_int(
        simulation,
        ("gas_used",),
        ("gasUsed",),
    )
    estimated_network_fee = coerce_int(facts.get("estimated_network_fee"))
    if estimated_network_fee is None and gas_unit_price is not None and simulation_gas_used is not None:
        estimated_network_fee = gas_unit_price * simulation_gas_used
    max_network_fee = coerce_int(facts.get("estimated_max_network_fee"))
    if max_network_fee is None and gas_unit_price is not None and max_gas_amount is not None:
        max_network_fee = gas_unit_price * max_gas_amount

    balances = sender_overview.get("balances") or []
    sender_token_balance = find_balance_for_token(balances, token_code)
    sender_gas_balance = find_balance_for_token(balances, gas_token_code)
    sender_post_transfer_balance = None
    if sender_token_balance is not None:
        sender_post_transfer_balance = sender_token_balance - raw_amount
        if (
            estimated_network_fee is not None
            and token_codes_match(token_code, gas_token_code)
        ):
            sender_post_transfer_balance -= estimated_network_fee

    risk_labels: list[TransferRiskLabel] = []
    prepared_context = session.prepare_result.get("chain_context") or {}
    chain_mismatch = (
        str(chain_status.get("chain_id")) != str(prepared_context.get("chain_id"))
        or str(chain_status.get("network") or "") != str(prepared_context.get("network") or "")
        or str(chain_status.get("genesis_hash") or "").lower()
        != str(prepared_context.get("genesis_hash") or "").lower()
    )
    if chain_mismatch:
        risk_labels.append(
            TransferRiskLabel(
                code="chain_context_mismatch",
                severity="block",
                message="Current chain status no longer matches the prepared chain context.",
            )
        )

    if not bool(node_health.get("node_available", True)):
        risk_labels.append(
            TransferRiskLabel(
                code="rpc_unavailable",
                severity="block",
                message="The node health probe reports that the RPC endpoint is unavailable.",
            )
        )

    warnings = [str(warning) for warning in node_health.get("warnings") or [] if str(warning)]
    if warnings:
        risk_labels.append(
            TransferRiskLabel(
                code="rpc_health_warning",
                severity="warn",
                message="RPC health warnings: " + "; ".join(warnings),
            )
        )

    if session.sender.lower() == session.receiver.lower():
        risk_labels.append(
            TransferRiskLabel(
                code="sender_equals_receiver",
                severity="warn",
                message="Sender and receiver are the same address.",
            )
        )

    if not sender_visible:
        risk_labels.append(
            TransferRiskLabel(
                code="sender_not_visible_in_wallet",
                severity="warn",
                message="The sender address is not visible in wallet_list_accounts output.",
            )
        )

    if sender_token_balance is None:
        if token_present_in_accepted_tokens(token_code, sender_overview.get("accepted_tokens") or []):
            risk_labels.append(
                TransferRiskLabel(
                    code="sender_balance_unknown",
                    severity="warn",
                    message="The sender token appears in accepted_tokens, but its balance could not be derived.",
                )
            )
        else:
            risk_labels.append(
                TransferRiskLabel(
                    code="token_balance_not_visible",
                    severity="warn",
                    message=f"No sender balance entry matched token {token_code}.",
                )
            )
    elif sender_token_balance < raw_amount:
        risk_labels.append(
            TransferRiskLabel(
                code="insufficient_token_balance",
                severity="block",
                message=(
                    f"Sender token balance {sender_token_balance} is below transfer amount {raw_amount}."
                ),
            )
        )

    if estimated_network_fee is None:
        risk_labels.append(
            TransferRiskLabel(
                code="fee_estimate_unavailable",
                severity="info",
                message="Network fee could not be estimated from the prepared transaction and simulation output.",
            )
        )
    elif token_codes_match(token_code, gas_token_code):
        if sender_token_balance is not None and sender_token_balance < raw_amount + estimated_network_fee:
            risk_labels.append(
                TransferRiskLabel(
                    code="insufficient_balance_for_amount_and_fee",
                    severity="block",
                    message=(
                        "The sender balance does not cover both the transfer amount and the estimated fee."
                    ),
                )
            )
    else:
        if sender_gas_balance is None:
            risk_labels.append(
                TransferRiskLabel(
                    code="gas_balance_unknown",
                    severity="warn",
                    message=f"No sender balance entry matched gas token {gas_token_code}.",
                )
            )
        elif sender_gas_balance < estimated_network_fee:
            risk_labels.append(
                TransferRiskLabel(
                    code="insufficient_gas_balance",
                    severity="block",
                    message=(
                        f"Sender gas-token balance {sender_gas_balance} is below estimated fee {estimated_network_fee}."
                    ),
                )
            )

    if (
        prepared_sequence_number is not None
        and next_sequence_number_hint is not None
        and next_sequence_number_hint > prepared_sequence_number
    ):
        risk_labels.append(
            TransferRiskLabel(
                code="nonce_advanced_after_prepare",
                severity="warn",
                message=(
                    f"Next sequence hint {next_sequence_number_hint} is ahead of prepared nonce {prepared_sequence_number}."
                ),
            )
        )

    if not bool(receiver_overview.get("onchain_exists", False)):
        risk_labels.append(
            TransferRiskLabel(
                code="receiver_account_not_initialized",
                severity="info",
                message="Receiver account does not currently exist on-chain.",
            )
        )

    return TransferPreflightReport(
        chain_status=chain_status,
        node_health=node_health,
        sender_overview=sender_overview,
        receiver_overview=receiver_overview,
        token_code=token_code,
        gas_token_code=gas_token_code,
        sender_visible_in_wallet=sender_visible,
        prepared_sequence_number=prepared_sequence_number,
        next_sequence_number_hint=next_sequence_number_hint,
        gas_unit_price=gas_unit_price,
        max_gas_amount=max_gas_amount,
        simulation_gas_used=simulation_gas_used,
        estimated_network_fee=estimated_network_fee,
        max_network_fee=max_network_fee,
        sender_token_balance=sender_token_balance,
        sender_gas_balance=sender_gas_balance,
        sender_post_transfer_balance=sender_post_transfer_balance,
        risk_labels=sorted_risk_labels(risk_labels),
    )


def build_preflight_rows(
    session: TransferSession,
    report: TransferPreflightReport,
    *,
    confirmation_depth: str,
) -> list[tuple[str, str]]:
    rows = [
        ("Network", f"{session.network} ({session.chain_id})"),
        ("Genesis", session.genesis_hash),
        ("Wallet Instance", session.wallet_instance_id),
        ("Sender", session.sender),
        ("Receiver", session.receiver),
        ("Token", report.token_code),
        ("Amount", session.amount.display_amount),
        ("Raw Amount", session.amount.raw_amount),
        ("Sender Visible", str(report.sender_visible_in_wallet)),
        ("Sender Balance", format_raw_units(report.sender_token_balance)),
        ("Balance After Transfer", format_raw_units(report.sender_post_transfer_balance)),
        ("Prepared Nonce", str(report.prepared_sequence_number)),
        ("Next Nonce Hint", str(report.next_sequence_number_hint)),
        ("Gas Unit Price", str(report.gas_unit_price)),
        ("Max Gas Amount", str(report.max_gas_amount)),
        ("Estimated Fee", format_raw_units(report.estimated_network_fee)),
        ("Max Fee Ceiling", format_raw_units(report.max_network_fee)),
        ("Simulation", str(session.prepare_result.get("simulation_status"))),
        ("RPC Node Available", str(report.node_health.get("node_available"))),
        (
            "RPC Peers",
            str((report.node_health.get("peers_summary") or {}).get("count")),
        ),
        ("Confirm Depth", confirmation_depth),
        (
            "Risk Summary",
            (
                f"{report.blocking_risk_count} blocking, "
                f"{report.warning_risk_count} warnings, "
                f"{report.info_risk_count} infos"
            ),
        ),
    ]
    if not token_codes_match(report.token_code, report.gas_token_code):
        rows.extend(
            [
                ("Gas Token", report.gas_token_code),
                ("Gas Token Balance", format_raw_units(report.sender_gas_balance)),
            ]
        )
    warnings = [str(warning) for warning in report.node_health.get("warnings") or [] if str(warning)]
    if warnings:
        rows.append(("RPC Warnings", "; ".join(warnings)))
    return rows


def build_risk_rows(report: TransferPreflightReport) -> list[tuple[str, str]]:
    return [
        (f"{risk.severity.upper()} {risk.code}", risk.message)
        for risk in report.risk_labels
    ]


def has_blocking_risks(report: TransferPreflightReport) -> bool:
    return report.blocking_risk_count > 0


class TransferAuditLogger:
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

    def _session_metadata(self, session: TransferSession) -> dict[str, Any]:
        return {
            "backend_id": session.wallet_instance_id,
            "network": session.network,
            "chain_id": session.chain_id,
            "sender": session.sender,
            "receiver": session.receiver,
            "token_code": selected_token_code(session),
            "raw_amount": session.amount.raw_amount,
            "display_amount": session.amount.display_amount,
            "prepared_at": session.prepare_result.get("prepared_at"),
            "payload_sha256": payload_sha256(session.prepare_result["raw_txn_bcs_hex"]),
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
