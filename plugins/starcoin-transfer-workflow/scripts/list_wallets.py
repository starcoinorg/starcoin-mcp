#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import sys
import unicodedata
from typing import Any

from starmaskd_client import StarmaskDaemonClient, resolve_socket_path


LOCALIZED_STRINGS = {
    "en": {
        "wallet_instance": "Wallet Instance",
        "label": "Label",
        "address": "Address",
        "default": "Default",
        "status": "Status",
        "public_key": "Public Key",
        "yes": "yes",
        "no": "no",
        "unlabeled": "<unlabeled>",
        "no_accounts": "No accounts found in the selected wallet instances.",
    },
    "zh": {
        "wallet_instance": "钱包实例",
        "label": "标签",
        "address": "地址",
        "default": "默认",
        "status": "状态",
        "public_key": "公钥",
        "yes": "是",
        "no": "否",
        "unlabeled": "<未命名>",
        "no_accounts": "所选钱包实例下没有地址。",
    },
}


def default_locale() -> str:
    language = os.environ.get("LANG", "").lower()
    if language.startswith("zh"):
        return "zh"
    return "en"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="List wallet accounts with an aligned table view."
    )
    parser.add_argument(
        "--socket-path",
        default=None,
        help="Explicit daemon socket path. Overrides wallet-runtime metadata discovery.",
    )
    parser.add_argument(
        "--wallet-runtime-dir",
        default=None,
        help="Optional wallet runtime directory used to discover wallet-runtime.json.",
    )
    parser.add_argument(
        "--wallet-instance-id",
        default=None,
        help="Optional wallet instance id. Lists all visible wallet instances when omitted.",
    )
    parser.add_argument(
        "--include-public-key",
        action="store_true",
        help="Include the public-key column in the rendered output.",
    )
    parser.add_argument(
        "--locale",
        choices=sorted(LOCALIZED_STRINGS.keys()),
        default=default_locale(),
        help="Header/boolean localization for human-readable output.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Emit machine-readable JSON instead of a table.",
    )
    return parser.parse_args()


def display_width(value: str) -> int:
    width = 0
    for char in value:
        if unicodedata.combining(char):
            continue
        width += 2 if unicodedata.east_asian_width(char) in {"F", "W"} else 1
    return width


def pad_cell(value: str, width: int) -> str:
    padding = max(0, width - display_width(value))
    return value + (" " * padding)


def render_table(headers: list[str], rows: list[list[str]]) -> str:
    widths = [display_width(header) for header in headers]
    for row in rows:
        for index, cell in enumerate(row):
            widths[index] = max(widths[index], display_width(cell))

    border = "+-" + "-+-".join("-" * width for width in widths) + "-+"
    lines = [border]
    lines.append(
        "| " + " | ".join(pad_cell(header, widths[index]) for index, header in enumerate(headers)) + " |"
    )
    lines.append(border)
    for row in rows:
        lines.append(
            "| " + " | ".join(pad_cell(cell, widths[index]) for index, cell in enumerate(row)) + " |"
        )
    lines.append(border)
    return "\n".join(lines)


def account_status(account: dict[str, Any]) -> str:
    if bool(account.get("is_read_only")):
        return "read-only"
    if bool(account.get("is_locked")):
        return "locked"
    return "unlocked"


def flatten_account_rows(
    *,
    wallet_instances: list[dict[str, Any]],
    account_groups: dict[str, list[dict[str, Any]]],
    locale: str,
    include_public_key: bool,
) -> list[dict[str, Any]]:
    strings = LOCALIZED_STRINGS[locale]
    flattened: list[dict[str, Any]] = []
    for instance in wallet_instances:
        wallet_instance_id = str(instance.get("wallet_instance_id") or "")
        for account in account_groups.get(wallet_instance_id, []):
            row = {
                "wallet_instance_id": wallet_instance_id,
                "label": str(account.get("label") or strings["unlabeled"]),
                "address": str(account.get("address") or ""),
                "is_default": bool(account.get("is_default")),
                "status": account_status(account),
            }
            if include_public_key:
                row["public_key"] = str(account.get("public_key") or "")
            flattened.append(row)
    return flattened


def read_accounts_for_instance(
    client: StarmaskDaemonClient,
    *,
    wallet_instance_id: str,
    include_public_key: bool,
) -> list[dict[str, Any]]:
    payload = client.call_tool(
        "wallet_list_accounts",
        {
            "wallet_instance_id": wallet_instance_id,
            "include_public_key": include_public_key,
        },
    )
    for group in payload.get("wallet_instances") or []:
        if group.get("wallet_instance_id") != wallet_instance_id:
            continue
        return [
            account for account in group.get("accounts") or [] if isinstance(account, dict)
        ]
    return []


def load_wallet_listing(
    client: StarmaskDaemonClient,
    *,
    wallet_instance_id: str | None,
    locale: str,
    include_public_key: bool,
) -> dict[str, Any]:
    instances_payload = client.call_tool("wallet_list_instances")
    wallet_instances = [
        instance
        for instance in instances_payload.get("wallet_instances") or []
        if isinstance(instance, dict)
    ]
    if wallet_instance_id is not None:
        wallet_instances = [
            instance
            for instance in wallet_instances
            if instance.get("wallet_instance_id") == wallet_instance_id
        ]
        if not wallet_instances:
            raise RuntimeError(
                f"wallet instance {wallet_instance_id} was not found"
            )

    account_groups: dict[str, list[dict[str, Any]]] = {}
    for instance in wallet_instances:
        instance_id = str(instance.get("wallet_instance_id") or "")
        account_groups[instance_id] = read_accounts_for_instance(
            client,
            wallet_instance_id=instance_id,
            include_public_key=include_public_key,
        )

    rows = flatten_account_rows(
        wallet_instances=wallet_instances,
        account_groups=account_groups,
        locale=locale,
        include_public_key=include_public_key,
    )
    return {
        "wallet_instances": wallet_instances,
        "rows": rows,
    }


def format_wallet_listing(listing: dict[str, Any], *, locale: str, include_public_key: bool) -> str:
    strings = LOCALIZED_STRINGS[locale]
    rows = list(listing.get("rows") or [])
    if not rows:
        return strings["no_accounts"]

    headers = [
        strings["wallet_instance"],
        strings["label"],
        strings["address"],
        strings["default"],
        strings["status"],
    ]
    if include_public_key:
        headers.append(strings["public_key"])

    rendered_rows: list[list[str]] = []
    for row in rows:
        rendered = [
            str(row.get("wallet_instance_id") or ""),
            str(row.get("label") or strings["unlabeled"]),
            str(row.get("address") or ""),
            strings["yes"] if bool(row.get("is_default")) else strings["no"],
            str(row.get("status") or ""),
        ]
        if include_public_key:
            rendered.append(str(row.get("public_key") or ""))
        rendered_rows.append(rendered)
    return render_table(headers, rendered_rows)


def main() -> int:
    args = parse_args()
    client = StarmaskDaemonClient(
        socket_path=resolve_socket_path(args.socket_path, args.wallet_runtime_dir)
    )
    listing = load_wallet_listing(
        client,
        wallet_instance_id=args.wallet_instance_id,
        locale=args.locale,
        include_public_key=args.include_public_key,
    )
    if args.json:
        json.dump(listing, sys.stdout, indent=2, sort_keys=True)
        sys.stdout.write("\n")
    else:
        print(
            format_wallet_listing(
                listing,
                locale=args.locale,
                include_public_key=args.include_public_key,
            )
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
