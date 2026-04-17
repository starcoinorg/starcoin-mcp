#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

from runtime_layout import (
    STARCOIN_NODE_CLI_MARKERS,
    platform_node_config_candidates,
    resolve_existing_path,
    resolve_node_config_override,
    resolve_workspace_root,
)
from transfer_controller import normalize_vm_profile

PLUGIN_ROOT = Path(__file__).resolve().parent.parent
VM_PROFILE_LINE_PATTERN = re.compile(r"(?m)^\s*vm_profile\s*=\s*.*$")


WORKSPACE_ROOT = resolve_workspace_root(
    PLUGIN_ROOT, STARCOIN_NODE_CLI_MARKERS
)
NODE_CLI_MANIFEST = (
    WORKSPACE_ROOT / "starcoin-node" / "crates" / "starcoin-node-cli" / "Cargo.toml"
)


def platform_config_candidates() -> list[Path]:
    return platform_node_config_candidates()


def resolve_config_path(config_arg: str | None) -> Path:
    if config_arg:
        return Path(config_arg).expanduser().resolve()
    override = resolve_node_config_override()
    if override is not None:
        return override.resolve()
    return resolve_existing_path(platform_config_candidates())


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
    def __init__(
        self,
        *,
        config_path: Path,
        workspace_root: Path = WORKSPACE_ROOT,
        vm_profile: str | None = None,
    ):
        self.config_path = Path(config_path).resolve()
        self.workspace_root = Path(workspace_root).resolve()
        self.vm_profile = normalize_vm_profile(vm_profile) if vm_profile is not None else None

    def call_tool(self, name: str, arguments: dict[str, Any] | None = None) -> dict[str, Any]:
        payload = json.dumps(arguments or {}, separators=(",", ":"))
        config_path = self.config_path
        temporary_config_path: Path | None = None
        if self.vm_profile is not None:
            temporary_config_path = write_vm_profile_override_config(
                self.config_path, self.vm_profile
            )
            config_path = temporary_config_path
        command = launch_command(
            env_name="STARCOIN_NODE_CLI_BIN",
            binary_name="starcoin-node-cli",
            manifest_path=Path(
                os.environ.get("STARCOIN_NODE_CLI_MANIFEST", str(NODE_CLI_MANIFEST))
            ).resolve(),
            cargo_bin_name="starcoin-node-cli",
            program_args=["--config", str(config_path), "call", name],
        )
        try:
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
        finally:
            if temporary_config_path is not None:
                temporary_config_path.unlink(missing_ok=True)


def rewrite_vm_profile_config_text(config_text: str, vm_profile: str) -> str:
    replacement = f'vm_profile = "{normalize_vm_profile(vm_profile)}"'
    if VM_PROFILE_LINE_PATTERN.search(config_text):
        return VM_PROFILE_LINE_PATTERN.sub(replacement, config_text, count=1)
    if config_text and not config_text.endswith("\n"):
        config_text += "\n"
    return f"{config_text}{replacement}\n"


def write_vm_profile_override_config(config_path: Path, vm_profile: str) -> Path:
    original_config = config_path.read_text(encoding="utf-8")
    updated_config = rewrite_vm_profile_config_text(original_config, vm_profile)
    with tempfile.NamedTemporaryFile(
        "w", encoding="utf-8", suffix=".toml", delete=False
    ) as handle:
        handle.write(updated_config)
        return Path(handle.name)


def read_json_arguments(arguments_json: str | None = None) -> dict[str, Any]:
    if arguments_json is not None:
        raw = arguments_json
    elif sys.stdin.isatty():
        return {}
    else:
        raw = sys.stdin.read()
    if not raw.strip():
        return {}
    value = json.loads(raw)
    if not isinstance(value, dict):
        raise RuntimeError("tool arguments must be a JSON object")
    return value


def parse_args() -> argparse.Namespace:
    def parse_vm_profile_arg(value: str) -> str:
        try:
            return normalize_vm_profile(value)
        except ValueError as exc:
            raise argparse.ArgumentTypeError(str(exc)) from exc

    parser = argparse.ArgumentParser(
        description="Call the standalone starcoin-node-cli with JSON arguments on stdin."
    )
    parser.add_argument(
        "--config",
        default=None,
        help="Path to the node runtime TOML config. Defaults to node-cli.toml.",
    )
    parser.add_argument(
        "--vm-profile",
        type=parse_vm_profile_arg,
        default=None,
        help="Override vm_profile for this invocation without editing the base config file.",
    )
    parser.add_argument(
        "--workspace-root",
        default=str(WORKSPACE_ROOT),
        help="Workspace root used for source-tree cargo launches.",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)
    call = subparsers.add_parser("call", help="Call one chain-side tool.")
    call.add_argument("tool", help="Tool name, for example chain_status or prepare_transfer.")
    call.add_argument(
        "arguments_json",
        nargs="?",
        help="Optional JSON object arguments. If omitted, arguments are read from stdin.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    client = NodeCliClient(
        config_path=resolve_config_path(args.config),
        workspace_root=Path(args.workspace_root),
        vm_profile=args.vm_profile,
    )
    if args.command != "call":
        raise SystemExit(f"unsupported command: {args.command}")
    result = client.call_tool(args.tool, read_json_arguments(args.arguments_json))
    json.dump(result, sys.stdout, separators=(",", ":"))
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
