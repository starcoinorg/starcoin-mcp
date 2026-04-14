#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import signal
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any
from urllib.request import Request, urlopen

from node_cli_client import NodeCliClient
from runtime_layout import (
    STARMASKD_MANIFEST_MARKERS,
    resolve_workspace_root,
    wallet_runtime_socket_path,
)
from starmaskd_client import StarmaskDaemonClient
from transfer_host import TransferAuditLogger
from transfer_controller import (
    TransferController,
    describe_confirmation_depth,
    normalize_vm_profile,
    normalize_min_confirmed_blocks,
    resolve_token_code,
)


PLUGIN_ROOT = Path(__file__).resolve().parent.parent


WORKSPACE_ROOT = resolve_workspace_root(
    PLUGIN_ROOT, STARMASKD_MANIFEST_MARKERS
)
STARMASKD_MANIFEST = (
    WORKSPACE_ROOT / "starmask-runtime" / "crates" / "starmaskd" / "Cargo.toml"
)
LOCAL_AGENT_MANIFEST = (
    WORKSPACE_ROOT
    / "starmask-runtime"
    / "crates"
    / "starmask-local-account-agent"
    / "Cargo.toml"
)


def resolve_binary(env_name: str, binary_name: str) -> str | None:
    override = os.environ.get(env_name)
    if override:
        return override
    return shutil.which(binary_name)


def launch_command(
    *,
    env_name: str,
    binary_name: str,
    manifest_path: Path,
    cargo_bin_name: str,
    program_args: list[str],
) -> list[str]:
    binary_path = resolve_binary(env_name, binary_name)
    if binary_path is not None:
        return [binary_path, *program_args]
    return [
        "cargo",
        "run",
        "--quiet",
        "--manifest-path",
        str(manifest_path),
        "--bin",
        cargo_bin_name,
        "--",
        *program_args,
    ]


