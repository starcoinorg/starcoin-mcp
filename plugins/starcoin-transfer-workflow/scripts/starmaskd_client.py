#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import platform
import socket
from pathlib import Path
import sys
from typing import Any


DAEMON_PROTOCOL_VERSION = 1
PLUGIN_ROOT = Path(__file__).resolve().parent.parent
WORKSPACE_ROOT = Path(
    os.environ.get(
        "STARCOIN_TRANSFER_WORKSPACE_ROOT",
        os.environ.get("STARCOIN_MCP_WORKSPACE_ROOT", str(PLUGIN_ROOT.parent.parent)),
    )
).resolve()
DEFAULT_WALLET_RUNTIME_DIR = Path.home() / ".runtime" / "wallet-runtime"


class StarmaskDaemonClient:
    def __init__(self, *, socket_path: Path, timeout_seconds: float = 5.0):
        self.socket_path = Path(socket_path)
        self.timeout_seconds = timeout_seconds
        self._next_id = 1

    def call_tool(self, name: str, arguments: dict[str, Any] | None = None) -> dict[str, Any]:
        params = arguments or {}
        if name == "wallet_status":
            return self._call(
                "wallet.status",
                {"protocol_version": DAEMON_PROTOCOL_VERSION},
            )
        if name == "wallet_list_instances":
            return self._call(
                "wallet.listInstances",
                {
                    "protocol_version": DAEMON_PROTOCOL_VERSION,
                    "connected_only": False,
                },
            )
        if name == "wallet_list_accounts":
            return self._call(
                "wallet.listAccounts",
                {
                    "protocol_version": DAEMON_PROTOCOL_VERSION,
                    "wallet_instance_id": params.get("wallet_instance_id"),
                    "include_public_key": bool(params.get("include_public_key", False)),
                },
            )
        if name == "wallet_get_public_key":
            return self._call(
                "wallet.getPublicKey",
                {
                    "protocol_version": DAEMON_PROTOCOL_VERSION,
                    "address": params["address"],
                    "wallet_instance_id": params.get("wallet_instance_id"),
                },
            )
        if name == "wallet_request_sign_transaction":
            return self._call(
                "request.createSignTransaction",
                {
                    "protocol_version": DAEMON_PROTOCOL_VERSION,
                    "client_request_id": params["client_request_id"],
                    "account_address": params["account_address"],
                    "wallet_instance_id": params.get("wallet_instance_id"),
                    "chain_id": params["chain_id"],
                    "raw_txn_bcs_hex": params["raw_txn_bcs_hex"],
                    "tx_kind": params["tx_kind"],
                    "display_hint": params.get("display_hint"),
                    "client_context": params.get("client_context"),
                    "ttl_seconds": params.get("ttl_seconds"),
                },
            )
        if name == "wallet_get_request_status":
            return self._call(
                "request.getStatus",
                {
                    "protocol_version": DAEMON_PROTOCOL_VERSION,
                    "request_id": params["request_id"],
                },
            )
        if name == "wallet_cancel_request":
            return self._call(
                "request.cancel",
                {
                    "protocol_version": DAEMON_PROTOCOL_VERSION,
                    "request_id": params["request_id"],
                },
            )
        raise RuntimeError(f"unsupported wallet tool: {name}")

    def _call(self, method: str, params: dict[str, Any]) -> dict[str, Any]:
        request_id = f"starmaskd-client-{self._next_id}"
        self._next_id += 1
        request = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        }
        raw_request = json.dumps(request, separators=(",", ":")).encode("utf-8")
        client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        client.settimeout(self.timeout_seconds)
        try:
            client.connect(str(self.socket_path))
            client.sendall(raw_request)
            client.shutdown(socket.SHUT_WR)
            chunks: list[bytes] = []
            while True:
                chunk = client.recv(65536)
                if not chunk:
                    break
                chunks.append(chunk)
        finally:
            client.close()
        if not chunks:
            raise RuntimeError(f"daemon {method} returned an empty response")
        response = json.loads(b"".join(chunks).decode("utf-8"))
        if "error" in response:
            error = response["error"]
            raise RuntimeError(
                f"daemon {method} failed: {error.get('code')} {error.get('message')}"
            )
        result = response.get("result")
        if not isinstance(result, dict):
            raise RuntimeError(f"daemon {method} returned a non-object result: {result!r}")
        return result


def platform_socket_path() -> Path:
    runtime_root = Path.home() / ".runtime"
    system = platform.system()
    if system == "Darwin":
        return runtime_root / "run" / "starmaskd.sock"
    if system == "Linux":
        config_home = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
        state_home = Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local" / "state"))
        runtime_dir = Path(os.environ.get("XDG_RUNTIME_DIR", state_home / "starcoin-mcp"))
        if runtime_dir.name != "starcoin-mcp":
            return runtime_dir / "starcoin-mcp" / "starmaskd.sock"
        return runtime_dir / "starmaskd.sock"
    return runtime_root / "run" / "starmaskd.sock"


def parse_json_if_exists(path: Path) -> dict[str, Any] | None:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (FileNotFoundError, json.JSONDecodeError):
        return None


def resolve_socket_path(socket_path_arg: str | None, runtime_dir_arg: str | None) -> Path:
    if socket_path_arg:
        return Path(socket_path_arg).expanduser()
    runtime_dir = Path(
        runtime_dir_arg
        or os.environ.get("STARMASK_WALLET_RUNTIME_DIR", str(DEFAULT_WALLET_RUNTIME_DIR))
    ).expanduser()
    metadata = parse_json_if_exists(runtime_dir / "wallet-runtime.json")
    if metadata is not None and metadata.get("daemon_socket_path"):
        return Path(str(metadata["daemon_socket_path"])).expanduser()
    env_socket_path = os.environ.get("STARMASKD_SOCKET_PATH") or os.environ.get(
        "STARMASK_MCP_DAEMON_SOCKET_PATH"
    )
    if env_socket_path:
        return Path(env_socket_path).expanduser()
    return platform_socket_path()


def read_json_arguments() -> dict[str, Any]:
    raw = sys.stdin.read()
    if not raw.strip():
        return {}
    value = json.loads(raw)
    if not isinstance(value, dict):
        raise RuntimeError("stdin arguments must be a JSON object")
    return value


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Call starmaskd directly over its local JSON-RPC socket."
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
    subparsers = parser.add_subparsers(dest="command", required=True)
    call = subparsers.add_parser("call", help="Call one wallet-side tool.")
    call.add_argument(
        "tool",
        help="Tool name, for example wallet_list_instances or wallet_request_sign_transaction.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    client = StarmaskDaemonClient(
        socket_path=resolve_socket_path(args.socket_path, args.wallet_runtime_dir)
    )
    if args.command != "call":
        raise SystemExit(f"unsupported command: {args.command}")
    result = client.call_tool(args.tool, read_json_arguments())
    json.dump(result, sys.stdout, separators=(",", ":"))
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
