#!/usr/bin/env python3
from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path
from typing import Any, Callable

from runtime_layout import resolve_wallet_runtime_dir
from starmaskd_client import StarmaskDaemonClient, resolve_socket_path
from workflow_audit import WorkflowAuditLogger


DEFAULT_TTL_SECONDS = 300
DEFAULT_CLIENT_CONTEXT = "starcoin-create-account"
TERMINAL_REQUEST_STATUSES = {"approved", "rejected", "cancelled", "expired", "failed"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Create a new wallet address through the local starmaskd approval flow."
    )
    parser.add_argument(
        "--socket-path",
        default=None,
        help="Explicit daemon socket path. Overrides wallet-runtime metadata discovery.",
    )
    parser.add_argument(
        "--wallet-runtime-dir",
        default=None,
        help="Wallet runtime directory used to discover wallet-runtime.json and the default audit path.",
    )
    parser.add_argument(
        "--wallet-instance-id",
        default=None,
        help="Target wallet instance id. If omitted, the script auto-selects a single available instance.",
    )
    parser.add_argument(
        "--client-request-id",
        default=None,
        help="Optional idempotency key for request.createAccount. Defaults to a time-based value.",
    )
    parser.add_argument(
        "--display-hint",
        default="Create local account",
        help="Human-readable hint shown on the wallet approval surface.",
    )
    parser.add_argument(
        "--client-context",
        default=DEFAULT_CLIENT_CONTEXT,
        help="Client context string stored with the wallet request.",
    )
    parser.add_argument(
        "--ttl-seconds",
        type=int,
        default=DEFAULT_TTL_SECONDS,
        help="Wallet request TTL in seconds.",
    )
    parser.add_argument(
        "--poll-interval-seconds",
        type=float,
        default=1.0,
        help="Polling interval for wallet_get_request_status.",
    )
    parser.add_argument(
        "--audit-log-path",
        default=None,
        help="Optional JSONL path for local account-creation audit records.",
    )
    return parser.parse_args()


def resolve_wallet_instance(
    wallet_instances: dict[str, Any],
    wallet_instance_id: str | None,
) -> dict[str, Any]:
    instances = list(wallet_instances.get("wallet_instances") or [])
    connected_instances = [
        instance for instance in instances if bool(instance.get("extension_connected", True))
    ]
    if wallet_instance_id:
        for instance in instances:
            if instance.get("wallet_instance_id") == wallet_instance_id:
                return instance
        raise RuntimeError(
            "wallet instance "
            + wallet_instance_id
            + " was not found. Available instances: "
            + format_wallet_candidates(instances)
        )
    if not connected_instances:
        raise RuntimeError("no connected wallet instances are available")
    if len(connected_instances) == 1:
        return connected_instances[0]
    raise RuntimeError(
        "wallet_instance_id is required because more than one connected wallet instance is available: "
        + format_wallet_candidates(connected_instances)
    )


def format_wallet_candidates(instances: list[dict[str, Any]]) -> str:
    if not instances:
        return "none"
    rendered = []
    for instance in instances:
        wallet_instance_id = str(instance.get("wallet_instance_id") or "<missing-id>")
        profile_hint = str(instance.get("profile_hint") or "unknown-profile")
        lock_state = str(instance.get("lock_state") or "unknown-lock-state")
        rendered.append(f"{wallet_instance_id} ({profile_hint}, {lock_state})")
    return ", ".join(rendered)


def account_count_for_instance(wallet_accounts: dict[str, Any], wallet_instance_id: str) -> int:
    for group in wallet_accounts.get("wallet_instances", []):
        if group.get("wallet_instance_id") == wallet_instance_id:
            return len(group.get("accounts") or [])
    return 0


def resolve_audit_log_path(
    audit_log_path_arg: str | None,
    *,
    wallet_runtime_dir_arg: str | None,
) -> Path:
    if audit_log_path_arg:
        return Path(audit_log_path_arg).expanduser().resolve()
    wallet_runtime_dir = resolve_wallet_runtime_dir(wallet_runtime_dir_arg).resolve()
    return wallet_runtime_dir / "audit" / "create-account-audit.jsonl"


def render_card(title: str, rows: list[tuple[str, str]]) -> str:
    width = 78
    label_width = max(len(label) for label, _ in rows) if rows else 0
    lines = []
    border = "+" + "-" * (width - 2) + "+"
    lines.append(border)
    lines.append(f"| {title.ljust(width - 4)} |")
    lines.append(border)
    for label, value in rows:
        prefix = f"{label}:".ljust(label_width + 2)
        text = f"{prefix} {value}"
        if len(text) <= width - 4:
            lines.append(f"| {text.ljust(width - 4)} |")
            continue
        wrapped = [text[i : i + (width - 4)] for i in range(0, len(text), width - 4)]
        for chunk in wrapped:
            lines.append(f"| {chunk.ljust(width - 4)} |")
    lines.append(border)
    return "\n".join(lines)


