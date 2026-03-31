#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import signal
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any
from urllib.error import URLError
from urllib.request import Request, urlopen


PLUGIN_ROOT = Path(__file__).resolve().parent.parent
WORKSPACE_ROOT = Path(
    os.environ.get("STARCOIN_MCP_WORKSPACE_ROOT", str(PLUGIN_ROOT.parent.parent))
).resolve()
STARMASKD_MANIFEST = (
    WORKSPACE_ROOT / "starmask-mcp" / "crates" / "starmaskd" / "Cargo.toml"
)
LOCAL_AGENT_MANIFEST = (
    WORKSPACE_ROOT
    / "starmask-mcp"
    / "crates"
    / "starmask-local-account-agent"
    / "Cargo.toml"
)
STARMASK_MCP_MANIFEST = (
    WORKSPACE_ROOT / "starmask-mcp" / "crates" / "starmask-mcp" / "Cargo.toml"
)
NODE_MCP_MANIFEST = (
    WORKSPACE_ROOT
    / "starcoin-node-mcp"
    / "crates"
    / "starcoin-node-mcp-server"
    / "Cargo.toml"
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run one local user-in-the-loop transfer test through the Starcoin MCP stack."
    )
    parser.add_argument("--rpc-url", required=True, help="Starcoin HTTP RPC endpoint")
    parser.add_argument(
        "--wallet-dir",
        help="Standalone local account vault directory for local-account-agent",
    )
    parser.add_argument(
        "--wallet-runtime-dir",
        help="Reuse an already-running wallet supervisor runtime from this directory.",
    )
    parser.add_argument(
        "--sender",
        required=True,
        help="Sender address from the standalone local wallet directory",
    )
    parser.add_argument(
        "--receiver",
        required=True,
        help="Receiver address for the transfer test",
    )
    parser.add_argument(
        "--amount",
        default="1000",
        help="Raw on-chain integer amount passed to prepare_transfer",
    )
    parser.add_argument(
        "--token-code",
        default="0x1::starcoin_coin::STC",
        help="Transfer token code passed to prepare_transfer",
    )
    parser.add_argument(
        "--wallet-instance-id",
        default="local-dev",
        help="Backend id used in the generated starmaskd config",
    )
    parser.add_argument(
        "--runtime-dir",
        default=None,
        help="Directory for generated configs, logs, and runtime socket/db files. Defaults to a unique per-run directory.",
    )
    parser.add_argument(
        "--ttl-seconds",
        type=int,
        default=300,
        help="Wallet signing request TTL",
    )
    parser.add_argument(
        "--watch-timeout-seconds",
        type=int,
        default=120,
        help="Blocking submit/watch timeout passed to starcoin-node-mcp",
    )
    return parser.parse_args()


def run_command(
    args: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    capture_output: bool = False,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=str(cwd) if cwd else None,
        env=env,
        text=True,
        check=True,
        capture_output=capture_output,
    )


def json_rpc(url: str, method: str, params: list[Any] | dict[str, Any] | None = None) -> Any:
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params if params is not None else [],
    }
    request = Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    with urlopen(request, timeout=5) as response:
        body = json.loads(response.read().decode("utf-8"))
    if "error" in body:
        raise RuntimeError(f"{method} failed: {body['error']}")
    return body["result"]


def wait_for_http_ready(url: str, timeout_seconds: int = 5) -> Any:
    deadline = time.time() + timeout_seconds
    last_error: str | None = None
    while time.time() < deadline:
        try:
            return json_rpc(url, "node.info")
        except Exception as exc:  # pragma: no cover - exercised in manual flow
            last_error = str(exc)
            time.sleep(0.2)
    raise RuntimeError(f"RPC endpoint {url} did not become ready: {last_error}")


def ensure_private_wallet_dir(wallet_dir: Path) -> None:
    if not wallet_dir.exists():
        raise FileNotFoundError(f"wallet_dir does not exist: {wallet_dir}")
    os.chmod(wallet_dir, 0o700)
    mode = wallet_dir.stat().st_mode & 0o777
    if mode & 0o077:
        raise RuntimeError(
            f"wallet_dir {wallet_dir} must not grant group/world permissions, got {oct(mode)}"
        )


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def choose_socket_path(runtime_dir: Path) -> Path:
    digest = hashlib.sha1(str(runtime_dir).encode("utf-8")).hexdigest()[:8]
    socket_dir = Path("/tmp") / "starcoin-mcp"
    socket_dir.mkdir(parents=True, exist_ok=True)
    return socket_dir / f"starmaskd-{digest}.sock"


