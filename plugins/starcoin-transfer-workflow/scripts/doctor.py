#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import socket
import stat
import sys
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    import tomli as tomllib  # type: ignore


PLUGIN_ROOT = Path(__file__).resolve().parent.parent
WORKSPACE_ROOT = Path(
    os.environ.get(
        "STARCOIN_TRANSFER_WORKSPACE_ROOT",
        os.environ.get("STARCOIN_MCP_WORKSPACE_ROOT", str(PLUGIN_ROOT.parent.parent)),
    )
).resolve()
MARKETPLACE_PATH = WORKSPACE_ROOT / ".agents" / "plugins" / "marketplace.json"
PLUGIN_MANIFEST_PATH = PLUGIN_ROOT / ".codex-plugin" / "plugin.json"
NODE_CLIENT_SCRIPT_PATH = PLUGIN_ROOT / "scripts" / "node_cli_client.py"
WALLET_CLIENT_SCRIPT_PATH = PLUGIN_ROOT / "scripts" / "starmaskd_client.py"
NODE_EXAMPLE_PATH = PLUGIN_ROOT / "examples" / "node-cli.example.toml"
WALLET_EXAMPLE_PATH = PLUGIN_ROOT / "examples" / "starmaskd-local-account.example.toml"
DEFAULT_WALLET_RUNTIME_DIR = WORKSPACE_ROOT / ".runtime" / "wallet-runtime"

DEFAULT_NODE_MANIFEST = (
    WORKSPACE_ROOT
    / "starcoin-node-mcp"
    / "crates"
    / "starcoin-node-cli"
    / "Cargo.toml"
)
DEFAULT_STARMASKD_MANIFEST = (
    WORKSPACE_ROOT
    / "starmask-mcp"
    / "crates"
    / "starmaskd"
    / "Cargo.toml"
)
DEFAULT_LOCAL_AGENT_MANIFEST = (
    WORKSPACE_ROOT
    / "starmask-mcp"
    / "crates"
    / "starmask-local-account-agent"
    / "Cargo.toml"
)


def resolve_binary(env_name: str, binary_name: str) -> str | None:
    override = os.environ.get(env_name)
    if override:
        return override
    return shutil.which(binary_name)


def platform_paths() -> tuple[Path, Path, Path, Path]:
    system = platform.system()
    if system == "Darwin":
        root = Path.home() / "Library" / "Application Support" / "StarcoinMCP"
        return (
            root / "node-cli.toml",
            root / "node-mcp.toml",
            root / "config.toml",
            root / "run" / "starmaskd.sock",
        )
    if system == "Linux":
        config_home = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
        state_home = Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local" / "state"))
        runtime_dir = Path(os.environ.get("XDG_RUNTIME_DIR", state_home / "starcoin-mcp"))
        return (
            config_home / "starcoin-mcp" / "node-cli.toml",
            config_home / "starcoin-mcp" / "node-mcp.toml",
            config_home / "starcoin-mcp" / "config.toml",
            runtime_dir / "starcoin-mcp" / "starmaskd.sock"
            if runtime_dir.name != "starcoin-mcp"
            else runtime_dir / "starmaskd.sock",
        )
    root = Path.home() / "AppData" / "Roaming" / "StarcoinMCP"
    return (
        root / "node-cli.toml",
        root / "node-mcp.toml",
        root / "config.toml",
        root / "starmaskd.sock",
    )


def resolve_node_config_path(
    preferred_path: Path, legacy_path: Path
) -> tuple[Path, Path | None]:
    override = os.environ.get("STARCOIN_NODE_CLI_CONFIG") or os.environ.get(
        "STARCOIN_NODE_MCP_CONFIG"
    )
    if override:
        return Path(override).expanduser(), None
    if preferred_path.exists():
        return preferred_path, None
    if legacy_path.exists():
        return legacy_path, legacy_path
    return preferred_path, legacy_path


def parse_toml(path: Path) -> dict:
    try:
        with path.open("rb") as handle:
            return tomllib.load(handle)
    except Exception as exc:
        return {"_parse_error": str(exc)}


def parse_json(path: Path) -> dict | None:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return None
    except json.JSONDecodeError:
        return None


def socket_reachable(path: Path) -> tuple[bool, str]:
    if not path.exists():
        return False, "socket file is missing"
    if platform.system() == "Windows":
        return True, "socket existence check only"
    client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    client.settimeout(0.5)
    try:
        client.connect(str(path))
        return True, "unix socket accepted a connection"
    except OSError as exc:
        return False, str(exc)
    finally:
        client.close()


