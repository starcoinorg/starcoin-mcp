#!/usr/bin/env python3
from __future__ import annotations

import re
import time
from dataclasses import dataclass
from typing import Any, Callable, Protocol

from transfer_host import (
    TransferPreflightReport,
    analyze_preflight,
    build_preflight_rows,
    build_risk_rows,
    has_blocking_risks,
)
from transfer_state import TransferStateStore


VM1_STC_TOKEN_CODE = "0x1::STC::STC"
# Default STC transfers follow the vm_profile=auto runtime, which prefers VM2.
CANONICAL_STC_TOKEN_CODE = "0x1::starcoin_coin::STC"
STARCOIN_COIN_STC_TOKEN_CODE = CANONICAL_STC_TOKEN_CODE
DEFAULT_VM_PROFILE = "auto"
VM1_ONLY_VM_PROFILE = "vm1_only"
VM2_ONLY_VM_PROFILE = "vm2_only"
VM_PROFILE_ALIASES = {
    DEFAULT_VM_PROFILE: DEFAULT_VM_PROFILE,
    "vm1-only": VM1_ONLY_VM_PROFILE,
    VM1_ONLY_VM_PROFILE: VM1_ONLY_VM_PROFILE,
    "vm2-only": VM2_ONLY_VM_PROFILE,
    VM2_ONLY_VM_PROFILE: VM2_ONLY_VM_PROFILE,
}
VM_PROFILE_STC_TOKEN_DEFAULTS = {
    DEFAULT_VM_PROFILE: CANONICAL_STC_TOKEN_CODE,
    VM1_ONLY_VM_PROFILE: VM1_STC_TOKEN_CODE,
    VM2_ONLY_VM_PROFILE: CANONICAL_STC_TOKEN_CODE,
}
STC_TOKEN_CODE_ALIASES = {
    CANONICAL_STC_TOKEN_CODE.lower(),
    VM1_STC_TOKEN_CODE.lower(),
    STARCOIN_COIN_STC_TOKEN_CODE.lower(),
}
DEFAULT_MIN_CONFIRMED_BLOCKS = 2
STC_SCALE = 1_000_000_000
STC_AMOUNT_PATTERN = re.compile(r"^(?P<whole>\d+)(?:\.(?P<fraction>\d{1,9}))?$")
TERMINAL_REQUEST_STATUSES = {"approved", "rejected", "cancelled", "expired", "failed"}
REQUIRED_PREPARE_FIELDS = (
    "transaction_kind",
    "raw_txn_bcs_hex",
    "chain_context",
    "prepared_at",
    "simulation_status",
    "next_action",
)


class ToolClient(Protocol):
    def call_tool(self, name: str, arguments: dict[str, Any] | None = None) -> dict[str, Any]:
        ...


@dataclass(frozen=True)
class TransferAmount:
    input_amount: str
    input_unit: str
    raw_amount: str
    display_amount: str
    token_code: str


@dataclass
class TransferSession:
    sender: str
    receiver: str
    wallet_instance_id: str
    vm_profile: str
    chain_id: int
    network: str
    genesis_hash: str
    amount: TransferAmount
    wallet_instances: dict[str, Any]
    wallet_accounts: dict[str, Any]
    public_key: str
    prepare_result: dict[str, Any]
    request: dict[str, Any] | None = None
    request_status: dict[str, Any] | None = None
    signed_txn_bcs_hex: str | None = None
    submit_result: dict[str, Any] | None = None
    watch_result: dict[str, Any] | None = None


@dataclass(frozen=True)
class TransferSubmitOutcome:
    submit_result: dict[str, Any]
    watch_result: dict[str, Any] | None
    watch_source: str | None
    success: bool
    guidance: str | None


def is_stc_token_code(token_code: str) -> bool:
    return normalize_token_code(token_code).lower() in STC_TOKEN_CODE_ALIASES


def normalize_token_code(token_code: str) -> str:
    normalized = token_code.strip()
    if not normalized:
        raise ValueError("token code must not be empty")
    return normalized


