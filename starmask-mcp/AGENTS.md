# Starmask MCP

This file adds stricter instructions for the `starmask-mcp` subproject.

## Architecture Boundary

- Keep the following split explicit:
  - `starmaskd`: lifecycle owner and persistence owner
  - `starmask-native-host`: Chrome Native Messaging bridge
  - `Starmask` extension: approval UI and signing authority
- Do not move signing logic into Rust binaries.
- Do not move lifecycle ownership into transport adapters.

## Rust Implementation Shape

- Prefer the workspace structure described in:
  - `docs/rust-implementation-strategy.md`
- Prefer:
  - `starmask-types`
  - `starmask-core`
  - `starmaskd`
  - `starmask-native-host`
  - `starmaskctl`

## MCP SDK Usage

- The workspace does not currently ship an in-tree MCP server boundary.
- If a dedicated MCP adapter crate is reintroduced, prefer the official Rust MCP SDK `rmcp` there
  and keep MCP SDK dependencies scoped to that adapter crate.
- Do not couple `starmask-core` or `starmaskd` to MCP SDK types.

This is a project-wide layering rule, not a special exception for this subproject.

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
- If a task requires `unsafe`, treat it as a blocking safety item: finish isolating, reviewing,
  and testing that unsafe code before continuing unrelated code generation in the surrounding area.
- Never log:
  - private keys
  - raw signed transaction payloads
  - full message signatures at normal log levels

## Commit and Push Discipline

- Name new task branches as `codex/<kind>/<topic>` by default.
- Prefer `feat`, `fix`, `refactor`, `chore`, `docs`, or `test` as the `<kind>` segment.
- Use `refactor` only for behavior-preserving structural changes, and do not default to `chore` when a more specific kind applies.
- For each new `starmask-mcp` task branch, prefer a dedicated git worktree based on the latest `main`.
- Reuse the current worktree only when the user explicitly requests it or when the task is to continue an already-dirty in-place branch.
- If continuing in a dirty worktree, explain why that is safer than creating a fresh worktree before making substantial edits.
- After a `starmask-mcp` change reaches a verified milestone, commit it promptly.
- Push promptly when the user asks, when the branch has reached a reviewable checkpoint, or when the remote branch should preserve current progress.
- Do not push partially integrated protocol, lifecycle, or persistence changes before they have passed the relevant tests and smoke checks.

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
- `docs/rust-core-api-design.md`
- `docs/sqlite-schema-and-migrations.md`
- `docs/rmcp-adapter-design.md`
- `docs/native-messaging-examples.md`
- `docs/test-harness-design.md`

If a change alters subproject-specific workflow or layering guidance, update this `AGENTS.md` in the same change.
