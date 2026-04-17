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
import time
from pathlib import Path
from typing import Any

from runtime_layout import (
    DEFAULT_LOCAL_ACCOUNT_DIR,
    DEFAULT_WALLET_BACKEND_ID,
    DEFAULT_WALLET_INSTANCE_LABEL,
    DEFAULT_WALLET_RUNTIME_DIR,
    RUNTIME_METADATA_NAME,
    STARMASKD_MANIFEST_MARKERS,
    resolve_workspace_root,
    wallet_runtime_socket_path,
)
from starmaskd_client import StarmaskDaemonClient

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
) -> tuple[list[str], str]:
    binary_path = resolve_binary(env_name, binary_name)
    if binary_path is not None:
        return [binary_path, *program_args], binary_path
    return (
        [
            "cargo",
            "run",
            "--quiet",
            "--manifest-path",
            str(manifest_path),
            "--bin",
            cargo_bin_name,
            "--",
            *program_args,
        ],
        f"cargo:{manifest_path}",
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run a foreground supervisor for the wallet-side local runtime."
    )
    parser.add_argument(
        "--runtime-dir",
        default=str(DEFAULT_WALLET_RUNTIME_DIR),
        help="Directory for generated config, pid files, and logs.",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    up = subparsers.add_parser("up", help="Start starmaskd and local-account-agent.")
    up.add_argument(
        "--wallet-dir",
        default=str(DEFAULT_LOCAL_ACCOUNT_DIR),
        help="Standalone local account vault directory used by local-account-agent.",
    )
    up.add_argument(
        "--chain-id",
        type=int,
        default=254,
        help="Chain id exposed to the wallet backend.",
    )
    up.add_argument(
        "--backend-id",
        default=DEFAULT_WALLET_BACKEND_ID,
        help="Backend id registered with starmaskd.",
    )
    up.add_argument(
        "--instance-label",
        default=DEFAULT_WALLET_INSTANCE_LABEL,
        help="Display label reported by the wallet backend.",
    )
    up.add_argument(
        "--replace",
        action="store_true",
        help="Stop any runtime already using this runtime-dir before starting.",
    )

    status = subparsers.add_parser("status", help="Show runtime status.")
    status.add_argument("--json", action="store_true", help="Emit machine-readable JSON.")

    down = subparsers.add_parser("down", help="Stop the supervised runtime.")
    down.add_argument("--json", action="store_true", help="Emit machine-readable JSON.")

    export_account = subparsers.add_parser(
        "export-account",
        help="Export one local account private key into a file.",
    )
    export_account.add_argument(
        "--address",
        required=True,
        help="Account address to export.",
    )
    export_account.add_argument(
        "--output-file",
        dest="output_file",
        default=None,
        help="Destination file, or an existing directory where a timestamped private-key file will be written. Prompts when omitted in an interactive shell.",
    )
    export_account.add_argument(
        "--wallet-instance-id",
        default=None,
        help="Wallet instance that owns the account. Defaults to the active runtime metadata.",
    )
    export_account.add_argument(
        "--ttl-seconds",
        type=int,
        default=300,
        help="Request TTL while waiting for local approval.",
    )
    export_account.add_argument(
        "--wait-timeout-seconds",
        type=float,
        default=300.0,
        help="How long to wait for the local approval result.",
    )
    export_account.add_argument(
        "--force",
        action="store_true",
        help="Overwrite an existing output file.",
    )
    export_account.add_argument("--json", action="store_true", help="Emit machine-readable JSON.")

    import_account = subparsers.add_parser(
        "import-account",
        help="Import one local account private key from a file through the running runtime.",
    )
    import_account.add_argument(
        "--private-key-file",
        required=True,
        help="Private-key file to import. The local account agent reads this file locally.",
    )
    import_account.add_argument(
        "--address",
        default=None,
        help="Optional account address. When omitted, the local agent derives it from the private key.",
    )
    import_account.add_argument(
        "--wallet-instance-id",
        default=None,
        help="Wallet instance that should import the account. Defaults to the active runtime metadata.",
    )
    import_account.add_argument(
        "--ttl-seconds",
        type=int,
        default=300,
        help="Request TTL while waiting for local approval.",
    )
    import_account.add_argument(
        "--wait-timeout-seconds",
        type=float,
        default=300.0,
        help="How long to wait for the local approval result.",
    )
    import_account.add_argument("--json", action="store_true", help="Emit machine-readable JSON.")

    return parser.parse_args()


def ensure_private_wallet_dir(wallet_dir: Path) -> None:
    if not wallet_dir.exists():
        wallet_dir.mkdir(parents=True, mode=0o700)
    os.chmod(wallet_dir, 0o700)
    mode = wallet_dir.stat().st_mode & 0o777
    if mode & 0o077:
        raise RuntimeError(
            f"wallet_dir {wallet_dir} must not grant group/world permissions, got {oct(mode)}"
        )


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def write_json(path: Path, payload: dict[str, Any]) -> None:
    write_text(path, json.dumps(payload, indent=2, sort_keys=True) + "\n")


def read_json(path: Path) -> dict[str, Any] | None:
    if not path.exists():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def pid_is_running(pid: int | None) -> bool:
    if not pid:
        return False
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def read_pid(path: Path) -> int | None:
    if not path.exists():
        return None
    try:
        return int(path.read_text(encoding="utf-8").strip())
    except ValueError:
        return None


def write_pid(path: Path, pid: int) -> None:
    write_text(path, f"{pid}\n")


def remove_if_exists(path: Path) -> None:
    try:
        path.unlink()
    except FileNotFoundError:
        pass


def append_log_note(path: Path, message: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(f"[wallet_runtime] {message}\n")


def prompt_for_path(prompt: str) -> Path:
    if not sys.stdin.isatty():
        raise RuntimeError("output file must be provided with --output-file in non-interactive mode")
    value = input(f"{prompt}: ").strip()
    if not value:
        raise RuntimeError("output file cannot be empty")
    return Path(value).expanduser().resolve()


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


def runtime_paths(runtime_dir: Path) -> dict[str, Path]:
    run_dir = runtime_dir / "run"
    logs_dir = runtime_dir / "logs"
    return {
        "run_dir": run_dir,
        "logs_dir": logs_dir,
        "config_path": runtime_dir / "starmaskd.toml",
        "database_path": run_dir / "starmaskd.sqlite3",
        "metadata_path": runtime_dir / RUNTIME_METADATA_NAME,
        "starmaskd_pid_path": run_dir / "starmaskd.pid",
        "agent_pid_path": run_dir / "local-account-agent.pid",
        "starmaskd_log_path": logs_dir / "starmaskd.log",
        "agent_log_path": logs_dir / "local-account-agent.log",
    }


def account_export_filename(account_address: str) -> str:
    address = account_address.strip()
    if address.startswith(("0x", "0X")):
        address = address[2:]
    safe_address = "".join(char.lower() if char.isalnum() else "-" for char in address)
    if not safe_address:
        safe_address = "account"
    timestamp = time.strftime("%Y%m%d-%H%M%S")
    return f"{safe_address}-private-key-export-{timestamp}.txt"


def resolve_account_export_output_file(
    output_file_arg: str | None,
    *,
    account_address: str,
) -> Path:
    requested = (
        Path(output_file_arg).expanduser().resolve()
        if output_file_arg is not None
        else prompt_for_path("Account export output file or directory")
    )
    if requested.exists():
        if requested.is_dir():
            return requested / account_export_filename(account_address)
    return requested


def toml_string(value: str) -> str:
    return json.dumps(value)


def build_wallet_config(
    *,
    socket_path: Path,
    database_path: Path,
    wallet_dir: Path,
    backend_id: str,
    instance_label: str,
    chain_id: int,
) -> str:
    return f"""channel = {toml_string("development")}
socket_path = {toml_string(str(socket_path))}
database_path = {toml_string(str(database_path))}
log_level = {toml_string("info")}
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
backend_id = {toml_string(backend_id)}
backend_kind = {toml_string("local_account_dir")}
enabled = true
instance_label = {toml_string(instance_label)}
approval_surface = {toml_string("tty_prompt")}
prompt_mode = {toml_string("tty_prompt")}
account_dir = {toml_string(str(wallet_dir))}
chain_id = {chain_id}
unlock_cache_ttl_seconds = 300
allow_read_only_accounts = true
require_strict_permissions = true
"""


def read_text_if_exists(path: Path) -> str:
    if not path.exists():
        return ""
    return path.read_text(encoding="utf-8")


def wait_for_socket(socket_path: Path, starmaskd: subprocess.Popen[str], log_path: Path) -> None:
    deadline = time.time() + 10
    last_detail = "socket not ready"
    while time.time() < deadline:
        ready, detail = socket_reachable(socket_path)
        if ready:
            return
        if starmaskd.poll() is not None:
            log_output = read_text_if_exists(log_path).strip()
            message = "starmaskd exited before creating the daemon socket"
            if log_output:
                message = f"{message}\n\n{log_output}"
            raise RuntimeError(message)
        if detail != last_detail:
            append_log_note(log_path, f"waiting for daemon socket: {detail}")
            last_detail = detail
        time.sleep(0.2)
    log_output = read_text_if_exists(log_path).strip()
    message = f"starmaskd socket did not become ready in time ({last_detail})"
    if log_output:
        message = f"{message}\n\n{log_output}"
    raise RuntimeError(message)


def load_status(runtime_dir: Path) -> dict[str, Any]:
    paths = runtime_paths(runtime_dir)
    metadata = read_json(paths["metadata_path"]) or {}
    socket_path = Path(
        metadata.get("daemon_socket_path", str(wallet_runtime_socket_path(runtime_dir)))
    )
    starmaskd_pid = read_pid(paths["starmaskd_pid_path"])
    agent_pid = read_pid(paths["agent_pid_path"])
    daemon_socket_ok, daemon_socket_detail = socket_reachable(socket_path)
    return {
        "runtime_dir": str(runtime_dir),
        "metadata_path": str(paths["metadata_path"]),
        "config_path": str(paths["config_path"]),
        "starmaskd_log_path": str(paths["starmaskd_log_path"]),
        "agent_log_path": metadata.get("agent_log_path"),
        "wallet_dir": metadata.get("wallet_dir"),
        "wallet_instance_id": metadata.get("wallet_instance_id"),
        "chain_id": metadata.get("chain_id"),
        "daemon_socket_path": str(socket_path),
        "starmaskd_pid": starmaskd_pid,
        "agent_pid": agent_pid,
        "starmaskd_running": pid_is_running(starmaskd_pid),
        "agent_running": pid_is_running(agent_pid),
        "daemon_socket_ok": daemon_socket_ok,
        "daemon_socket_detail": daemon_socket_detail,
        "metadata_exists": paths["metadata_path"].exists(),
    }


def print_status(status: dict[str, Any]) -> None:
    print(f"runtime_dir:        {status['runtime_dir']}")
    print(f"wallet_dir:         {status.get('wallet_dir')}")
    print(f"wallet_instance_id: {status.get('wallet_instance_id')}")
    print(f"chain_id:           {status.get('chain_id')}")
    print(f"daemon_socket_path: {status['daemon_socket_path']}")
    print(
        f"starmaskd:          pid={status['starmaskd_pid']} running={status['starmaskd_running']}"
    )
    print(f"local-agent:        pid={status['agent_pid']} running={status['agent_running']}")
    print(
        f"daemon_socket:      ok={status['daemon_socket_ok']} detail={status['daemon_socket_detail']}"
    )
    print(f"config_path:        {status['config_path']}")
    print(f"metadata_path:      {status['metadata_path']}")
    print(f"starmaskd_log_path: {status['starmaskd_log_path']}")
    print(f"agent_log_path:     {status['agent_log_path'] or 'attached to supervisor terminal'}")


def terminate_pid(pid: int | None, *, timeout_seconds: float = 5.0) -> bool:
    if not pid or not pid_is_running(pid):
        return False
    os.kill(pid, signal.SIGTERM)
    deadline = time.time() + timeout_seconds
    while time.time() < deadline:
        if not pid_is_running(pid):
            return True
        time.sleep(0.2)
    os.kill(pid, signal.SIGKILL)
    deadline = time.time() + 2
    while time.time() < deadline:
        if not pid_is_running(pid):
            return True
        time.sleep(0.2)
    return not pid_is_running(pid)


def stop_runtime(runtime_dir: Path) -> dict[str, Any]:
    status = load_status(runtime_dir)
    terminated_agent = terminate_pid(status["agent_pid"])
    terminated_starmaskd = terminate_pid(status["starmaskd_pid"])
    paths = runtime_paths(runtime_dir)
    remove_if_exists(Path(status["daemon_socket_path"]))
    remove_if_exists(paths["starmaskd_pid_path"])
    remove_if_exists(paths["agent_pid_path"])
    remove_if_exists(paths["metadata_path"])
    return {
        **status,
        "terminated_agent": terminated_agent,
        "terminated_starmaskd": terminated_starmaskd,
    }


TERMINAL_REQUEST_STATUSES = {"approved", "rejected", "expired", "cancelled", "failed"}


def new_client_request_id(prefix: str) -> str:
    return f"{prefix}-{int(time.time() * 1000)}-{os.getpid()}"


def require_wallet_runtime_ready(runtime_dir: Path) -> dict[str, Any]:
    status = load_status(runtime_dir)
    if not status["daemon_socket_ok"]:
        raise RuntimeError(
            "wallet runtime must be running for account import/export; start it with "
            "`wallet_runtime.py up` and keep the supervisor terminal open "
            f"(daemon socket: {status['daemon_socket_detail']})"
        )
    if not status["agent_running"]:
        raise RuntimeError(
            "local-account-agent is not running; start `wallet_runtime.py up` and approve "
            "account import/export in that supervisor terminal"
        )
    return status


def wait_for_wallet_request(
    client: StarmaskDaemonClient,
    *,
    request_id: str,
    timeout_seconds: float,
    poll_interval_seconds: float = 0.5,
) -> dict[str, Any]:
    deadline = time.time() + timeout_seconds
    last_status: dict[str, Any] | None = None
    while True:
        current = client.call_tool(
            "wallet_get_request_status",
            {"request_id": request_id},
        )
        last_status = current
        status = str(current.get("status", "")).lower()
        if status in TERMINAL_REQUEST_STATUSES:
            if status != "approved":
                error_message = current.get("error_message") or "no error detail"
                raise RuntimeError(
                    f"wallet request {request_id} ended with status {status}: {error_message}"
                )
            result = current.get("result")
            if not isinstance(result, dict):
                raise RuntimeError(f"wallet request {request_id} approved without a result")
            return current
        if time.time() >= deadline:
            raise RuntimeError(
                f"wallet request {request_id} did not finish within {timeout_seconds:g}s; "
                f"last status was {last_status}"
            )
        time.sleep(poll_interval_seconds)


def request_account_export(
    client: StarmaskDaemonClient,
    *,
    account_address: str,
    output_file: Path,
    wallet_instance_id: str | None,
    force: bool,
    ttl_seconds: int,
    wait_timeout_seconds: float,
) -> dict[str, Any]:
    created = client.call_tool(
        "wallet_request_export_account",
        {
            "client_request_id": new_client_request_id("export-account"),
            "account_address": account_address,
            "wallet_instance_id": wallet_instance_id,
            "output_file": str(output_file),
            "force": force,
            "display_hint": f"Export account {account_address}",
            "client_context": "starcoin-transfer wallet_runtime export-account",
            "ttl_seconds": ttl_seconds,
        },
    )
    request_id = created.get("request_id")
    if not isinstance(request_id, str) or not request_id:
        raise RuntimeError(f"daemon did not return a request_id: {created!r}")
    completed = wait_for_wallet_request(
        client,
        request_id=request_id,
        timeout_seconds=wait_timeout_seconds,
    )
    return {
        "request": created,
        "request_id": request_id,
        "status": completed.get("status"),
        "result": completed["result"],
    }


def request_account_import(
    client: StarmaskDaemonClient,
    *,
    private_key_file: Path,
    account_address: str | None,
    wallet_instance_id: str,
    ttl_seconds: int,
    wait_timeout_seconds: float,
) -> dict[str, Any]:
    created = client.call_tool(
        "wallet_request_import_account",
        {
            "client_request_id": new_client_request_id("import-account"),
            "wallet_instance_id": wallet_instance_id,
            "private_key_file": str(private_key_file),
            "account_address": account_address,
            "display_hint": "Import local account private key",
            "client_context": "starcoin-transfer wallet_runtime import-account",
            "ttl_seconds": ttl_seconds,
        },
    )
    request_id = created.get("request_id")
    if not isinstance(request_id, str) or not request_id:
        raise RuntimeError(f"daemon did not return a request_id: {created!r}")
    completed = wait_for_wallet_request(
        client,
        request_id=request_id,
        timeout_seconds=wait_timeout_seconds,
    )
    return {
        "request": created,
        "request_id": request_id,
        "status": completed.get("status"),
        "result": completed["result"],
    }


def main() -> int:
    args = parse_args()
    runtime_dir = Path(args.runtime_dir).resolve()
    paths = runtime_paths(runtime_dir)

    if args.command == "status":
        status = load_status(runtime_dir)
        if args.json:
            print(json.dumps(status, indent=2, sort_keys=True))
        else:
            print_status(status)
        return 0

    if args.command == "down":
        result = stop_runtime(runtime_dir)
        if args.json:
            print(json.dumps(result, indent=2, sort_keys=True))
        else:
            print_status(result)
            print("runtime stopped")
        return 0

    if args.command == "export-account":
        status = require_wallet_runtime_ready(runtime_dir)
        destination_file = resolve_account_export_output_file(
            args.output_file,
            account_address=args.address,
        )
        wallet_instance_id = args.wallet_instance_id or status.get("wallet_instance_id")
        client = StarmaskDaemonClient(socket_path=Path(status["daemon_socket_path"]))
        payload = request_account_export(
            client,
            account_address=args.address,
            output_file=destination_file,
            wallet_instance_id=wallet_instance_id,
            force=args.force,
            ttl_seconds=args.ttl_seconds,
            wait_timeout_seconds=args.wait_timeout_seconds,
        )
        payload["runtime_dir"] = str(runtime_dir)
        payload["daemon_socket_path"] = status["daemon_socket_path"]
        payload["wallet_instance_id"] = wallet_instance_id
        if args.json:
            print(json.dumps(payload, indent=2, sort_keys=True))
        else:
            result = payload["result"]
            print(f"account private-key export created: {result.get('output_file', destination_file)}")
            print(f"address:                {result.get('address', args.address)}")
            print(f"request_id:             {payload['request_id']}")
            print(f"wallet_instance_id:     {wallet_instance_id}")
        return 0

    if args.command == "import-account":
        status = require_wallet_runtime_ready(runtime_dir)
        private_key_file = Path(args.private_key_file).expanduser().resolve()
        if not private_key_file.is_file():
            raise FileNotFoundError(f"private_key_file does not exist: {private_key_file}")
        wallet_instance_id = (
            args.wallet_instance_id
            or status.get("wallet_instance_id")
            or DEFAULT_WALLET_BACKEND_ID
        )
        client = StarmaskDaemonClient(socket_path=Path(status["daemon_socket_path"]))
        payload = request_account_import(
            client,
            private_key_file=private_key_file,
            account_address=args.address,
            wallet_instance_id=wallet_instance_id,
            ttl_seconds=args.ttl_seconds,
            wait_timeout_seconds=args.wait_timeout_seconds,
        )
        payload["runtime_dir"] = str(runtime_dir)
        payload["daemon_socket_path"] = status["daemon_socket_path"]
        payload["wallet_instance_id"] = wallet_instance_id
        if args.json:
            print(json.dumps(payload, indent=2, sort_keys=True))
        else:
            result = payload["result"]
            print(f"account private-key import completed: {result.get('address')}")
            print(f"public_key:             {result.get('public_key')}")
            print(f"request_id:             {payload['request_id']}")
            print(f"wallet_instance_id:     {wallet_instance_id}")
        return 0

    wallet_dir = Path(args.wallet_dir).resolve()
    ensure_private_wallet_dir(wallet_dir)

    if args.replace:
        stop_runtime(runtime_dir)
    else:
        current = load_status(runtime_dir)
        if current["starmaskd_running"] or current["agent_running"]:
            print_status(current)
            print("runtime already running; use --replace or run the down command", file=sys.stderr)
            return 1

    paths["run_dir"].mkdir(parents=True, exist_ok=True)
    paths["logs_dir"].mkdir(parents=True, exist_ok=True)
    socket_path = wallet_runtime_socket_path(runtime_dir)
    remove_if_exists(socket_path)

    wallet_config = build_wallet_config(
        socket_path=socket_path,
        database_path=paths["database_path"],
        wallet_dir=wallet_dir,
        backend_id=args.backend_id,
        instance_label=args.instance_label,
        chain_id=args.chain_id,
    )
    write_text(paths["config_path"], wallet_config)

    starmaskd_command, starmaskd_launch = launch_command(
        env_name="STARMASKD_BIN",
        binary_name="starmaskd",
        manifest_path=STARMASKD_MANIFEST,
        cargo_bin_name="starmaskd",
        program_args=["serve", "--config", str(paths["config_path"])],
    )
    starmaskd_log = paths["starmaskd_log_path"].open("w", encoding="utf-8")
    starmaskd = subprocess.Popen(
        starmaskd_command,
        cwd=str(WORKSPACE_ROOT),
        stdin=subprocess.DEVNULL,
        stdout=starmaskd_log,
        stderr=subprocess.STDOUT,
        text=True,
    )

    try:
        wait_for_socket(socket_path, starmaskd, paths["starmaskd_log_path"])

        agent_command, agent_launch = launch_command(
            env_name="LOCAL_ACCOUNT_AGENT_BIN",
            binary_name="local-account-agent",
            manifest_path=LOCAL_AGENT_MANIFEST,
            cargo_bin_name="local-account-agent",
            program_args=[
                "--config",
                str(paths["config_path"]),
                "--backend-id",
                args.backend_id,
            ],
        )
        agent = subprocess.Popen(
            agent_command,
            cwd=str(WORKSPACE_ROOT),
            text=True,
        )

        write_pid(paths["starmaskd_pid_path"], starmaskd.pid)
        write_pid(paths["agent_pid_path"], agent.pid)
        write_json(
            paths["metadata_path"],
            {
                "runtime_dir": str(runtime_dir),
                "wallet_dir": str(wallet_dir),
                "wallet_instance_id": args.backend_id,
                "chain_id": args.chain_id,
                "config_path": str(paths["config_path"]),
                "database_path": str(paths["database_path"]),
                "daemon_socket_path": str(socket_path),
                "starmaskd_pid": starmaskd.pid,
                "agent_pid": agent.pid,
                "starmaskd_launch": starmaskd_launch,
                "agent_launch": agent_launch,
                "starmaskd_log_path": str(paths["starmaskd_log_path"]),
                "agent_log_path": None,
            },
        )

        print(f"wallet runtime ready: {runtime_dir}")
        print(f"config_path:         {paths['config_path']}")
        print(f"daemon_socket_path:  {socket_path}")
        print(f"wallet_instance_id:  {args.backend_id}")
        print(f"starmaskd_pid:       {starmaskd.pid}")
        print(f"agent_pid:           {agent.pid}")
        print(f"starmaskd_launch:    {starmaskd_launch}")
        print(f"agent_launch:        {agent_launch}")
        print(f"metadata_path:       {paths['metadata_path']}")
        print(f"starmaskd_log_path:  {paths['starmaskd_log_path']}")
        print("agent_log_path:      attached to this terminal")
        print()
        print(
            "Keep this supervisor running. Start host-side tests in another terminal and point them"
        )
        print(f"at --wallet-runtime-dir {runtime_dir}.")

        while True:
            if starmaskd.poll() is not None:
                raise RuntimeError(
                    "starmaskd exited; see " + str(paths["starmaskd_log_path"])
                )
            agent_exit = agent.poll()
            if agent_exit is not None:
                return agent_exit
            time.sleep(0.5)
    finally:
        if 'agent' in locals() and agent.poll() is None:
            agent.send_signal(signal.SIGTERM)
            try:
                agent.wait(timeout=5)
            except subprocess.TimeoutExpired:
                agent.kill()
                agent.wait(timeout=5)
        if starmaskd.poll() is None:
            starmaskd.send_signal(signal.SIGTERM)
            try:
                starmaskd.wait(timeout=5)
            except subprocess.TimeoutExpired:
                starmaskd.kill()
                starmaskd.wait(timeout=5)
        starmaskd_log.close()
        remove_if_exists(paths["starmaskd_pid_path"])
        remove_if_exists(paths["agent_pid_path"])
        remove_if_exists(paths["metadata_path"])
        remove_if_exists(socket_path)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        raise SystemExit(130)
