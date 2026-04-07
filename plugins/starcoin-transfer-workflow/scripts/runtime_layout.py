#!/usr/bin/env python3
from __future__ import annotations

import os
from pathlib import Path
from typing import Iterable


DEFAULT_WALLET_RUNTIME_DIR = Path.home() / ".runtime" / "wallet-runtime"
RUNTIME_METADATA_NAME = "wallet-runtime.json"


def resolve_workspace_root(plugin_root: Path, marker_paths: Iterable[str]) -> Path:
    env_override = os.environ.get("STARCOIN_TRANSFER_WORKSPACE_ROOT") or os.environ.get(
        "STARCOIN_MCP_WORKSPACE_ROOT"
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
