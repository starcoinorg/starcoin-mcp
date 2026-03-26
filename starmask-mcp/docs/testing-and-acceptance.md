# Starmask MCP Testing and Acceptance

## Status

This document defines acceptance criteria for the current `v1` extension-backed implementation.

Future multi-backend expansion has additional requirements, but those are not part of the current
release contract unless explicitly called out below.

## 1. Current `v1` Scope

The current acceptance scope covers:

- `starmask-mcp`
- `starmaskd`
- `starmask-native-host`
- Starmask extension

It does not yet cover:

- `local_account_dir`
- `private_key_dev`
- explicit unlock requests

Those are follow-on phases described in `docs/unified-wallet-coordinator-evolution.md`.

## 2. Test Areas

The current release must cover:

1. protocol correctness
2. lifecycle correctness
3. restart and disconnect recovery
4. security behavior
5. approval UI behavior
6. configuration safety

## 3. Rust Test Layers

Recommended Rust test layout for the current implementation:

1. unit tests in `starmask-core` for lifecycle and policy
2. integration tests for daemon JSON-RPC transport
3. integration tests for Native Messaging framing and bridge behavior
4. recovery tests using temporary SQLite databases and restart simulation
5. adapter tests for the MCP shim and error mapping

## 4. Protocol Acceptance

The implementation must demonstrate:

1. `client_request_id` retries return the same `request_id`
2. duplicate `client_request_id` with different payload fails with `idempotency_key_conflict`
3. `wallet_selection_required` is returned when routing is ambiguous
4. `protocol_version_mismatch` is returned for unsupported daemon or native protocol versions

## 5. Lifecycle Acceptance

The implementation must demonstrate:

1. `created -> dispatched -> pending_user_approval -> approved`
2. `created -> dispatched -> pending_user_approval -> rejected`
3. `created -> cancelled`
4. `dispatched -> created` on delivery-lease expiry
5. `pending_user_approval -> cancelled`
6. non-terminal request -> `expired`

## 6. Recovery Acceptance

The implementation must demonstrate:

1. host restart and continued polling by `request_id`
2. daemon restart with a `created` request
3. daemon restart with a `dispatched` request
4. daemon restart with a `pending_user_approval` request
5. browser restart before `request.presented`
6. browser restart after `request.presented` with same-instance resume
7. no cross-instance re-delivery after `request.presented`

## 7. Security Acceptance

The implementation must demonstrate:

1. transaction approval UI renders canonical fields from payload bytes
2. `display_hint` is secondary and not trusted as source of truth
3. unsupported payloads do not permit blind signing
4. signing is impossible when the selected wallet is locked
5. daemon never logs private keys or full signed payloads by default
6. production channel rejects development extension IDs

## 8. Result Handling Acceptance

The implementation must demonstrate:

1. approved results are readable multiple times before `result_expires_at`
2. approved results are no longer readable after retention expiry
3. `wallet_get_request_status` still reports terminal metadata after payload eviction

## 9. UI Acceptance

The extension must demonstrate:

1. transaction approve, reject, cancel, expire, and unsupported states
2. message sign approve and reject states
3. recovery banner on resumed pending request
4. approve button disabled in `loading`, `cancelled`, `expired`, and `unsupported`

## 10. Configuration Acceptance

The implementation must demonstrate:

1. invalid extension ID allowlist fails safely
2. missing native host manifest is diagnosable
3. insecure socket or pipe configuration is rejected or warned loudly
4. unsafe TTL values are clamped

## 11. End-to-End Scenarios

The current release must pass these end-to-end scenarios:

1. sign transaction with one connected wallet instance
2. sign message with one connected wallet instance
3. reject transaction request in UI
4. cancel transaction request while approval UI is open
5. recover pending request after browser restart
6. preserve idempotency on host retry after uncertain create failure

## 12. Current Release Gate

The project is not ready for implementation freeze unless these current-contract documents exist and
remain consistent:

- `docs/starmask-mcp-interface-design.md`
- `docs/daemon-protocol.md`
- `docs/security-model.md`
- `docs/configuration.md`
- `docs/persistence-and-recovery.md`
- `docs/sqlite-schema-and-migrations.md`
- `docs/rmcp-adapter-design.md`

The project is not ready for release unless every acceptance area above has at least one passing
test or one manual verification record.

## 13. Phase 2 Acceptance Additions

Before a multi-backend release can freeze, the project must add new acceptance coverage for:

1. backend-generic registration and routing
2. `local_account_dir` filesystem permission checks
3. backend-local unlock prompts and unlock TTL behavior
4. local-account signing flows
5. protocol-version migration between current `v1` and any future backend-generic protocol

Those additions are gated by `docs/unified-wallet-coordinator-evolution.md`.
