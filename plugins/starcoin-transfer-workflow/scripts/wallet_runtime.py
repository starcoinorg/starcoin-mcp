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
import time
from pathlib import Path
from typing import Any


PLUGIN_ROOT = Path(__file__).resolve().parent.parent
WORKSPACE_ROOT = PLUGIN_ROOT.parent.parent
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
RUNTIME_METADATA_NAME = "wallet-runtime.json"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run a foreground supervisor for the wallet-side local runtime."
    )
    parser.add_argument(
        "--runtime-dir",
        default=str(WORKSPACE_ROOT / ".runtime" / "wallet-runtime"),
        help="Directory for generated config, pid files, and logs.",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    up = subparsers.add_parser("up", help="Start starmaskd and local-account-agent.")
    up.add_argument(
        "--wallet-dir",
        default=str(WORKSPACE_ROOT / ".runtime" / "devwallet"),
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
        default="local-dev",
        help="Backend id registered with starmaskd.",
    )
    up.add_argument(
        "--instance-label",
        default="Local Dev Wallet",
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

    return parser.parse_args()


def choose_socket_path(runtime_dir: Path) -> Path:
    digest = hashlib.sha1(str(runtime_dir).encode("utf-8")).hexdigest()[:8]
    socket_dir = Path("/tmp") / "starcoin-mcp"
    socket_dir.mkdir(parents=True, exist_ok=True)
    return socket_dir / f"wallet-runtime-{digest}.sock"


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


def build_wallet_config(
    *,
    socket_path: Path,
    database_path: Path,
    wallet_dir: Path,
    backend_id: str,
    instance_label: str,
    chain_id: int,
) -> str:
    return f"""channel = "development"
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
backend_id = "{backend_id}"
backend_kind = "local_account_dir"
enabled = true
instance_label = "{instance_label}"
approval_surface = "tty_prompt"
prompt_mode = "tty_prompt"
account_dir = "{wallet_dir}"
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
    while time.time() < deadline:
        if socket_path.exists():
            return
        if starmaskd.poll() is not None:
            log_output = read_text_if_exists(log_path).strip()
            message = "starmaskd exited before creating the daemon socket"
            if log_output:
                message = f"{message}\n\n{log_output}"
            raise RuntimeError(message)
        time.sleep(0.2)
    log_output = read_text_if_exists(log_path).strip()
    message = "starmaskd socket did not appear in time"
    if log_output:
        message = f"{message}\n\n{log_output}"
    raise RuntimeError(message)


def load_status(runtime_dir: Path) -> dict[str, Any]:
    paths = runtime_paths(runtime_dir)
    metadata = read_json(paths["metadata_path"]) or {}
    socket_path = Path(
        metadata.get("daemon_socket_path", str(choose_socket_path(runtime_dir)))
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
    socket_path = choose_socket_path(runtime_dir)
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

    starmaskd_log = paths["starmaskd_log_path"].open("w", encoding="utf-8")
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
            str(paths["config_path"]),
        ],
        cwd=str(WORKSPACE_ROOT),
        stdin=subprocess.DEVNULL,
        stdout=starmaskd_log,
        stderr=subprocess.STDOUT,
        text=True,
    )

    try:
        wait_for_socket(socket_path, starmaskd, paths["starmaskd_log_path"])

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
                str(paths["config_path"]),
                "--backend-id",
                args.backend_id,
            ],
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