def normalize_vm_profile(vm_profile: str) -> str:
    normalized = vm_profile.strip().lower().replace("-", "_")
    resolved = VM_PROFILE_ALIASES.get(normalized)
    if resolved is None:
        raise ValueError(
            "vm profile must be one of auto, vm1_only, or vm2_only"
        )
    return resolved


def default_token_code_for_vm_profile(vm_profile: str) -> str:
    return VM_PROFILE_STC_TOKEN_DEFAULTS[normalize_vm_profile(vm_profile)]


def resolve_token_code(token_code: str | None, vm_profile: str) -> str:
    if token_code is None:
        return default_token_code_for_vm_profile(vm_profile)
    return normalize_token_code(token_code)


def normalize_min_confirmed_blocks(min_confirmed_blocks: int | None) -> int:
    if min_confirmed_blocks is None:
        return DEFAULT_MIN_CONFIRMED_BLOCKS
    return max(1, min_confirmed_blocks)


def describe_confirmation_depth(min_confirmed_blocks: int) -> str:
    normalized = normalize_min_confirmed_blocks(min_confirmed_blocks)
    if normalized == 1:
        return "1 block (the inclusion block only)"
    additional_blocks = normalized - 1
    return f"{normalized} blocks (the inclusion block plus {additional_blocks} more)"


def normalize_transfer_amount(amount: str, amount_unit: str, token_code: str) -> TransferAmount:
    value = amount.strip()
    normalized_token_code = normalize_token_code(token_code)
    if amount_unit == "raw":
        if not value.isdigit():
            raise ValueError("raw transfer amount must be a non-negative integer string")
        return TransferAmount(
            input_amount=amount,
            input_unit=amount_unit,
            raw_amount=value,
            display_amount=f"{value} raw units",
            token_code=normalized_token_code,
        )
    if not is_stc_token_code(normalized_token_code):
        raise ValueError("--amount-unit stc is only supported for STC token codes")
    match = STC_AMOUNT_PATTERN.fullmatch(value)
    if match is None:
        raise ValueError("human-readable STC amount must look like 1, 1.5, or 1.234567890")
    raw_amount = int(match.group("whole")) * STC_SCALE
    fraction = match.group("fraction")
    if fraction:
        raw_amount += int(fraction.ljust(9, "0"))
    return TransferAmount(
        input_amount=amount,
        input_unit=amount_unit,
        raw_amount=str(raw_amount),
        display_amount=f"{value} STC",
        token_code=normalized_token_code,
    )


def validate_prepare_result(prepare_result: dict[str, Any]) -> dict[str, Any]:
    missing = [field for field in REQUIRED_PREPARE_FIELDS if field not in prepare_result]
    if missing:
        raise RuntimeError(
            "prepare_transfer returned an incomplete result: missing "
            + ", ".join(missing)
        )
    if prepare_result.get("simulation_status") == "failed":
        raise RuntimeError("prepare_transfer returned simulation_status = failed")
    if prepare_result.get("next_action") != "sign_transaction":
        raise RuntimeError(
            "prepare_transfer returned a non-signable next_action: "
            + str(prepare_result.get("next_action"))
        )
    return prepare_result


def summarize_watch_result(watch_result: dict[str, Any]) -> list[tuple[str, str]]:
    status_summary = watch_result.get("status_summary") or {}
    rows = [
        ("Confirmed", str(watch_result.get("confirmed"))),
        ("Found", str(status_summary.get("found"))),
        ("VM Status", str(status_summary.get("vm_status"))),
    ]
    if watch_result.get("confirmed_blocks") is not None:
        rows.append(("Confirmed Blocks", str(watch_result.get("confirmed_blocks"))))
    if watch_result.get("effective_min_confirmed_blocks") is not None:
        rows.append(
            (
                "Required Blocks",
                str(watch_result.get("effective_min_confirmed_blocks")),
            )
        )
    if watch_result.get("inclusion_block_number") is not None:
        rows.append(
            ("Inclusion Block", str(watch_result.get("inclusion_block_number")))
        )
    return rows


