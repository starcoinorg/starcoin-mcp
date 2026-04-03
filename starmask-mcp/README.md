# Starmask MCP

This subproject contains the current Starmask daemon-side implementation and its companion design
documents.

The repository no longer ships an in-tree `starmask-mcp` stdio adapter crate. The current Rust
workspace centers on `starmaskd`, `starmask-native-host`, `starmask-core`, and the local-account
agent/runtime pieces.

The design documents still record the adapter contract and related wallet workflow assumptions:

- `starmask-mcp` is a local MCP server exposed to MCP hosts such as Claude Code and Codex.
- `starmaskd` is a long-lived local daemon.
- Starmask can be reached either through the Chrome extension path or through a local
  `local_account_dir` backend agent.
- The MCP host entrypoint is `starmask-mcp`, not the Chrome extension directly.
- The first Rust implementation should prefer the official MCP Rust SDK `rmcp` only at the MCP shim layer.

## Contents

- `docs/starmask-mcp-interface-design.md`: detailed interface design draft
- `docs/security-model.md`: security assumptions, trust boundaries, and implementation gates
- `docs/daemon-protocol.md`: JSON-RPC contract between `starmask-mcp` and `starmaskd`
- `docs/native-messaging-contract.md`: bridge contract between the daemon side and the Chrome extension
- `docs/persistence-and-recovery.md`: request storage, lease, retention, and restart rules
- `docs/configuration.md`: default paths, timing, and policy settings
- `docs/approval-ui-spec.md`: approval UI interaction and information requirements
- `docs/testing-and-acceptance.md`: end-to-end acceptance matrix
- `docs/rust-implementation-strategy.md`: Rust workspace, runtime, persistence, and IPC strategy
- `docs/rust-core-api-design.md`: core crate API, coordinator command model, and repository traits
- `docs/sqlite-schema-and-migrations.md`: SQLite physical schema, indexes, and migration strategy
- `docs/rmcp-adapter-design.md`: MCP shim structure around `rmcp`
- `docs/native-messaging-examples.md`: canonical Native Messaging sample payloads
- `docs/test-harness-design.md`: test layering and fake-component strategy
- `docs/mcp-shim-coverage-matrix.md`: adapter-layer automated coverage and gap inventory
- `docs/local-automated-coverage-matrix.md`: workspace-level local automated coverage across daemon, native host, core, and diagnostics
- `docs/mcp-shim-real-environment-runbook.md`: real-environment validation steps and evidence checklist

## Status

Phase 2 is now implemented for the daemon-side generic backend contract and the first
`local_account_dir` agent over the local-socket binding. The extension-backed `v1` path remains
supported for compatibility, but the repository no longer includes the MCP stdio adapter binary.
