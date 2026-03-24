# Starmask MCP

This subproject contains the interface design draft for `starmask-mcp` under the following assumptions:

- `starmask-mcp` is a local MCP server exposed to MCP hosts such as Claude Code and Codex.
- `starmaskd` is a long-lived local daemon.
- Starmask is implemented as a Chrome extension and acts as the signing backend and approval UI.
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

## Status

Design is now closed far enough to begin Phase 1 Rust implementation, local integration work, and acceptance-oriented test scaffolding.
