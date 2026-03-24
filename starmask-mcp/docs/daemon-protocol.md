# Starmaskd Daemon Protocol

## Purpose

This document defines the RPC contract between:

- `starmask-mcp`
- `starmaskd`

The protocol is local-only and versioned independently from MCP itself.

## Goals

The daemon protocol must provide:

1. deterministic request creation
2. safe retry behavior
3. explicit wallet routing
4. durable status polling
5. no direct signing capability outside the extension

## Transport

The daemon protocol uses JSON-RPC 2.0 over:

- Unix domain socket on macOS and Linux
- named pipe on Windows

The daemon must reject non-local access.

The first implementation may use one request per local connection:

1. the client opens a local socket or pipe connection
2. the client writes one JSON-RPC request
3. the daemon writes one JSON-RPC response
4. the connection closes

Framing rule for this mode:

- request body is complete when the client closes its write side (EOF)
- daemon returns exactly one JSON-RPC response, then closes the connection

Persistent local connections may be added later without changing the request and response envelope.

## Protocol Version

Initial daemon protocol version:

- `1`

Every client request must include:

- `protocol_version`

If the version is unsupported, the daemon must return:

- `protocol_version_mismatch`

## Envelope

Every JSON-RPC request should follow this shape:

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-123",
  "method": "request.createSignTransaction",
  "params": {
    "protocol_version": 1
  }
}
```

Every error response should contain a shared code where applicable:

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-123",
  "error": {
    "code": "wallet_selection_required",
    "message": "Multiple wallet instances expose the requested account.",
    "retryable": true
  }
}
```

## Rust Implementation Guidance

Recommended Rust boundary model:

1. parse JSON-RPC messages into DTOs with `serde`
2. convert DTOs into typed domain commands
3. send those commands to one daemon coordinator task
4. let the coordinator own lifecycle transitions and persistence

The daemon should not let arbitrary transport tasks mutate request state directly.

Recommended crates:

- `serde`
- `serde_json`
- Tokio for local IPC transport
- `thiserror` for typed library errors

Boundary rule:

- this daemon protocol remains project-owned even if `starmask-mcp` uses `rmcp`
- `rmcp` should not leak into daemon-facing Rust core crates

## System Methods

### `system.ping`

Purpose:

- check daemon reachability

Params:

- `protocol_version`

Result:

- `ok`
- `daemon_protocol_version`
- `daemon_version`

### `system.getInfo`

Purpose:

- expose local daemon metadata useful for diagnostics

Params:

- `protocol_version`

Result:

- `daemon_protocol_version`
- `daemon_version`
- `socket_scope`
- `db_schema_version`
- `result_retention_seconds`
- `default_request_ttl_seconds`

## Wallet Methods

### `wallet.status`

Purpose:

- return current wallet availability

Params:

- `protocol_version`

Result:

- `wallet_available`
- `wallet_online`
- `default_wallet_instance_id`
- `wallet_instances`

### `wallet.listInstances`

Purpose:

- return known wallet instances

Params:

- `protocol_version`
- `connected_only`: boolean, default `false`

Result:

- `wallet_instances`
  - `wallet_instance_id`
  - `extension_connected`
  - `lock_state`
  - `profile_hint`
  - `last_seen_at`

### `wallet.listAccounts`

Purpose:

- list visible wallet accounts

Policy:

- the first release does not require an interactive approval gate for account listing
- account listing remains a local same-user capability, not a signing capability
- public keys may be returned only when known and requested

Params:

- `protocol_version`
- `wallet_instance_id`: optional
- `include_public_key`: boolean, default `false`

Result:

- `wallet_instances`
  - `wallet_instance_id`
  - `extension_connected`
  - `lock_state`
  - `accounts`
    - `address`
    - `label`
    - `public_key`: optional
    - `is_default`
    - `is_locked`

### `wallet.getPublicKey`

Purpose:

- return the public key for a known account

Params:

- `protocol_version`
- `address`
- `wallet_instance_id`: optional

Resolution rules:

1. if `wallet_instance_id` is provided, the daemon must route only to that instance
2. if `wallet_instance_id` is omitted and exactly one known instance exposes the account, the daemon may auto-select
3. otherwise the daemon must fail with `wallet_selection_required`

