#!/usr/bin/env python3
from __future__ import annotations

import re
import time
from dataclasses import dataclass
from typing import Any, Callable, Protocol


CANONICAL_STC_TOKEN_CODE = "0x1::STC::STC"
LEGACY_STC_TOKEN_CODE = "0x1::starcoin_coin::STC"
STC_TOKEN_CODE_ALIASES = {
    CANONICAL_STC_TOKEN_CODE.lower(),
    LEGACY_STC_TOKEN_CODE.lower(),
}
STC_SCALE = 1_000_000_000
STC_AMOUNT_PATTERN = re.compile(r"^(?P<whole>\d+)(?:\.(?P<fraction>\d{1,9}))?$")
TERMINAL_REQUEST_STATUSES = {"approved", "rejected", "cancelled", "expired", "failed"}


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
    return token_code.strip().lower() in STC_TOKEN_CODE_ALIASES


def normalize_transfer_amount(amount: str, amount_unit: str, token_code: str) -> TransferAmount:
    value = amount.strip()
    if amount_unit == "raw":
        if not value.isdigit():
            raise ValueError("raw transfer amount must be a non-negative integer string")
        return TransferAmount(
            input_amount=amount,
            input_unit=amount_unit,
            raw_amount=value,
            display_amount=f"{value} raw units",
            token_code=token_code,
        )
    if not is_stc_token_code(token_code):
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
        token_code=token_code,
    )


def summarize_watch_result(watch_result: dict[str, Any]) -> list[tuple[str, str]]:
    status_summary = watch_result.get("status_summary") or {}
    return [
        ("Confirmed", str(watch_result.get("confirmed"))),
        ("Found", str(status_summary.get("found"))),
        ("VM Status", str(status_summary.get("vm_status"))),
    ]


def submit_recovery_hint(
    submit_result: dict[str, Any], watch_result: dict[str, Any] | None
) -> str | None:
    next_action = submit_result.get("next_action")
    submission_state = submit_result.get("submission_state")
    if next_action == "reconcile_by_txn_hash":
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
    ):
        self.node_client = node_client
        self.wallet_client = wallet_client
        self.chain_id = chain_id
        self.network = network
        self.genesis_hash = genesis_hash

    def prepare_session(
        self,
        *,
        wallet_instance_id: str,
        sender: str,
        receiver: str,
        amount: str,
        amount_unit: str,
        token_code: str,
    ) -> TransferSession:
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
                "token_code": token_code,
            },
        )
        return TransferSession(
            sender=sender,
            receiver=receiver,
            wallet_instance_id=wallet_instance_id,
            chain_id=self.chain_id,
            network=self.network,
            genesis_hash=self.genesis_hash,
            amount=normalized_amount,
            wallet_instances=wallet_instances,
            wallet_accounts=wallet_accounts,
            public_key=public_key_result["public_key"],
            prepare_result=prepare_result,
        )

    def confirmation_rows(self, session: TransferSession) -> list[tuple[str, str]]:
        rows = [
            ("Network", f"{session.network} ({session.chain_id})"),
            ("Genesis", session.genesis_hash),
            ("Wallet Instance", session.wallet_instance_id),
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
            rows.append(("Amount", session.amount.raw_amount))
        rows.extend(
            [
                ("Token", session.amount.token_code),
                ("Simulation", session.prepare_result["simulation_status"]),
                ("Prepared At", session.prepare_result["prepared_at"]),
            ]
        )
        return rows

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
        blocking: bool = True,
    ) -> TransferSubmitOutcome:
        if session.signed_txn_bcs_hex is None:
            raise RuntimeError("transfer session does not have signed_txn_bcs_hex yet")
        submit_result = self.node_client.call_tool(
            "submit_signed_transaction",
            {
                "signed_txn_bcs_hex": session.signed_txn_bcs_hex,
                "prepared_chain_context": session.prepare_result["chain_context"],
                "blocking": blocking,
                "timeout_seconds": timeout_seconds,
            },
        )
        session.submit_result = submit_result
        watch_result = submit_result.get("watch_result")
        watch_source = "submit" if watch_result is not None else None
        if submit_result["next_action"] == "reconcile_by_txn_hash":
            watch_result = self.node_client.call_tool(
                "watch_transaction",
                {
                    "txn_hash": submit_result["txn_hash"],
                    "timeout_seconds": timeout_seconds,
                },
            )
            watch_source = "reconcile"
        elif submit_result["submission_state"] == "accepted" and watch_result is None:
            watch_result = self.node_client.call_tool(
                "watch_transaction",
                {
                    "txn_hash": submit_result["txn_hash"],
                    "timeout_seconds": timeout_seconds,
                },
            )
            watch_source = "follow-up watch"
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