def is_unix_socket(path: Path) -> bool:
    try:
        return stat.S_ISSOCK(path.stat().st_mode)
    except OSError:
        return False


def stale_socket_cleanup_candidate(path: Path, detail: str) -> bool:
    if platform.system() == "Windows":
        return False
    if not path.exists() or not is_unix_socket(path):
        return False
    return "connection refused" in detail.lower()


def cleanup_stale_socket(path: Path, detail: str) -> tuple[bool, str] | None:
    if not stale_socket_cleanup_candidate(path, detail):
        return None
    try:
        path.unlink()
        return True, f"removed stale socket {path}"
    except OSError as exc:
        return False, f"failed to remove stale socket {path}: {exc}"


def check(name: str, ok: bool, detail: str, hint: str | None = None) -> dict:
    return {
        "name": name,
        "ok": ok,
        "detail": detail,
        "hint": hint,
    }


def format_status(item: dict) -> str:
    mark = "OK" if item["ok"] else "FAIL"
    line = f"[{mark}] {item['name']}: {item['detail']}"
    if item.get("hint"):
        return f"{line}\n      hint: {item['hint']}"
    return line


def looks_like_placeholder_rpc(node_rpc: object) -> bool:
    if not isinstance(node_rpc, str):
        return True
    lowered = node_rpc.strip().lower()
    if not lowered:
        return True
    return "example" in lowered or "replace" in lowered


def looks_like_placeholder_hash(expected_genesis_hash: object) -> bool:
    if not isinstance(expected_genesis_hash, str):
        return True
    lowered = expected_genesis_hash.strip().lower()
    if not lowered:
        return True
    return "replace" in lowered or not lowered.startswith("0x")


def resolve_runtime_metadata(runtime_dir_arg: str | None) -> tuple[Path, Path, dict | None]:
    runtime_dir = Path(
        os.environ.get(
            "STARMASK_WALLET_RUNTIME_DIR",
            runtime_dir_arg or str(DEFAULT_WALLET_RUNTIME_DIR),
        )
    ).expanduser()
    metadata_path = runtime_dir / "wallet-runtime.json"
    return runtime_dir, metadata_path, parse_json(metadata_path)


def resolve_daemon_socket_path(
    runtime_dir_arg: str | None, platform_socket_path: Path
) -> tuple[Path, Path, dict | None]:
    runtime_dir, metadata_path, metadata = resolve_runtime_metadata(runtime_dir_arg)
    if metadata is not None and metadata.get("daemon_socket_path"):
        return Path(metadata["daemon_socket_path"]), metadata_path, metadata
    daemon_socket_override = os.environ.get("STARMASKD_SOCKET_PATH") or os.environ.get(
        "STARMASK_MCP_DAEMON_SOCKET_PATH"
    )
    if daemon_socket_override:
        return Path(daemon_socket_override).expanduser(), metadata_path, metadata
    return platform_socket_path, metadata_path, metadata


