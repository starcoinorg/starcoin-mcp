#!/usr/bin/env python3
from __future__ import annotations

import os
import platform
from pathlib import Path
from typing import Iterable


DEFAULT_WALLET_RUNTIME_DIR = Path.home() / ".runtime" / "wallet-runtime"
RUNTIME_METADATA_NAME = "wallet-runtime.json"
STARCOIN_NODE_CLI_MARKERS = (
    "starcoin-node/crates/starcoin-node-cli/Cargo.toml",
    "starcoin-node-mcp/crates/starcoin-node-cli/Cargo.toml",
)
STARMASKD_MANIFEST_MARKERS = (
    "starmask-runtime/crates/starmaskd/Cargo.toml",
    "starmask-mcp/crates/starmaskd/Cargo.toml",
)


def first_env_value(*names: str) -> str | None:
    for name in names:
        value = os.environ.get(name)
        if value:
            return value
    return None


def dedupe_paths(paths: Iterable[Path]) -> list[Path]:
    unique: list[Path] = []
    seen: set[Path] = set()
    for path in paths:
        resolved = Path(path).expanduser().resolve()
        if resolved in seen:
            continue
        seen.add(resolved)
        unique.append(resolved)
    return unique


def resolve_existing_path(candidates: Iterable[Path]) -> Path:
    ordered_candidates = dedupe_paths(candidates)
    if not ordered_candidates:
        raise ValueError("expected at least one candidate path")
    for candidate in ordered_candidates:
        if candidate.exists():
            return candidate
    return ordered_candidates[0]


def resolve_workspace_root(plugin_root: Path, marker_paths: Iterable[str]) -> Path:
    env_override = first_env_value(
        "STARCOIN_TRANSFER_WORKSPACE_ROOT",
        "STARCOIN_WORKSPACE_ROOT",
        "STARCOIN_MCP_WORKSPACE_ROOT",
    )
    if env_override:
        return Path(env_override).expanduser().resolve()

    plugin_default = plugin_root.parent.parent.resolve()
    candidates = [plugin_default]
    cwd = Path.cwd().resolve()
    for base in (cwd, *cwd.parents):
        candidates.append(base)
        candidates.append(base / "starcoin-mcp")

    markers = tuple(marker_paths)
    seen: set[Path] = set()
    for candidate in candidates:
        candidate = candidate.resolve()
        if candidate in seen:
            continue
        seen.add(candidate)
        if any((candidate / marker).exists() for marker in markers):
            return candidate
    return plugin_default


def resolve_wallet_runtime_dir(runtime_dir_arg: str | None) -> Path:
    runtime_dir = runtime_dir_arg or os.environ.get(
        "STARMASK_WALLET_RUNTIME_DIR", str(DEFAULT_WALLET_RUNTIME_DIR)
    )
    return Path(runtime_dir).expanduser()


def wallet_runtime_metadata_path(runtime_dir: Path) -> Path:
    return Path(runtime_dir).expanduser() / RUNTIME_METADATA_NAME


def wallet_runtime_socket_path(runtime_dir: Path) -> Path:
    return Path(runtime_dir).expanduser() / "run" / "starmaskd.sock"


def platform_node_config_candidates() -> list[Path]:
    runtime_root = Path.home() / ".runtime"
    system = platform.system()
    if system == "Darwin":
        preferred = [Path.home() / "Library" / "Application Support" / "StarcoinNode" / "node-cli.toml"]
        legacy = [
            Path.home() / "Library" / "Application Support" / "StarcoinMCP" / "node-cli.toml",
            Path.home() / "Library" / "Application Support" / "StarcoinMCP" / "node-mcp.toml",
        ]
    elif system == "Linux":
        config_home = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
        preferred = [config_home / "starcoin-node" / "node-cli.toml"]
        legacy = [
            config_home / "starcoin-mcp" / "node-cli.toml",
            config_home / "starcoin-mcp" / "node-mcp.toml",
        ]
    else:
        config_home = Path.home() / "AppData" / "Roaming"
        preferred = [config_home / "StarcoinNode" / "node-cli.toml"]
        legacy = [
            config_home / "StarcoinMCP" / "node-cli.toml",
            config_home / "StarcoinMCP" / "node-mcp.toml",
        ]
    return dedupe_paths(
        [
            runtime_root / "node-cli.toml",
            runtime_root / "node-mcp.toml",
            *preferred,
            *legacy,
        ]
    )


def platform_wallet_config_candidates() -> list[Path]:
    runtime_root = Path.home() / ".runtime"
    system = platform.system()
    if system == "Darwin":
        preferred = [Path.home() / "Library" / "Application Support" / "StarmaskRuntime" / "config.toml"]
        legacy = [Path.home() / "Library" / "Application Support" / "StarcoinMCP" / "config.toml"]
    elif system == "Linux":
        config_home = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
        preferred = [config_home / "starmask-runtime" / "config.toml"]
        legacy = [config_home / "starcoin-mcp" / "config.toml"]
    else:
        config_home = Path.home() / "AppData" / "Roaming"
        preferred = [config_home / "StarmaskRuntime" / "config.toml"]
        legacy = [config_home / "StarcoinMCP" / "config.toml"]
    return dedupe_paths(
        [
            DEFAULT_WALLET_RUNTIME_DIR / "starmaskd.toml",
            runtime_root / "config.toml",
            *preferred,
            *legacy,
        ]
    )


def platform_daemon_socket_candidates() -> list[Path]:
    runtime_root = Path.home() / ".runtime"
    system = platform.system()
    if system == "Darwin":
        preferred = [Path.home() / "Library" / "Application Support" / "StarmaskRuntime" / "run" / "starmaskd.sock"]
    elif system == "Linux":
        state_home = Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local" / "state"))
        runtime_dir = Path(os.environ.get("XDG_RUNTIME_DIR", state_home))
        preferred = [runtime_dir / "starmask-runtime" / "starmaskd.sock"]
    else:
        preferred = [Path.home() / "AppData" / "Roaming" / "StarmaskRuntime" / "starmaskd.sock"]
    return dedupe_paths(
        [
            wallet_runtime_socket_path(DEFAULT_WALLET_RUNTIME_DIR),
            runtime_root / "run" / "starmaskd.sock",
            *preferred,
        ]
    )