def parse_args() -> argparse.Namespace:
    def parse_vm_profile_arg(value: str) -> str:
        try:
            return normalize_vm_profile(value)
        except ValueError as exc:
            raise argparse.ArgumentTypeError(str(exc)) from exc

    parser = argparse.ArgumentParser(
        description="Run one local user-in-the-loop transfer test through the script and CLI transfer stack."
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
        help="Transfer amount. Use raw on-chain units by default, or pair with --amount-unit stc for human-readable STC.",
    )
    parser.add_argument(
        "--amount-unit",
        choices=("raw", "stc"),
        default="raw",
        help="Interpret --amount as raw on-chain units or human-readable STC.",
    )
    parser.add_argument(
        "--token-code",
        default=None,
        help="Transfer token code passed to prepare_transfer. Defaults to a vm_profile-matched STC token code.",
    )
    parser.add_argument(
        "--vm-profile",
        type=parse_vm_profile_arg,
        default="auto",
        help="RPC routing profile for the generated node-cli.toml. Use vm1_only or vm2_only for fixed transfer semantics.",
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
        help="Blocking submit/watch timeout passed to starcoin-node-cli",
    )
    parser.add_argument(
        "--min-confirmed-blocks",
        type=int,
        default=2,
        help="Minimum confirmed block count required before success. 2 means the inclusion block plus 1 additional block.",
    )
    parser.add_argument(
        "--audit-log-path",
        default=None,
        help="Optional JSONL path for local transfer audit records. Defaults under the active runtime directory.",
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


def read_text_if_exists(path: Path) -> str:
    if not path.exists():
        return ""
    return path.read_text(encoding="utf-8")


def read_json_if_exists(path: Path) -> dict[str, Any] | None:
    if not path.exists():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def resolve_audit_log_path(
    audit_log_path_arg: str | None,
    *,
    runtime_dir: Path,
    wallet_runtime_dir: Path | None,
) -> Path:
    if audit_log_path_arg:
        return Path(audit_log_path_arg).expanduser().resolve()
    base_dir = wallet_runtime_dir if wallet_runtime_dir is not None else runtime_dir
    return base_dir / "audit" / "transfer-audit.jsonl"


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
    wallet_client: Any, wallet_instance_id: str, timeout_seconds: int = 10
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
    token_code = resolve_token_code(args.token_code, args.vm_profile)
    min_confirmed_blocks = normalize_min_confirmed_blocks(args.min_confirmed_blocks)
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
    audit_log_path = resolve_audit_log_path(
        args.audit_log_path,
        runtime_dir=runtime_dir,
        wallet_runtime_dir=wallet_runtime_dir,
    )
    audit_logger = TransferAuditLogger(audit_log_path)
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
        socket_path = wallet_runtime_socket_path(runtime_dir)
    database_path = run_dir / "starmaskd.sqlite3"
    node_config_path = runtime_dir / "node-cli.toml"
    wallet_config_path = runtime_dir / "starmaskd.toml"
    if wallet_runtime is None and runtime_dir_explicit and socket_path.exists():
        socket_path.unlink()

    node_config = f"""rpc_endpoint_url = "{args.rpc_url}"
mode = "transaction"
vm_profile = "{args.vm_profile}"
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
            launch_command(
                env_name="STARMASKD_BIN",
                binary_name="starmaskd",
                manifest_path=STARMASKD_MANIFEST,
                cargo_bin_name="starmaskd",
                program_args=["serve", "--config", str(wallet_config_path)],
            ),
            cwd=str(WORKSPACE_ROOT),
            stdin=subprocess.DEVNULL,
            stdout=starmaskd_log,
            stderr=subprocess.STDOUT,
            text=True,
        )

        agent = subprocess.Popen(
            launch_command(
                env_name="LOCAL_ACCOUNT_AGENT_BIN",
                binary_name="local-account-agent",
                manifest_path=LOCAL_AGENT_MANIFEST,
                cargo_bin_name="local-account-agent",
                program_args=[
                    "--config",
                    str(wallet_config_path),
                    "--backend-id",
                    wallet_instance_id,
                ],
            ),
            cwd=str(WORKSPACE_ROOT),
            text=True,
        )

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

        node_client = NodeCliClient(
            config_path=node_config_path,
            workspace_root=WORKSPACE_ROOT,
        )
        wallet_client = StarmaskDaemonClient(
            socket_path=socket_path,
        )
        wait_for_wallet_instance(wallet_client, wallet_instance_id)
        controller = TransferController(
            node_client=node_client,
            wallet_client=wallet_client,
            chain_id=chain_id,
            network=network,
            genesis_hash=genesis_hash,
        )
        try:
            session = controller.prepare_session(
                wallet_instance_id=wallet_instance_id,
                vm_profile=args.vm_profile,
                sender=args.sender,
                receiver=args.receiver,
                amount=args.amount,
                amount_unit=args.amount_unit,
                token_code=token_code,
            )
        except ValueError as error:
            raise SystemExit(f"invalid transfer amount: {error}") from error
        preflight_report = controller.collect_preflight_report(session)
        audit_logger.record_preflight(session, preflight_report)
        print(
            render_card(
                "Transfer Preflight Preview",
                controller.preflight_rows(
                    session,
                    preflight_report,
                    min_confirmed_blocks=min_confirmed_blocks,
                ),
            )
        )
        risk_rows = controller.risk_rows(preflight_report)
        if risk_rows:
            print()
            print(render_card("Risk Labels", risk_rows))
        print()
        print(f"Audit records will be written to {audit_logger.path}.")
        if controller.has_blocking_risks(preflight_report):
            audit_logger.record_host_decision(
                session,
                decision="blocked",
                reason="blocking_preflight_risk",
                report=preflight_report,
            )
            print("Blocking risks were detected. Fix them before requesting a wallet signature.")
            print(f"Audit log: {audit_logger.path}")
            return 1
        print("The next step will create a wallet signing request.")
        print(
            "A successful result will require "
            + describe_confirmation_depth(min_confirmed_blocks)
            + "."
        )
        print("The local-account-agent will then show its own CLI approval card.")
        if not prompt_yes_no("Continue with wallet signing"):
            audit_logger.record_host_decision(
                session,
                decision="cancelled",
                reason="user_declined_after_preview",
                report=preflight_report,
            )
            print("Transfer test cancelled before wallet_request_sign_transaction.")
            print(f"Audit log: {audit_logger.path}")
            return 0

        request = controller.create_sign_request(
            session,
            client_request_id=f"transfer-test-{int(time.time())}",
            ttl_seconds=args.ttl_seconds,
            client_context="starcoin-transfer-test",
        )
        audit_logger.record_sign_request_created(session, request)
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
        status = controller.wait_for_terminal_request(
            session,
            on_status_change=lambda current: print(f"wallet_get_request_status -> {current}"),
        )
        audit_logger.record_sign_request_terminal(session, status)

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
            print(f"Audit log: {audit_logger.path}")
            return 1

        submit_outcome = controller.submit(
            session,
            timeout_seconds=args.watch_timeout_seconds,
            min_confirmed_blocks=min_confirmed_blocks,
            blocking=True,
        )
        audit_logger.record_submission(session, submit_outcome)
        print()
        print(render_card("Submit Result", controller.submit_rows(submit_outcome)))
        print(f"Audit log: {audit_logger.path}")
        return 0 if submit_outcome.success else 1
    finally:
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
