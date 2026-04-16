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
from typing import Any
from urllib.error import URLError
from urllib.parse import urlsplit, urlunsplit
from urllib.request import Request, urlopen

from runtime_layout import (
    STARMASKD_MANIFEST_MARKERS,
    STARCOIN_NODE_CLI_MARKERS,
    platform_daemon_socket_candidates,
    platform_node_config_candidates,
    platform_wallet_config_candidates,
    resolve_daemon_socket_override,
    resolve_existing_path,
    resolve_node_config_override,
    resolve_wallet_daemon_socket_path,
    resolve_wallet_runtime_dir,
    resolve_workspace_root,
    wallet_runtime_metadata_path,
)

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    import tomli as tomllib  # type: ignore


PLUGIN_ROOT = Path(__file__).resolve().parent.parent


WORKSPACE_ROOT = resolve_workspace_root(
    PLUGIN_ROOT,
    (*STARCOIN_NODE_CLI_MARKERS, *STARMASKD_MANIFEST_MARKERS),
)
MARKETPLACE_PATH = WORKSPACE_ROOT / ".agents" / "plugins" / "marketplace.json"
PLUGIN_MANIFEST_PATH = PLUGIN_ROOT / ".codex-plugin" / "plugin.json"
NODE_CLIENT_SCRIPT_PATH = PLUGIN_ROOT / "scripts" / "node_cli_client.py"
WALLET_CLIENT_SCRIPT_PATH = PLUGIN_ROOT / "scripts" / "starmaskd_client.py"
NODE_EXAMPLE_PATH = PLUGIN_ROOT / "examples" / "node-cli.example.toml"
WALLET_EXAMPLE_PATH = PLUGIN_ROOT / "examples" / "starmaskd-local-account.example.toml"

