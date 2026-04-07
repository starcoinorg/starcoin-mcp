#!/usr/bin/env python3
from __future__ import annotations

import os
import platform
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Mapping


DEFAULT_WALLET_RUNTIME_DIR = Path.home() / ".runtime" / "wallet-runtime"
RUNTIME_METADATA_NAME = "wallet-runtime.json"
DAEMON_SOCKET_ENV_NAME = "STARMASKD_SOCKET_PATH"
STARCOIN_NODE_CLI_MARKERS = (
    "starcoin-node/crates/starcoin-node-cli/Cargo.toml",
)
STARMASKD_MANIFEST_MARKERS = (
    "starmask-runtime/crates/starmaskd/Cargo.toml",
)


@dataclass(frozen=True)
class LinuxRuntimeDirs:
    config_home: Path
    runtime_dir: Path


@dataclass(frozen=True)
class PlatformRuntimePaths:
    runtime_root: Path
    node_config_path: Path
    wallet_config_path: Path
    daemon_socket_path: Path


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


def non_empty_env_path(name: str) -> Path | None:
    value = os.environ.get(name)
    if not value:
        return None
    return Path(value).expanduser()


def home_dir() -> Path:
    return Path.home()


def xdg_config_home(home: Path) -> Path:
    return non_empty_env_path("XDG_CONFIG_HOME") or home / ".config"


def xdg_state_home(home: Path) -> Path:
    return non_empty_env_path("XDG_STATE_HOME") or home / ".local" / "state"


def xdg_runtime_dir(state_home: Path) -> Path:
    return non_empty_env_path("XDG_RUNTIME_DIR") or state_home


def linux_runtime_dirs(home: Path) -> LinuxRuntimeDirs:
    state_home = xdg_state_home(home)
    return LinuxRuntimeDirs(
        config_home=xdg_config_home(home),
        runtime_dir=xdg_runtime_dir(state_home),
    )


def current_platform_paths() -> PlatformRuntimePaths:
    home = home_dir()
    runtime_root = home / ".runtime"
    system = platform.system()
    if system == "Darwin":
        support_dir = home / "Library" / "Application Support"
        return PlatformRuntimePaths(
            runtime_root=runtime_root,
            node_config_path=support_dir / "StarcoinNode" / "node-cli.toml",
            wallet_config_path=support_dir / "StarmaskRuntime" / "config.toml",
            daemon_socket_path=support_dir / "StarmaskRuntime" / "run" / "starmaskd.sock",
        )
    if system == "Linux":
        dirs = linux_runtime_dirs(home)
        return PlatformRuntimePaths(
            runtime_root=runtime_root,
            node_config_path=dirs.config_home / "starcoin-node" / "node-cli.toml",
            wallet_config_path=dirs.config_home / "starmask-runtime" / "config.toml",
            daemon_socket_path=dirs.runtime_dir / "starmask-runtime" / "starmaskd.sock",
        )
    roaming_dir = home / "AppData" / "Roaming"
    return PlatformRuntimePaths(
        runtime_root=runtime_root,
        node_config_path=roaming_dir / "StarcoinNode" / "node-cli.toml",
        wallet_config_path=roaming_dir / "StarmaskRuntime" / "config.toml",
        daemon_socket_path=roaming_dir / "StarmaskRuntime" / "starmaskd.sock",
    )


def metadata_daemon_socket_path(metadata: Mapping[str, object] | None) -> Path | None:
    if metadata is None:
        return None
    socket_path = metadata.get("daemon_socket_path")
    if not isinstance(socket_path, str) or not socket_path.strip():
        return None
    return Path(socket_path).expanduser()


def resolve_daemon_socket_override() -> Path | None:
    return non_empty_env_path(DAEMON_SOCKET_ENV_NAME)


def resolve_node_config_override() -> Path | None:
    return non_empty_env_path("STARCOIN_NODE_CLI_CONFIG")


def resolve_wallet_daemon_socket_path(
    runtime_dir: Path,
    *,
    metadata: Mapping[str, object] | None = None,
    default_socket_path: Path | None = None,
) -> Path:
    metadata_socket_path = metadata_daemon_socket_path(metadata)
    if metadata_socket_path is not None:
        return metadata_socket_path

    env_socket_path = resolve_daemon_socket_override()
    if env_socket_path is not None:
        return env_socket_path

    if default_socket_path is not None and runtime_dir == DEFAULT_WALLET_RUNTIME_DIR:
        return default_socket_path
    return wallet_runtime_socket_path(runtime_dir)


def platform_node_config_candidates() -> list[Path]:
    paths = current_platform_paths()
    return dedupe_paths(
        [
            paths.runtime_root / "node-cli.toml",
            paths.node_config_path,
        ]
    )


def platform_wallet_config_candidates() -> list[Path]:
    paths = current_platform_paths()
    return dedupe_paths(
        [
            DEFAULT_WALLET_RUNTIME_DIR / "starmaskd.toml",
            paths.runtime_root / "config.toml",
            paths.wallet_config_path,
        ]
    )


def platform_daemon_socket_candidates() -> list[Path]:
    paths = current_platform_paths()
    return dedupe_paths(
        [
            wallet_runtime_socket_path(DEFAULT_WALLET_RUNTIME_DIR),
            paths.runtime_root / "run" / "starmaskd.sock",
            paths.daemon_socket_path,
        ]
    )