def main() -> int:
    parser = argparse.ArgumentParser(description="Check the Starcoin transfer workflow plugin runtime.")
    parser.add_argument("--json", action="store_true", help="Emit machine-readable JSON")
    parser.add_argument(
        "--cleanup-stale-socket",
        action="store_true",
        help="Remove the daemon socket when it exists but refuses connections, then probe again.",
    )
    parser.add_argument(
        "--runtime-dir",
        default=None,
        help="Optional wallet runtime directory to probe for wallet-runtime.json before platform defaults.",
    )
    parser.add_argument(
        "--session-start",
        action="store_true",
        help="Emit a compact warning for Codex SessionStart hooks and stay silent when healthy",
    )
    args = parser.parse_args()

    (
        preferred_node_config_path,
        legacy_node_config_path,
        wallet_config_path,
        platform_socket_path,
    ) = platform_paths()
    node_config_path, legacy_node_config_hint = resolve_node_config_path(
        preferred_node_config_path, legacy_node_config_path
    )
    daemon_socket_path, runtime_metadata_path, runtime_metadata = resolve_daemon_socket_path(
        args.runtime_dir, platform_socket_path
    )

    node_bin = resolve_binary("STARCOIN_NODE_CLI_BIN", "starcoin-node-cli")
    starmaskd_bin = resolve_binary("STARMASKD_BIN", "starmaskd")
    local_agent_bin = resolve_binary("LOCAL_ACCOUNT_AGENT_BIN", "local-account-agent")
    node_uses_source_launch = node_bin is None
    requires_cargo = node_uses_source_launch

    node_manifest = Path(
        os.environ.get("STARCOIN_NODE_CLI_MANIFEST", str(DEFAULT_NODE_MANIFEST))
    )

    results = [
        check(
            "codex host",
            bool(os.environ.get("CODEX_HOME")) or shutil.which("codex") is not None,
            os.environ.get("CODEX_HOME") or shutil.which("codex") or "Codex desktop/CLI not detected",
            "Run from the Codex desktop app or ensure the codex CLI is on PATH.",
        ),
        check(
            "plugin manifest",
            PLUGIN_MANIFEST_PATH.exists(),
            str(PLUGIN_MANIFEST_PATH),
        ),
        check(
            "node client script",
            NODE_CLIENT_SCRIPT_PATH.exists(),
            str(NODE_CLIENT_SCRIPT_PATH),
        ),
        check(
            "wallet client script",
            WALLET_CLIENT_SCRIPT_PATH.exists(),
            str(WALLET_CLIENT_SCRIPT_PATH),
        ),
        check(
            "plugin marketplace",
            MARKETPLACE_PATH.exists(),
            str(MARKETPLACE_PATH),
            "Install or enable the plugin from this marketplace before asking Codex to use it.",
        ),
    ]

    if requires_cargo:
        results.append(
            check(
                "cargo binary",
                shutil.which("cargo") is not None,
                shutil.which("cargo") or "cargo is not on PATH",
                "The script-driven transfer path launches starcoin-node-cli from source through cargo run when no installed binary is available.",
            )
        )
    if node_uses_source_launch:
        results.append(
            check(
                "node cli manifest",
                node_manifest.exists(),
                str(node_manifest),
                "Override with STARCOIN_NODE_CLI_MANIFEST or STARCOIN_TRANSFER_WORKSPACE_ROOT if the source tree moved.",
            )
        )
    else:
        results.append(
            check(
                "node cli binary",
                True,
                str(node_bin),
                "Unset STARCOIN_NODE_CLI_BIN if you want doctor.py to validate the source-tree manifest instead.",
            )
        )

    daemon_ok, daemon_detail = socket_reachable(daemon_socket_path)
    socket_cleanup_result = None
    if args.cleanup_stale_socket and not daemon_ok:
        socket_cleanup_result = cleanup_stale_socket(daemon_socket_path, daemon_detail)
        if socket_cleanup_result is not None:
            daemon_ok, daemon_detail = socket_reachable(daemon_socket_path)

    if not daemon_ok:
        results.extend(
            [
                check(
                    "starmaskd launcher",
                    starmaskd_bin is not None or DEFAULT_STARMASKD_MANIFEST.exists(),
                    str(starmaskd_bin or DEFAULT_STARMASKD_MANIFEST),
                    "Install starmaskd on PATH, export STARMASKD_BIN, or point STARCOIN_TRANSFER_WORKSPACE_ROOT at a source tree that contains crates/starmaskd.",
                ),
                check(
                    "local-account-agent launcher",
                    local_agent_bin is not None or DEFAULT_LOCAL_AGENT_MANIFEST.exists(),
                    str(local_agent_bin or DEFAULT_LOCAL_AGENT_MANIFEST),
                    "Install local-account-agent on PATH, export LOCAL_ACCOUNT_AGENT_BIN, or point STARCOIN_TRANSFER_WORKSPACE_ROOT at a source tree that contains crates/starmask-local-account-agent.",
                ),
            ]
        )

    node_config = parse_toml(node_config_path) if node_config_path.exists() else {}
    wallet_config = parse_toml(wallet_config_path) if wallet_config_path.exists() else {}

    node_mode = node_config.get("mode") if isinstance(node_config, dict) else None
    node_rpc = node_config.get("rpc_endpoint_url") if isinstance(node_config, dict) else None
    expected_genesis_hash = (
        node_config.get("expected_genesis_hash") if isinstance(node_config, dict) else None
    )
    wallet_backends = wallet_config.get("wallet_backends") if isinstance(wallet_config, dict) else None

    results.extend(
        [
            check(
                "node config",
                node_config_path.exists() and "_parse_error" not in node_config,
                str(node_config_path),
                (
                    f"Copy {NODE_EXAMPLE_PATH} to {preferred_node_config_path}"
                    + (
                        f". The runtime also accepts the legacy path {legacy_node_config_hint}"
                        if legacy_node_config_hint
                        else ""
                    )
                    + ", or export STARCOIN_NODE_CLI_CONFIG."
                ),
            ),
            check(
                "node config mode",
                node_mode == "transaction",
                f"mode={node_mode!r}",
                f"Use {NODE_EXAMPLE_PATH} as the transaction-mode starting point.",
            ),
            check(
                "node rpc endpoint",
                not looks_like_placeholder_rpc(node_rpc),
                f"rpc_endpoint_url={node_rpc!r}",
                f"Set rpc_endpoint_url in the copied template from {NODE_EXAMPLE_PATH}.",
            ),
            check(
                "node genesis hash",
                not looks_like_placeholder_hash(expected_genesis_hash),
                f"expected_genesis_hash={expected_genesis_hash!r}",
                f"Replace the example genesis hash in {NODE_EXAMPLE_PATH} before attempting a transfer.",
            ),
            check(
                "wallet config",
                wallet_config_path.exists() and "_parse_error" not in wallet_config,
                str(wallet_config_path),
                f"Copy {WALLET_EXAMPLE_PATH} to the default starmaskd config path.",
            ),
            check(
                "wallet backends",
                bool(wallet_backends),
                f"wallet_backends configured={bool(wallet_backends)}",
                f"Use the [[wallet_backends]] entry from {WALLET_EXAMPLE_PATH}.",
            ),
        ]
        )

    if socket_cleanup_result is not None:
        cleanup_ok, cleanup_detail = socket_cleanup_result
        results.append(
            check(
                "stale socket cleanup",
                cleanup_ok,
                cleanup_detail,
                "Start starmaskd again after cleanup if you expect the default daemon socket to exist.",
            )
        )

    socket_hint = "Start starmaskd and the local-account-agent before asking Codex to sign."
    if stale_socket_cleanup_candidate(daemon_socket_path, daemon_detail):
        socket_hint = (
            f"Run `python3 {PLUGIN_ROOT / 'scripts' / 'doctor.py'} --cleanup-stale-socket` "
            "to remove the stale socket, then restart starmaskd and the local-account-agent."
        )

    results.append(
        check(
            "starmaskd socket",
            daemon_ok,
            f"{daemon_socket_path} ({daemon_detail})",
            socket_hint,
        )
    )

    payload = {
        "plugin_root": str(PLUGIN_ROOT),
        "workspace_root": str(WORKSPACE_ROOT),
        "node_config_path": str(node_config_path),
        "wallet_config_path": str(wallet_config_path),
        "daemon_socket_path": str(daemon_socket_path),
        "node_cli_bin": node_bin,
        "starmaskd_bin": starmaskd_bin,
        "local_account_agent_bin": local_agent_bin,
        "wallet_runtime_metadata_path": str(runtime_metadata_path),
        "wallet_runtime_metadata_found": runtime_metadata is not None,
        "checks": results,
        "next_steps": [
            "Install or enable the plugin from the marketplace entry shown above.",
            f"Copy and edit {NODE_EXAMPLE_PATH} and {WALLET_EXAMPLE_PATH} if the default configs are missing.",
            "Install starcoin-node-cli, starmaskd, and local-account-agent on PATH if you want a global plugin that does not rely on a source checkout.",
            "Start starmaskd and the wallet backend if the daemon socket check failed.",
            "Ask Codex to use the starcoin-transfer skill for one transfer after the checks pass.",
        ],
    }

    if args.json:
        json.dump(payload, sys.stdout, indent=2)
        sys.stdout.write("\n")
    elif args.session_start:
        failures = [item for item in results if not item["ok"]]
        if failures:
            names = ", ".join(item["name"] for item in failures[:4])
            if len(failures) > 4:
                names = f"{names}, ..."
            print(
                "[starcoin-transfer-workflow] transfer runtime is not ready "
                f"({names}). Run "
                f"`python3 {PLUGIN_ROOT / 'scripts' / 'doctor.py'}` "
                "and warn the user before attempting a transfer."
            )
    else:
        print(f"Plugin root: {PLUGIN_ROOT}")
        print(f"Workspace root: {WORKSPACE_ROOT}")
        print(f"Node config: {node_config_path}")
        print(f"Wallet config: {wallet_config_path}")
        print(f"Daemon socket: {daemon_socket_path}")
        print()
        for item in results:
            print(format_status(item))
        print()
        print("Next steps:")
        for step in payload["next_steps"]:
            print(f"- {step}")

    return 0 if all(item["ok"] for item in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