def submit_recovery_hint(
    submit_result: dict[str, Any], watch_result: dict[str, Any] | None
) -> str | None:
    next_action = submit_result.get("next_action")
    submission_state = submit_result.get("submission_state")
    if next_action == "reconcile_by_txn_hash":
        if watch_result is not None and bool(watch_result.get("confirmed")):
            return None
        return "Submission outcome was uncertain. Reconcile by txn hash before any retry."
    if next_action == "reprepare_then_resign":
        return "The prepared transaction is no longer fresh. Prepare again and request a new signature."
    if submission_state == "accepted" and (
        watch_result is None or not watch_result.get("confirmed", False)
    ):
        return "The transaction was submitted, but final confirmation was not observed yet."
    return None


class TransferController:
    def __init__(
        self,
        *,
        node_client: ToolClient,
        wallet_client: ToolClient,
        chain_id: int,
        network: str,
        genesis_hash: str,
        state_store: TransferStateStore | None = None,
    ):
        self.node_client = node_client
        self.wallet_client = wallet_client
        self.chain_id = chain_id
        self.network = network
        self.genesis_hash = genesis_hash
        self.state_store = state_store

    def prepare_session(
        self,
        *,
        wallet_instance_id: str,
        vm_profile: str,
        sender: str,
        receiver: str,
        amount: str,
        amount_unit: str,
        token_code: str,
    ) -> TransferSession:
        normalized_vm_profile = normalize_vm_profile(vm_profile)
        normalized_amount = normalize_transfer_amount(amount, amount_unit, token_code)
        wallet_instances = self.wallet_client.call_tool("wallet_list_instances")
        wallet_accounts = self.wallet_client.call_tool(
            "wallet_list_accounts",
            {"wallet_instance_id": wallet_instance_id, "include_public_key": True},
        )
        public_key_result = self.wallet_client.call_tool(
            "wallet_get_public_key",
            {
                "wallet_instance_id": wallet_instance_id,
                "address": sender,
            },
        )
        prepare_result = self.node_client.call_tool(
            "prepare_transfer",
            {
                "sender": sender,
                "sender_public_key": public_key_result["public_key"],
                "receiver": receiver,
                "amount": normalized_amount.raw_amount,
                "token_code": normalized_amount.token_code,
            },
        )
        prepare_result = validate_prepare_result(prepare_result)
        session = TransferSession(
            sender=sender,
            receiver=receiver,
            wallet_instance_id=wallet_instance_id,
            vm_profile=normalized_vm_profile,
            chain_id=self.chain_id,
            network=self.network,
            genesis_hash=self.genesis_hash,
            amount=normalized_amount,
            wallet_instances=wallet_instances,
            wallet_accounts=wallet_accounts,
            public_key=public_key_result["public_key"],
            prepare_result=prepare_result,
        )
        if self.state_store is not None:
            self.state_store.record_prepared(session)
        return session

    def collect_preflight_report(self, session: TransferSession) -> TransferPreflightReport:
        chain_status = self.node_client.call_tool("chain_status")
        node_health = self.node_client.call_tool("node_health")
        sender_overview = self.node_client.call_tool(
            "get_account_overview",
            {
                "address": session.sender,
                "include_resources": True,
            },
        )
        receiver_overview = self.node_client.call_tool(
            "get_account_overview",
            {"address": session.receiver},
        )
        return analyze_preflight(
            session,
            chain_status=chain_status,
            node_health=node_health,
            sender_overview=sender_overview,
            receiver_overview=receiver_overview,
        )

    def confirmation_rows(
        self,
        session: TransferSession,
        *,
        min_confirmed_blocks: int | None = None,
    ) -> list[tuple[str, str]]:
        prepared_token_code = (
            session.prepare_result.get("transaction_summary", {}).get("token_code")
            or session.amount.token_code
        )
        rows = [
            ("Network", f"{session.network} ({session.chain_id})"),
            ("Genesis", session.genesis_hash),
            ("Wallet Instance", session.wallet_instance_id),
            ("VM Profile", session.vm_profile),
            ("Known Wallets", str(len(session.wallet_instances["wallet_instances"]))),
            (
                "Visible Accounts",
                str(
                    sum(
                        len(group["accounts"])
                        for group in session.wallet_accounts["wallet_instances"]
                    )
                ),
            ),
            ("Sender", session.sender),
            ("Receiver", session.receiver),
        ]
        if session.amount.input_unit == "stc":
            rows.extend(
                [
                    ("Amount", session.amount.display_amount),
                    ("Raw Amount", session.amount.raw_amount),
                ]
            )
        else:
            rows.append(("Raw Amount", session.amount.raw_amount))
        rows.extend(
            [
                ("Token", prepared_token_code),
                ("Simulation", session.prepare_result["simulation_status"]),
                ("Prepared At", session.prepare_result["prepared_at"]),
            ]
        )
        rows.append(
            (
                "Confirm Depth",
                describe_confirmation_depth(
                    normalize_min_confirmed_blocks(min_confirmed_blocks)
                ),
            )
        )
        return rows

    def preflight_rows(
        self,
        session: TransferSession,
        report: TransferPreflightReport,
        *,
        min_confirmed_blocks: int | None = None,
    ) -> list[tuple[str, str]]:
        return build_preflight_rows(
            session,
            report,
            confirmation_depth=describe_confirmation_depth(
                normalize_min_confirmed_blocks(min_confirmed_blocks)
            ),
        )

    def risk_rows(self, report: TransferPreflightReport) -> list[tuple[str, str]]:
        return build_risk_rows(report)

    def has_blocking_risks(self, report: TransferPreflightReport) -> bool:
        return has_blocking_risks(report)

    def create_sign_request(
        self,
        session: TransferSession,
        *,
        client_request_id: str,
        ttl_seconds: int,
        client_context: str,
    ) -> dict[str, Any]:
        request = self.wallet_client.call_tool(
            "wallet_request_sign_transaction",
            {
                "client_request_id": client_request_id,
                "account_address": session.sender,
                "wallet_instance_id": session.wallet_instance_id,
                "chain_id": session.chain_id,
                "raw_txn_bcs_hex": session.prepare_result["raw_txn_bcs_hex"],
                "tx_kind": str(session.prepare_result["transaction_kind"]).lower(),
                "display_hint": f"Transfer {session.amount.display_amount} to {session.receiver}",
                "client_context": client_context,
                "ttl_seconds": ttl_seconds,
            },
        )
        session.request = request
        return request

    def wait_for_terminal_request(
        self,
        session: TransferSession,
        *,
        poll_interval_seconds: float = 1.0,
        on_status_change: Callable[[str], None] | None = None,
    ) -> dict[str, Any]:
        if session.request is None:
            raise RuntimeError("wallet request has not been created yet")
        last_status = None
        while True:
            status = self.wallet_client.call_tool(
                "wallet_get_request_status", {"request_id": session.request["request_id"]}
            )
            current = status["status"]
            if current != last_status and on_status_change is not None:
                on_status_change(current)
            last_status = current
            session.request_status = status
            if current in TERMINAL_REQUEST_STATUSES:
                if current == "approved":
                    result = status.get("result") or {}
                    session.signed_txn_bcs_hex = result.get("signed_txn_bcs_hex")
                return status
            time.sleep(poll_interval_seconds)

    def submit(
        self,
        session: TransferSession,
        *,
        timeout_seconds: int,
        min_confirmed_blocks: int | None = None,
        blocking: bool = True,
    ) -> TransferSubmitOutcome:
        if session.signed_txn_bcs_hex is None:
            raise RuntimeError("transfer session does not have signed_txn_bcs_hex yet")
        normalized_min_confirmed_blocks = normalize_min_confirmed_blocks(min_confirmed_blocks)
        if self.state_store is not None:
            unresolved = self.state_store.unresolved_for_session(session)
            if unresolved is not None:
                watch_result = self.node_client.call_tool(
                    "watch_transaction",
                    {
                        "txn_hash": unresolved.txn_hash,
                        "timeout_seconds": timeout_seconds,
                        "min_confirmed_blocks": normalized_min_confirmed_blocks,
                    },
                )
                submit_result = {
                    "txn_hash": unresolved.txn_hash,
                    "submission_state": "unknown",
                    "submitted": False,
                    "next_action": "reconcile_by_txn_hash",
                    "error_code": "submission_unknown",
                }
                if bool(watch_result.get("confirmed")):
                    submit_result.update(
                        {
                            "submission_state": "accepted",
                            "submitted": True,
                            "next_action": None,
                            "error_code": None,
                            "reconciled_from_unresolved": True,
                        }
                    )
                    self.state_store.clear_unresolved_submission(session)
                session.submit_result = submit_result
                session.watch_result = watch_result
                return TransferSubmitOutcome(
                    submit_result=submit_result,
                    watch_result=watch_result,
                    watch_source="pre-submit reconcile",
                    success=bool(watch_result.get("confirmed")),
                    guidance=submit_recovery_hint(submit_result, watch_result),
                )
            self.state_store.require_prepared_for_submit(session)
        submit_result = self.node_client.call_tool(
            "submit_signed_transaction",
            {
                "signed_txn_bcs_hex": session.signed_txn_bcs_hex,
                "prepared_chain_context": session.prepare_result["chain_context"],
                "blocking": blocking,
                "timeout_seconds": timeout_seconds,
                "min_confirmed_blocks": normalized_min_confirmed_blocks,
            },
        )
        session.submit_result = submit_result
        watch_result = submit_result.get("watch_result")
        watch_source = "submit" if watch_result is not None else None
        if (
            self.state_store is not None
            and submit_result.get("next_action") == "reconcile_by_txn_hash"
        ):
            self.state_store.record_unresolved_submission(session, submit_result)
        if blocking and submit_result["next_action"] == "reconcile_by_txn_hash":
            watch_result = self.node_client.call_tool(
                "watch_transaction",
                {
                    "txn_hash": submit_result["txn_hash"],
                    "timeout_seconds": timeout_seconds,
                    "min_confirmed_blocks": normalized_min_confirmed_blocks,
                },
            )
            watch_source = "reconcile"
            if self.state_store is not None and bool(watch_result.get("confirmed")):
                self.state_store.clear_unresolved_submission(session)
        elif (
            blocking
            and submit_result["submission_state"] == "accepted"
            and watch_result is None
        ):
            watch_result = self.node_client.call_tool(
                "watch_transaction",
                {
                    "txn_hash": submit_result["txn_hash"],
                    "timeout_seconds": timeout_seconds,
                    "min_confirmed_blocks": normalized_min_confirmed_blocks,
                },
            )
            watch_source = "follow-up watch"
        if (
            self.state_store is not None
            and watch_result is not None
            and bool(watch_result.get("confirmed"))
        ):
            self.state_store.clear_unresolved_submission(session)
        session.watch_result = watch_result
        guidance = submit_recovery_hint(submit_result, watch_result)
        success = (
            submit_result["submission_state"] == "accepted"
            and watch_result is not None
            and bool(watch_result.get("confirmed"))
        )
        return TransferSubmitOutcome(
            submit_result=submit_result,
            watch_result=watch_result,
            watch_source=watch_source,
            success=success,
            guidance=guidance,
        )

    def submit_rows(self, outcome: TransferSubmitOutcome) -> list[tuple[str, str]]:
        submit_result = outcome.submit_result
        rows = [
            ("Txn Hash", submit_result["txn_hash"]),
            ("Submission State", submit_result["submission_state"]),
            ("Submitted", str(submit_result["submitted"])),
            ("Next Action", submit_result["next_action"]),
        ]
        if submit_result.get("error_code") is not None:
            rows.append(("Error Code", str(submit_result["error_code"])))
        if submit_result.get("effective_timeout_seconds") is not None:
            rows.append(("Effective Timeout", str(submit_result["effective_timeout_seconds"])))
        if outcome.watch_result is not None and outcome.watch_source is not None:
            rows.append(("Watch Source", outcome.watch_source))
            rows.extend(summarize_watch_result(outcome.watch_result))
        if outcome.guidance:
            rows.append(("Guidance", outcome.guidance))
        if not outcome.success and outcome.watch_result is None:
            rows.append(
                (
                    "Guidance",
                    "No confirmation result is available yet. Check the txn hash before any retry.",
                )
            )
        return rows