def wait_for_terminal_request(
    wallet_client: StarmaskDaemonClient,
    *,
    request_id: str,
    poll_interval_seconds: float = 1.0,
    on_status_change: Callable[[str], None] | None = None,
) -> dict[str, Any]:
    last_status = None
    while True:
        status = wallet_client.call_tool(
            "wallet_get_request_status",
            {"request_id": request_id},
        )
        current = str(status.get("status"))
        if current != last_status and on_status_change is not None:
            on_status_change(current)
        last_status = current
        if current in TERMINAL_REQUEST_STATUSES:
            return status
        time.sleep(poll_interval_seconds)


def main() -> int:
    args = parse_args()
    wallet_client = StarmaskDaemonClient(
        socket_path=resolve_socket_path(args.socket_path, args.wallet_runtime_dir)
    )
    wallet_instances = wallet_client.call_tool("wallet_list_instances")
    selected_instance = resolve_wallet_instance(wallet_instances, args.wallet_instance_id)
    wallet_instance_id = str(selected_instance["wallet_instance_id"])
    wallet_accounts = wallet_client.call_tool(
        "wallet_list_accounts",
        {
            "wallet_instance_id": wallet_instance_id,
            "include_public_key": False,
        },
    )
    accounts_before = account_count_for_instance(wallet_accounts, wallet_instance_id)
    audit_logger = WorkflowAuditLogger(
        resolve_audit_log_path(
            args.audit_log_path,
            wallet_runtime_dir_arg=args.wallet_runtime_dir,
        )
    )
    client_request_id = args.client_request_id or f"create-account-{int(time.time())}"
    request = wallet_client.call_tool(
        "wallet_create_account",
        {
            "client_request_id": client_request_id,
            "wallet_instance_id": wallet_instance_id,
            "display_hint": args.display_hint,
            "client_context": args.client_context,
            "ttl_seconds": args.ttl_seconds,
        },
    )
    audit_logger.record_create_account_request_created(
        wallet_instance_id=wallet_instance_id,
        request=request,
        client_context=args.client_context,
        display_hint=args.display_hint,
    )

    print(
        render_card(
            "Create Account Request",
            [
                ("Wallet Instance", wallet_instance_id),
                ("Lock State", str(selected_instance.get("lock_state"))),
                ("Profile Hint", str(selected_instance.get("profile_hint"))),
                ("Accounts Before", str(accounts_before)),
                ("Request ID", str(request.get("request_id"))),
                ("Status", str(request.get("status"))),
                ("TTL", str(args.ttl_seconds)),
            ],
        )
    )
    print("Use the wallet approval surface to approve or reject the account creation request.")
    print(f"Audit log: {audit_logger.path}")
    status = wait_for_terminal_request(
        wallet_client,
        request_id=str(request["request_id"]),
        poll_interval_seconds=args.poll_interval_seconds,
        on_status_change=lambda current: print(f"wallet_get_request_status -> {current}"),
    )
    audit_logger.record_create_account_request_terminal(
        wallet_instance_id=wallet_instance_id,
        request_id=str(request["request_id"]),
        status=status,
    )

    if status.get("status") != "approved":
        print()
        print(
            render_card(
                "Create Account Result",
                [
                    ("Request ID", str(request.get("request_id"))),
                    ("Status", str(status.get("status"))),
                    ("Error Code", str(status.get("error_code"))),
                    ("Error Message", str(status.get("error_message"))),
                ],
            )
        )
        return 1

    refreshed_accounts = wallet_client.call_tool(
        "wallet_list_accounts",
        {
            "wallet_instance_id": wallet_instance_id,
            "include_public_key": False,
        },
    )
    accounts_after = account_count_for_instance(refreshed_accounts, wallet_instance_id)
    result = status.get("result") or {}
    print()
    print(
        render_card(
            "Created Account",
            [
                ("Wallet Instance", wallet_instance_id),
                ("Address", str(result.get("address"))),
                ("Is Default", str(result.get("is_default"))),
                ("Is Locked", str(result.get("is_locked"))),
                ("Accounts After", str(accounts_after)),
            ],
        )
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        print("\nInterrupted.", file=sys.stderr)
        raise SystemExit(130)