def read_text_if_exists(path: Path) -> str:
    if not path.exists():
        return ""
    return path.read_text(encoding="utf-8")


def read_json_if_exists(path: Path) -> dict[str, Any] | None:
    if not path.exists():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def socket_reachable(path: Path) -> tuple[bool, str]:
    if not path.exists():
        return False, "socket file is missing"
    client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    client.settimeout(0.5)
    try:
        client.connect(str(path))
        return True, "unix socket accepted a connection"
    except OSError as exc:
        return False, str(exc)
    finally:
        client.close()


class JsonRpcLineClient:
    def __init__(
        self,
        name: str,
        args: list[str],
        *,
        cwd: Path,
        env: dict[str, str] | None = None,
        stderr_path: Path | None = None,
    ):
        self.name = name
        self._next_id = 1
        self.stderr_path = stderr_path
        self.stderr_handle = (
            stderr_path.open("w", encoding="utf-8") if stderr_path is not None else None
        )
        self.process = subprocess.Popen(
            args,
            cwd=str(cwd),
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self.stderr_handle if self.stderr_handle is not None else subprocess.DEVNULL,
            text=True,
            bufsize=1,
        )
        assert self.process.stdin is not None
        assert self.process.stdout is not None

    def _send(self, payload: dict[str, Any]) -> None:
        line = json.dumps(payload, separators=(",", ":"))
        self.process.stdin.write(line + "\n")
        self.process.stdin.flush()

    def _receive_until_response(self, request_id: int) -> dict[str, Any]:
        while True:
            line = self.process.stdout.readline()
            if not line:
                stderr_tail = (
                    read_text_if_exists(self.stderr_path).strip()
                    if self.stderr_path is not None
                    else ""
                )
                raise RuntimeError(
                    f"{self.name} exited before replying to request {request_id}. stderr: {stderr_tail}"
                )
            message = json.loads(line)
            if message.get("id") == request_id:
                return message

    def initialize(self) -> None:
        request_id = self._next_id
        self._next_id += 1
        self._send(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": {"name": "starcoin-transfer-test", "version": "0.1.0"},
                },
            }
        )
        response = self._receive_until_response(request_id)
        if "error" in response:
            raise RuntimeError(f"{self.name} initialize failed: {response['error']}")
        self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def call_tool(self, name: str, arguments: dict[str, Any] | None = None) -> dict[str, Any]:
        request_id = self._next_id
        self._next_id += 1
        payload = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments or {}},
        }
        self._send(payload)
        response = self._receive_until_response(request_id)
        if "error" in response:
            raise RuntimeError(f"{self.name} {name} failed: {response['error']}")
        result = response["result"]
        structured = result.get("structuredContent")
        if structured is not None:
            return structured
        content = result.get("content") or []
        if content:
            first = content[0]
            if first.get("type") == "text":
                return json.loads(first["text"])
        raise RuntimeError(f"{self.name} {name} returned no structured content")

    def terminate(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:  # pragma: no cover - manual flow only
                self.process.kill()
                self.process.wait(timeout=5)
        if self.stderr_handle is not None:
            self.stderr_handle.close()


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
        else:
            wrapped = [text[i : i + (width - 4)] for i in range(0, len(text), width - 4)]
            for chunk in wrapped:
                lines.append(f"| {chunk.ljust(width - 4)} |")
    lines.append(border)
    return "\n".join(lines)


def prompt_yes_no(prompt: str) -> bool:
    reply = input(f"{prompt} [y/N]: ").strip().lower()
    return reply in {"y", "yes"}


def wait_for_wallet_instance(
    wallet_client: JsonRpcLineClient, wallet_instance_id: str, timeout_seconds: int = 10
) -> dict[str, Any]:
    deadline = time.time() + timeout_seconds
    last_result: dict[str, Any] | None = None
    while time.time() < deadline:
        result = wallet_client.call_tool("wallet_list_instances")
        last_result = result
        for instance in result["wallet_instances"]:
            if instance["wallet_instance_id"] == wallet_instance_id:
                return result
        time.sleep(0.5)
    raise RuntimeError(
        f"wallet instance {wallet_instance_id} did not register in time: {last_result}"
    )


def main() -> int:
    args = parse_args()
    runtime_dir_explicit = args.runtime_dir is not None
    if runtime_dir_explicit:
        runtime_dir = Path(args.runtime_dir).resolve()
    else:
        runtime_base_dir = (WORKSPACE_ROOT / ".runtime").resolve()
        runtime_base_dir.mkdir(parents=True, exist_ok=True)
        runtime_dir = Path(
            tempfile.mkdtemp(prefix="transfer-test-", dir=str(runtime_base_dir))
        ).resolve()
    wallet_runtime_dir = (
        Path(args.wallet_runtime_dir).resolve() if args.wallet_runtime_dir else None
    )
    wallet_runtime = None
    wallet_dir: Path | None = None
    wallet_instance_id = args.wallet_instance_id

    if wallet_runtime_dir is not None:
        wallet_runtime = read_json_if_exists(wallet_runtime_dir / "wallet-runtime.json")
        if wallet_runtime is None:
            raise FileNotFoundError(
                f"wallet runtime metadata is missing: {wallet_runtime_dir / 'wallet-runtime.json'}"
            )
        wallet_instance_id = str(wallet_runtime.get("wallet_instance_id") or wallet_instance_id)
    else:
        if not args.wallet_dir:
            raise RuntimeError(
                "--wallet-dir is required unless --wallet-runtime-dir is provided"
            )
        wallet_dir = Path(args.wallet_dir).resolve()
        ensure_private_wallet_dir(wallet_dir)

    node_info = wait_for_http_ready(args.rpc_url)
    chain_info = node_info["peer_info"]["chain_info"]
    chain_id = int(chain_info["chain_id"])
    network = str(node_info["net"])
    genesis_hash = str(chain_info["genesis_hash"])

    run_dir = runtime_dir / "run"
    log_dir = runtime_dir / "logs"
    run_dir.mkdir(parents=True, exist_ok=True)
    log_dir.mkdir(parents=True, exist_ok=True)

    if wallet_runtime is not None:
        socket_raw = wallet_runtime.get("daemon_socket_path")
        if not socket_raw:
            raise RuntimeError("wallet runtime metadata does not include daemon_socket_path")
        socket_path = Path(socket_raw)
        metadata_chain_id = wallet_runtime.get("chain_id")
        if metadata_chain_id is not None and int(metadata_chain_id) != chain_id:
            raise RuntimeError(
                f"wallet runtime chain_id {metadata_chain_id} does not match rpc chain_id {chain_id}"
            )
    else:
        socket_path = choose_socket_path(runtime_dir)
    database_path = run_dir / "starmaskd.sqlite3"
    node_config_path = runtime_dir / "node-mcp.toml"
    wallet_config_path = runtime_dir / "starmaskd.toml"
    if wallet_runtime is None and runtime_dir_explicit and socket_path.exists():
        socket_path.unlink()

    node_config = f"""rpc_endpoint_url = "{args.rpc_url}"
mode = "transaction"
vm_profile = "auto"
expected_chain_id = {chain_id}
expected_network = "{network}"
expected_genesis_hash = "{genesis_hash}"
require_genesis_hash_match = true
connect_timeout_ms = 3000
request_timeout_ms = 10000
startup_probe_timeout_ms = 5000
default_expiration_ttl_seconds = 600
max_expiration_ttl_seconds = 3600
watch_poll_interval_seconds = 3
watch_timeout_seconds = {args.watch_timeout_seconds}
max_head_lag_seconds = 60
warn_head_lag_seconds = 15
allow_submit_without_prior_simulation = true
chain_status_cache_ttl_seconds = 3
abi_cache_ttl_seconds = 300
module_cache_max_entries = 1024
disable_disk_cache = true
max_submit_blocking_timeout_seconds = 60
max_watch_timeout_seconds = 300
min_watch_poll_interval_seconds = 2
max_list_blocks_count = 100
max_events_limit = 200
max_account_resource_limit = 100
max_account_module_limit = 50
max_list_resources_size = 100
max_list_modules_size = 100
max_publish_package_bytes = 524288
max_concurrent_watch_requests = 8
max_inflight_expensive_requests = 16
log_level = "info"
"""
    write_text(node_config_path, node_config)
    starmaskd: subprocess.Popen[str] | None = None
    agent: subprocess.Popen[str] | None = None
    starmaskd_log = None

    if wallet_runtime is None:
        assert wallet_dir is not None
        wallet_config = f"""channel = "development"
socket_path = "{socket_path}"
database_path = "{database_path}"
log_level = "info"
maintenance_interval_seconds = 1
default_request_ttl_seconds = 300
min_request_ttl_seconds = 30
max_request_ttl_seconds = 3600
delivery_lease_seconds = 30
presentation_lease_seconds = 45
heartbeat_interval_seconds = 10
wallet_offline_after_seconds = 25
result_retention_seconds = 600

[[wallet_backends]]
backend_id = "{wallet_instance_id}"
backend_kind = "local_account_dir"
enabled = true
instance_label = "Local Dev Wallet"
approval_surface = "tty_prompt"
prompt_mode = "tty_prompt"
account_dir = "{wallet_dir}"
chain_id = {chain_id}
unlock_cache_ttl_seconds = 300
allow_read_only_accounts = true
require_strict_permissions = true
"""
        write_text(wallet_config_path, wallet_config)

        starmaskd_log = (log_dir / "starmaskd.log").open("w", encoding="utf-8")
        starmaskd = subprocess.Popen(
            [
                "cargo",
                "run",
                "--quiet",
                "--manifest-path",
                str(STARMASKD_MANIFEST),
                "--bin",
                "starmaskd",
                "--",
                "serve",
                "--config",
                str(wallet_config_path),
            ],
            cwd=str(WORKSPACE_ROOT),
            stdin=subprocess.DEVNULL,
            stdout=starmaskd_log,
            stderr=subprocess.STDOUT,
            text=True,
        )

        agent = subprocess.Popen(
            [
                "cargo",
                "run",
                "--quiet",
                "--manifest-path",
                str(LOCAL_AGENT_MANIFEST),
                "--bin",
                "local-account-agent",
                "--",
                "--config",
                str(wallet_config_path),
                "--backend-id",
                wallet_instance_id,
            ],
            cwd=str(WORKSPACE_ROOT),
            text=True,
        )

    started_children: list[Any] = []
    node_client: JsonRpcLineClient | None = None
    wallet_client: JsonRpcLineClient | None = None

    try:
        deadline = time.time() + 10
        while time.time() < deadline:
            socket_ok, socket_detail = socket_reachable(socket_path)
            if socket_ok:
                break
            if wallet_runtime is None and starmaskd is not None and starmaskd.poll() is not None:
                log_output = read_text_if_exists(runtime_dir / "logs" / "starmaskd.log").strip()
                message = "starmaskd exited before creating the daemon socket"
                if log_output:
                    message = f"{message}\n\n{log_output}"
                raise RuntimeError(message)
            time.sleep(0.2)
        else:
            if wallet_runtime is None:
                log_output = read_text_if_exists(runtime_dir / "logs" / "starmaskd.log").strip()
                message = f"starmaskd socket did not become ready in time ({socket_detail})"
                if log_output:
                    message = f"{message}\n\n{log_output}"
                raise RuntimeError(message)
            raise RuntimeError(
                f"wallet runtime socket did not become ready in time: {socket_path} ({socket_detail})"
            )

        node_client = JsonRpcLineClient(
            "starcoin-node-mcp",
            [
                "cargo",
                "run",
                "--quiet",
                "--manifest-path",
                str(NODE_MCP_MANIFEST),
                "--bin",
                "starcoin-node-mcp",
                "--",
                "--config",
                str(node_config_path),
            ],
            cwd=WORKSPACE_ROOT,
            stderr_path=log_dir / "starcoin-node-mcp.stderr.log",
        )
        wallet_client = JsonRpcLineClient(
            "starmask-mcp",
            [
                "cargo",
                "run",
                "--quiet",
                "--manifest-path",
                str(STARMASK_MCP_MANIFEST),
                "--bin",
                "starmask-mcp",
                "--",
                "--daemon-socket-path",
                str(socket_path),
            ],
            cwd=WORKSPACE_ROOT,
            stderr_path=log_dir / "starmask-mcp.stderr.log",
        )
        started_children.extend([node_client, wallet_client])
        node_client.initialize()
        wallet_client.initialize()

        wallet_instances = wait_for_wallet_instance(wallet_client, wallet_instance_id)
        wallet_accounts = wallet_client.call_tool(
            "wallet_list_accounts",
            {"wallet_instance_id": wallet_instance_id, "include_public_key": True},
        )
        public_key_result = wallet_client.call_tool(
            "wallet_get_public_key",
            {
                "wallet_instance_id": wallet_instance_id,
                "address": args.sender,
            },
        )
        prepare_result = node_client.call_tool(
            "prepare_transfer",
            {
                "sender": args.sender,
                "sender_public_key": public_key_result["public_key"],
                "receiver": args.receiver,
                "amount": args.amount,
                "token_code": args.token_code,
            },
        )

        print(
            render_card(
                "Host Transfer Confirmation",
                [
                    ("Network", f"{network} ({chain_id})"),
                    ("Genesis", genesis_hash),
                    ("Wallet Instance", wallet_instance_id),
                    ("Known Wallets", str(len(wallet_instances["wallet_instances"]))),
                    (
                        "Visible Accounts",
                        str(
                            sum(
                                len(group["accounts"])
                                for group in wallet_accounts["wallet_instances"]
                            )
                        ),
                    ),
                    ("Sender", args.sender),
                    ("Receiver", args.receiver),
                    ("Amount", args.amount),
                    ("Token", args.token_code),
                    ("Simulation", prepare_result["simulation_status"]),
                    ("Prepared At", prepare_result["prepared_at"]),
                ],
            )
        )
        print()
        print("The next step will create a wallet signing request.")
        print("The local-account-agent will then show its own CLI approval card.")
        if not prompt_yes_no("Continue with wallet signing"):
            print("Transfer test cancelled before wallet_request_sign_transaction.")
            return 0

        request = wallet_client.call_tool(
            "wallet_request_sign_transaction",
            {
                "client_request_id": f"transfer-test-{int(time.time())}",
                "account_address": args.sender,
                "wallet_instance_id": wallet_instance_id,
                "chain_id": chain_id,
                "raw_txn_bcs_hex": prepare_result["raw_txn_bcs_hex"],
                "tx_kind": str(prepare_result["transaction_kind"]).lower(),
                "display_hint": f"Transfer {args.amount} to {args.receiver}",
                "client_context": "starcoin-transfer-test",
                "ttl_seconds": args.ttl_seconds,
            },
        )
        request_id = request["request_id"]
        print()
        print(
            render_card(
                "Wallet Signing Request",
                [
                    ("Request ID", request_id),
                    ("Status", request["status"]),
                    ("Account", args.sender),
                    ("Wallet Instance", request["wallet_instance_id"]),
                    ("TTL", str(args.ttl_seconds)),
                ],
            )
        )
        print("Use the local-account-agent prompt in this terminal to approve or reject.")

        last_status = None
        while True:
            status = wallet_client.call_tool(
                "wallet_get_request_status", {"request_id": request_id}
            )
            current = status["status"]
            if current != last_status:
                print(f"wallet_get_request_status -> {current}")
                last_status = current
            if current in {"approved", "rejected", "cancelled", "expired", "failed"}:
                break
            time.sleep(1)

        if status["status"] != "approved":
            print(
                render_card(
                    "Wallet Result",
                    [
                        ("Request ID", request_id),
                        ("Status", status["status"]),
                        ("Error Code", str(status.get("error_code"))),
                        ("Error Message", str(status.get("error_message"))),
                    ],
                )
            )
            return 1

        signed_txn = status["result"]["signed_txn_bcs_hex"]
        submit_result = node_client.call_tool(
            "submit_signed_transaction",
            {
                "signed_txn_bcs_hex": signed_txn,
                "prepared_chain_context": prepare_result["chain_context"],
                "blocking": True,
                "timeout_seconds": args.watch_timeout_seconds,
            },
        )
        rows = [
            ("Txn Hash", submit_result["txn_hash"]),
            ("Submission State", submit_result["submission_state"]),
            ("Submitted", str(submit_result["submitted"])),
            ("Next Action", submit_result["next_action"]),
        ]
        watch_result = submit_result.get("watch_result")
        if watch_result:
            rows.extend(
                [
                    ("Confirmed", str(watch_result["confirmed"])),
                    (
                        "VM Status",
                        str(watch_result["status_summary"].get("vm_status")),
                    ),
                    (
                        "Found",
                        str(watch_result["status_summary"].get("found")),
                    ),
                ]
            )
        print()
        print(render_card("Submit Result", rows))
        return 0
    finally:
        if node_client is not None:
            node_client.terminate()
        if wallet_client is not None:
            wallet_client.terminate()
        if agent is not None and agent.poll() is None:
            agent.send_signal(signal.SIGTERM)
            try:
                agent.wait(timeout=5)
            except subprocess.TimeoutExpired:  # pragma: no cover - manual flow only
                agent.kill()
        if starmaskd is not None and starmaskd.poll() is None:
            starmaskd.send_signal(signal.SIGTERM)
            try:
                starmaskd.wait(timeout=5)
            except subprocess.TimeoutExpired:  # pragma: no cover - manual flow only
                starmaskd.kill()
        if starmaskd_log is not None:
            starmaskd_log.close()


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        print("\nInterrupted.", file=sys.stderr)
        raise SystemExit(130)
