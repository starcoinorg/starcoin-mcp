# Starmask MCP Testing and Acceptance

## Purpose

This document defines the minimum acceptance criteria before `starmask-mcp` can be considered implementation-ready and release-ready.

## Test Areas

The first release must cover:

1. protocol correctness
2. lifecycle correctness
3. restart and disconnect recovery
4. security behavior
5. approval UI behavior
6. configuration safety

## Protocol Acceptance

The implementation must demonstrate:

1. `client_request_id` retries return the same `request_id`
2. duplicate `client_request_id` with different payload fails with `idempotency_key_conflict`
3. `wallet_selection_required` is returned when routing is ambiguous
4. `protocol_version_mismatch` is returned for unsupported daemon or native protocol versions

## Lifecycle Acceptance

The implementation must demonstrate these flows:

1. `created -> dispatched -> pending_user_approval -> approved`
2. `created -> dispatched -> pending_user_approval -> rejected`
3. `created -> cancelled`
4. `dispatched -> created` on delivery lease expiry
5. `pending_user_approval -> cancelled`
6. non-terminal request -> `expired`

## Recovery Acceptance

The implementation must demonstrate:

1. host restart and successful continued polling by `request_id`
2. daemon restart with a `created` request
3. daemon restart with a `dispatched` request
4. daemon restart with a `pending_user_approval` request
5. browser restart before `request.presented`
6. browser restart after `request.presented` and same-instance resume
7. no cross-instance re-delivery after `request.presented`

## Security Acceptance

The implementation must demonstrate:

1. transaction approval UI renders canonical fields from payload bytes
2. `display_hint` is secondary and not trusted as source of truth
3. unsupported payloads do not permit blind signing
4. signing is impossible when the selected wallet is locked
5. daemon never logs private keys or full signed payloads by default
6. production channel rejects development extension IDs

## Result Handling Acceptance

The implementation must demonstrate:

1. approved results are readable multiple times before `result_expires_at`
2. approved results are no longer readable after retention expiry
3. `wallet_get_request_status` still reports terminal metadata after payload eviction

## UI Acceptance

The extension must demonstrate:

1. transaction approve, reject, cancel, expire, and unsupported states
2. message sign approve and reject states
3. recovery banner on resumed pending request
4. approve button disabled in `loading`, `cancelled`, `expired`, and `unsupported`

## Configuration Acceptance

The implementation must demonstrate:

1. invalid extension ID allowlist fails safely
2. missing native host manifest is diagnosable
3. insecure socket or pipe configuration is rejected or warned loudly
4. unsafe TTL values are clamped

## End-to-End Scenarios

The first release must pass these end-to-end scenarios:

1. sign transaction with one connected wallet instance
2. sign message with one connected wallet instance
3. reject transaction request in UI
4. cancel transaction request while approval UI is open
5. recover pending request after browser restart
6. preserve idempotency on host retry after uncertain create failure

## Release Gate

The project is not ready for implementation freeze unless every document in:

- `docs/architecture/design-closure-plan.md`

exists and the first-release policy decisions in that document remain unchanged.

The project is not ready for release unless every acceptance area above has at least one passing test or manual verification record.