Lookup rules:

1. if a cached public key is available for the selected account, the daemon may return it immediately
2. if no cached public key exists and the wallet is locked, the daemon must fail with `wallet_locked`
3. if no cached public key exists and the account is unknown, the daemon must fail with `invalid_account`

Result:

- `wallet_instance_id`
- `address`
- `public_key`
- `curve`

## Request Creation Methods

Request creation is asynchronous but must be safe to retry.

### Idempotency Rule

Both request-creation methods require:

- `client_request_id`

The daemon must enforce:

1. same `client_request_id` plus same `payload_hash` returns the existing request
2. same `client_request_id` plus different `payload_hash` fails with `idempotency_key_conflict`
3. retries must not create duplicate approval prompts

### `request.createSignTransaction`

Purpose:

- create a new asynchronous transaction-signing request

Params:

- `protocol_version`
- `client_request_id`
- `account_address`
- `wallet_instance_id`: optional but strongly recommended
- `chain_id`
- `raw_txn_bcs_hex`
- `tx_kind`
- `display_hint`: optional
- `client_context`: optional
- `ttl_seconds`: optional

Creation policy:

1. the daemon must resolve the wallet instance before creating the request
2. the selected wallet instance must be connected
3. the selected wallet instance must be unlocked
4. the first release fails fast if no connected unlocked wallet instance can satisfy the request
5. the first release does not queue signing requests for future wallet availability

Possible shared errors:

- `wallet_selection_required`
- `wallet_instance_not_found`
- `wallet_unavailable`
- `wallet_locked`
- `invalid_account`
- `invalid_transaction_payload`
- `unsupported_chain`
- `idempotency_key_conflict`

Result:

- `request_id`
- `client_request_id`
- `kind`
- `status`
- `wallet_instance_id`
- `created_at`
- `expires_at`

### `request.createSignMessage`

Purpose:

- create a new asynchronous message-signing request

Params:

- `protocol_version`
- `client_request_id`
- `account_address`
- `wallet_instance_id`: optional but strongly recommended
- `message`
- `format`
- `display_hint`: optional
- `client_context`: optional
- `ttl_seconds`: optional

Creation policy:

- same as `request.createSignTransaction`

Result:

- `request_id`
- `client_request_id`
- `kind`
- `status`
- `wallet_instance_id`
- `created_at`
- `expires_at`

## Request Query Methods

### `request.getStatus`

Purpose:

- return the canonical lifecycle state of a known request

Params:

- `protocol_version`
- `request_id`

Result:

- `request_id`
- `client_request_id`
- `kind`
- `status`
- `wallet_instance_id`
- `updated_at`
- `result_kind`
- `result_available`
- `result_expires_at`
- `error_code`: optional shared code explaining a blocking or terminal condition
- `reason`: optional human-readable text
- `signed_txn_bcs_hex`: only when available
- `signature`: only when available

Rules:

1. approved results are readable multiple times during the retention window
2. after `result_expires_at`, the request remains terminal but the payload result may be omitted
3. when the result payload has been evicted, `result_available` must be `false`
4. when the result payload has been evicted, `error_code` should be `result_unavailable`

### `request.cancel`

Purpose:

- cancel a non-terminal request

Params:

- `protocol_version`
- `request_id`

Result:

- `request_id`
- `status`
- `cancelled`

Rules:

1. `created`, `dispatched`, and `pending_user_approval` may be cancelled
2. cancelling a terminal request is a no-op with `cancelled = false`
3. if the request was already approved, the daemon must not revoke the signature result

## Status Ownership

The daemon is the single owner of request lifecycle state.

The MCP shim:

- validates inputs
- forwards calls
- does not synthesize lifecycle transitions

## Retry Guidance

Clients should:

1. reuse `client_request_id` when retrying a create call after an uncertain transport failure
2. poll by `request_id` after a successful create response
3. avoid creating a second request unless the first request reached a terminal state

## Non-Goals

The daemon protocol does not define:

- the extension wire format
- the browser approval UI
- the on-disk storage schema

Those are defined in:

- `native-messaging-contract.md`
- `approval-ui-spec.md`
- `persistence-and-recovery.md`