DEFAULT_NODE_MANIFEST = (
    WORKSPACE_ROOT
    / "starcoin-node"
    / "crates"
    / "starcoin-node-cli"
    / "Cargo.toml"
)
DEFAULT_STARMASKD_MANIFEST = (
    WORKSPACE_ROOT
    / "starmask-runtime"
    / "crates"
    / "starmaskd"
    / "Cargo.toml"
)
DEFAULT_LOCAL_AGENT_MANIFEST = (
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


def platform_paths() -> tuple[Path, Path, Path, Path]:
    node_candidates = platform_node_config_candidates()
    wallet_candidates = platform_wallet_config_candidates()
    socket_candidates = platform_daemon_socket_candidates()
    return (
        node_candidates[0],
        resolve_existing_path(node_candidates[1:]) if len(node_candidates) > 1 else node_candidates[0],
        resolve_existing_path(wallet_candidates),
        select_socket_candidate(socket_candidates),
    )


def resolve_node_config_path(preferred_path: Path, fallback_path: Path) -> Path:
    override = resolve_node_config_override()
    if override:
        return override.resolve()
    if preferred_path.exists():
        return preferred_path
    if fallback_path.exists():
        return fallback_path
    return preferred_path


def parse_toml(path: Path) -> dict:
    try:
        with path.open("rb") as handle:
            return tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as exc:
        return {"_parse_error": str(exc)}


def parse_json(path: Path) -> dict | None:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return None
    except json.JSONDecodeError:
        return None


def json_rpc(
    url: str, method: str, params: list[Any] | dict[str, Any] | None = None
) -> Any:
    validate_rpc_url(url)
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
    with urlopen(request, timeout=3) as response:
        body = json.loads(response.read().decode("utf-8"))
    if "error" in body:
        raise RuntimeError(f"{method} failed: {body['error']}")
    return body["result"]


def validate_rpc_url(url: str) -> None:
    parts = urlsplit(url)
    if parts.scheme not in {"http", "https"} or not parts.netloc:
        raise ValueError("RPC endpoint must be an http or https URL with a host")


def extract_chain_field(value: Any, field: str) -> Any:
    if isinstance(value, dict):
        if field in value:
            return value[field]
        peer_info = value.get("peer_info")
        if isinstance(peer_info, dict):
            chain_info = peer_info.get("chain_info")
            if isinstance(chain_info, dict) and field in chain_info:
                return chain_info[field]
    return None


def first_chain_field(payloads: tuple[Any, ...], fields: tuple[str, ...]) -> Any:
    for payload in payloads:
        for field in fields:
            value = extract_chain_field(payload, field)
            if value is not None:
                return value
    return None


def live_rpc_checks(
    node_rpc: object,
    *,
    expected_chain_id: object,
    expected_network: object,
    expected_genesis_hash: object,
) -> list[dict]:
    redacted_rpc = redacted_url_repr(node_rpc)
    if looks_like_placeholder_rpc(node_rpc):
        return [
            check(
                "node rpc live",
                False,
                f"rpc_endpoint_url={redacted_rpc}",
                "Set rpc_endpoint_url before running the live RPC probe.",
            )
        ]
    assert isinstance(node_rpc, str)
    try:
        node_info = json_rpc(node_rpc, "node.info")
    except (OSError, TimeoutError, RuntimeError, ValueError, KeyError, URLError) as exc:
        return [
            check(
                "node rpc live",
                False,
                f"rpc_endpoint_url={redacted_rpc}, error_type={type(exc).__name__}",
                "Start the Starcoin RPC node or fix rpc_endpoint_url in node-cli.toml.",
            )
        ]
    try:
        chain_info = json_rpc(node_rpc, "chain.info")
    except (OSError, TimeoutError, RuntimeError, ValueError, KeyError, URLError):
        chain_info = {}

    observed_chain_id = first_chain_field((chain_info, node_info), ("chain_id",))
    observed_network = first_chain_field(
        (node_info, chain_info), ("net", "network")
    )
    observed_genesis_hash = first_chain_field(
        (chain_info, node_info), ("genesis_hash",)
    )

    results = [
        check(
            "node rpc live",
            True,
            f"rpc_endpoint_url={redacted_rpc}, node.info responded",
        )
    ]
    results.append(
        check(
            "node rpc chain id",
            expected_chain_id is not None and str(observed_chain_id) == str(expected_chain_id),
            f"observed={observed_chain_id!r}, expected={expected_chain_id!r}",
            "Set expected_chain_id and use the RPC endpoint that matches it.",
        )
    )
    expected_network_text = str(expected_network or "").strip()
    results.append(
        check(
            "node rpc network",
            bool(expected_network_text)
            and str(observed_network or "").lower() == expected_network_text.lower(),
            f"observed={observed_network!r}, expected={expected_network!r}",
            "Set expected_network and use the RPC endpoint that matches it.",
        )
    )
    if not looks_like_placeholder_hash(expected_genesis_hash):
        results.append(
            check(
                "node rpc genesis hash",
                str(observed_genesis_hash or "").lower()
                == str(expected_genesis_hash or "").lower(),
                f"observed={observed_genesis_hash!r}, expected={expected_genesis_hash!r}",
                "Use the RPC endpoint that matches expected_genesis_hash, or update node-cli.toml after verifying the target chain.",
            )
        )
    return results


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


def select_socket_candidate(candidates: list[Path]) -> Path:
    if platform.system() == "Windows":
        return resolve_existing_path(candidates)

    socket_candidates: list[Path] = []
    for candidate in candidates:
        if not candidate.exists() or not is_unix_socket(candidate):
            continue
        socket_candidates.append(candidate)
        reachable, _ = socket_reachable(candidate)
        if reachable:
            return candidate

    if socket_candidates:
        return socket_candidates[0]
    return resolve_existing_path(candidates)


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


def redacted_url_repr(value: object) -> str:
    if not isinstance(value, str):
        return repr(value)
    try:
        parts = urlsplit(value)
    except ValueError:
        return repr(value)
    netloc = parts.netloc
    if "@" in netloc:
        netloc = "<redacted>@" + netloc.rsplit("@", 1)[1]
    query = "<redacted>" if parts.query else ""
    fragment = "<redacted>" if parts.fragment else ""
    return repr(urlunsplit((parts.scheme, netloc, parts.path, query, fragment)))


def looks_like_placeholder_hash(expected_genesis_hash: object) -> bool:
    if not isinstance(expected_genesis_hash, str):
        return True
    lowered = expected_genesis_hash.strip().lower()
    if not lowered:
        return True
    return "replace" in lowered or not lowered.startswith("0x")


def resolve_runtime_metadata(runtime_dir_arg: str | None) -> tuple[Path, Path, dict | None]:
    runtime_dir = resolve_wallet_runtime_dir(runtime_dir_arg)
    metadata_path = wallet_runtime_metadata_path(runtime_dir)
    return runtime_dir, metadata_path, parse_json(metadata_path)


def resolve_daemon_socket_path(
    runtime_dir_arg: str | None,
) -> tuple[Path, Path, dict | None]:
    runtime_dir, metadata_path, metadata = resolve_runtime_metadata(runtime_dir_arg)
    if (
        runtime_dir_arg is None
        and metadata is None
        and resolve_daemon_socket_override() is None
    ):
        socket_path = select_socket_candidate(platform_daemon_socket_candidates())
    else:
        socket_path = resolve_wallet_daemon_socket_path(
            runtime_dir,
            metadata=metadata,
        )
    return socket_path, metadata_path, metadata


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
        help="Emit a compact warning for agentic-host session hooks and stay silent when healthy",
    )
    args = parser.parse_args()

    (
        preferred_node_config_path,
        fallback_node_config_path,
        wallet_config_path,
        platform_socket_path,
    ) = platform_paths()
    node_config_path = resolve_node_config_path(
        preferred_node_config_path, fallback_node_config_path
    )
    daemon_socket_path, runtime_metadata_path, runtime_metadata = resolve_daemon_socket_path(
        args.runtime_dir
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
            "agentic host",
            bool(os.environ.get("CODEX_HOME")) or shutil.which("codex") is not None,
            os.environ.get("CODEX_HOME")
            or shutil.which("codex")
            or "No supported agentic host session detected",
            "Run from your agentic host session. Codex desktop or the codex CLI is the currently supported detection target.",
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
            "Install or enable the plugin from this marketplace before asking an agentic host to use it.",
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
    expected_chain_id = (
        node_config.get("expected_chain_id") if isinstance(node_config, dict) else None
    )
    expected_network = (
        node_config.get("expected_network") if isinstance(node_config, dict) else None
    )
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
                f"Copy {NODE_EXAMPLE_PATH} to {preferred_node_config_path}, or export STARCOIN_NODE_CLI_CONFIG.",
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
                f"rpc_endpoint_url={redacted_url_repr(node_rpc)}",
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
    if node_config_path.exists() and "_parse_error" not in node_config:
        results.extend(
            live_rpc_checks(
                node_rpc,
                expected_chain_id=expected_chain_id,
                expected_network=expected_network,
                expected_genesis_hash=expected_genesis_hash,
            )
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

    socket_hint = "Start starmaskd and the local-account-agent before asking the host to sign."
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
            "Make sure the configured Starcoin RPC endpoint responds and matches the expected chain identity.",
            "Install starcoin-node-cli, starmaskd, and local-account-agent on PATH if you want a global plugin that does not rely on a source checkout.",
            "Start starmaskd and the wallet backend if the daemon socket check failed.",
            "Ask your agentic host to use the starcoin-transfer skill for one transfer after the checks pass.",
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
