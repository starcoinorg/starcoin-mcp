#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any


PLUGIN_ROOT = Path(__file__).resolve().parent.parent
WORKSPACE_ROOT = Path(
    os.environ.get(
        "STARCOIN_TRANSFER_WORKSPACE_ROOT",
        os.environ.get("STARCOIN_MCP_WORKSPACE_ROOT", str(PLUGIN_ROOT.parent.parent)),
    )
).resolve()
NODE_CLI_MANIFEST = (
    WORKSPACE_ROOT / "starcoin-node-mcp" / "crates" / "starcoin-node-cli" / "Cargo.toml"
)


def platform_config_candidates() -> list[Path]:
    system = platform.system()
    if system == "Darwin":
        config_dir = Path.home() / "Library" / "Application Support" / "StarcoinMCP"
    elif system == "Linux":
        config_dir = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config")) / "starcoin-mcp"
    else:
        config_dir = Path.home() / "AppData" / "Roaming" / "StarcoinMCP"
    return [
        config_dir / "node-cli.toml",
        config_dir / "node-mcp.toml",
    ]


def resolve_config_path(config_arg: str | None) -> Path:
    if config_arg:
        return Path(config_arg).expanduser().resolve()
    for env_name in ("STARCOIN_NODE_CLI_CONFIG", "STARCOIN_NODE_MCP_CONFIG"):
        override = os.environ.get(env_name)
        if override:
            return Path(override).expanduser().resolve()
    candidates = platform_config_candidates()
    for candidate in candidates:
        if candidate.exists():
            return candidate.resolve()
    return candidates[0].resolve()


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


class NodeCliClient:
    def __init__(self, *, config_path: Path, workspace_root: Path = WORKSPACE_ROOT):
        self.config_path = Path(config_path).resolve()
        self.workspace_root = Path(workspace_root).resolve()

    def call_tool(self, name: str, arguments: dict[str, Any] | None = None) -> dict[str, Any]:
        payload = json.dumps(arguments or {}, separators=(",", ":"))
        command = launch_command(
            env_name="STARCOIN_NODE_CLI_BIN",
            binary_name="starcoin-node-cli",
            manifest_path=Path(
                os.environ.get("STARCOIN_NODE_CLI_MANIFEST", str(NODE_CLI_MANIFEST))
            ).resolve(),
            cargo_bin_name="starcoin-node-cli",
            program_args=["--config", str(self.config_path), "call", name],
        )
        completed = subprocess.run(
            command,
            cwd=str(self.workspace_root),
            input=payload,
            text=True,
            capture_output=True,
        )
        if completed.returncode != 0:
            stderr = completed.stderr.strip()
            raise RuntimeError(
                f"starcoin-node-cli {name} failed with exit code {completed.returncode}: {stderr}"
            )
        stdout = completed.stdout.strip()
        if not stdout:
            raise RuntimeError(f"starcoin-node-cli {name} returned empty stdout")
        return json.loads(stdout)


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
        description="Call the non-MCP starcoin-node-cli with JSON arguments on stdin."
    )
    parser.add_argument(
        "--config",
        default=None,
        help="Path to the node runtime TOML config. Defaults to node-cli.toml, then falls back to node-mcp.toml.",
    )
    parser.add_argument(
        "--workspace-root",
        default=str(WORKSPACE_ROOT),
        help="Workspace root used for source-tree cargo launches.",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)
    call = subparsers.add_parser("call", help="Call one chain-side tool.")
    call.add_argument("tool", help="Tool name, for example chain_status or prepare_transfer.")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    client = NodeCliClient(
        config_path=resolve_config_path(args.config),
        workspace_root=Path(args.workspace_root),
    )
    if args.command != "call":
        raise SystemExit(f"unsupported command: {args.command}")
    result = client.call_tool(args.tool, read_json_arguments())
    json.dump(result, sys.stdout, separators=(",", ":"))
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
