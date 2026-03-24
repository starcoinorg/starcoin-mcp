# Starmask MCP

This file adds stricter instructions for the `starmask-mcp` subproject.

## Architecture Boundary

- Keep the following split explicit:
  - `starmask-mcp`: MCP stdio adapter
  - `starmaskd`: lifecycle owner and persistence owner
  - `starmask-native-host`: Chrome Native Messaging bridge
  - `Starmask` extension: approval UI and signing authority
- Do not move signing logic into Rust binaries.
- Do not move lifecycle ownership into the MCP adapter.

## Rust Implementation Shape

- Prefer the workspace structure described in:
  - `docs/rust-implementation-strategy.md`
- Prefer:
  - `starmask-types`
  - `starmask-core`
  - `starmaskd`
  - `starmask-mcp`
  - `starmask-native-host`
  - `starmaskctl`

## MCP SDK Usage

- Prefer the official Rust MCP SDK `rmcp` for the MCP server boundary.
- Keep `rmcp` dependency scoped to the `starmask-mcp` crate.
- Do not couple `starmask-core` or `starmaskd` to MCP SDK types.

## Domain Types

- Use typed ids and enums in Rust core code:
  - `RequestId`
  - `ClientRequestId`
  - `WalletInstanceId`
  - `DeliveryLeaseId`
  - `PresentationId`
  - `RequestStatus`
  - `RequestKind`
- Keep raw protocol strings at serialization boundaries only.

## State Machine Rules

- All lifecycle mutations must go through one coordinator path.
- Transport tasks may decode and forward commands, but should not mutate canonical state directly.
- Persistence writes should happen inside explicit transactions.

## Persistence Rules

- Keep SQLite access behind repository traits.
- Prefer `rusqlite` in the first implementation.
- Enable WAL mode, foreign keys, and a busy timeout at startup.
- Do not scatter SQL across MCP handlers or Native Messaging handlers.

## Native Messaging Rules

- Implement Chrome framing exactly:
  - UTF-8 JSON
  - 32-bit native-endian length prefix
- Never write logs to stdout.
- Keep stdout reserved for protocol frames.
- Use stderr for diagnostics.

## Safety and Logging

- Default to `#![forbid(unsafe_code)]`.
- If platform-specific unsafe code is unavoidable, isolate it in the smallest possible module.
- Never log:
  - private keys
  - raw signed transaction payloads
  - full message signatures at normal log levels

## Required Doc Sync

Any change to behavior covered by these docs must update the docs in the same change:

- `docs/starmask-mcp-interface-design.md`
- `docs/security-model.md`
- `docs/daemon-protocol.md`
- `docs/native-messaging-contract.md`
- `docs/persistence-and-recovery.md`
- `docs/configuration.md`
- `docs/approval-ui-spec.md`
- `docs/testing-and-acceptance.md`
- `docs/rust-implementation-strategy.md`
