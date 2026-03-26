# Starmaskd Daemon Protocol

## 1. Purpose

This document defines the local RPC contract between:

- `starmask-mcp`
- `starmaskd`

The protocol is local-only and versioned independently from MCP itself.

## 2. Goals

The daemon protocol must provide:

1. deterministic request creation
2. safe retry behavior
3. explicit wallet routing
4. durable status polling
5. no direct signing capability outside registered backends
6. one stable contract across browser and local-account backends

## 3. Transport

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

- request body is complete when the client closes its write side
- daemon returns exactly one JSON-RPC response, then closes the connection

Persistent local connections may be added later without changing the request and response envelope.

## 4. Protocol Version

Initial daemon protocol version:

- `2`

Every client request must include:

- `protocol_version`

If the version is unsupported, the daemon must return:

- `protocol_version_mismatch`

## 5. Envelope

Every JSON-RPC request should follow this shape:

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-123",
  "method": "request.createSignTransaction",
  "params": {
    "protocol_version": 2
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

## 6. Rust Implementation Guidance

Recommended Rust boundary model:

1. parse JSON-RPC messages into DTOs with `serde`
2. convert DTOs into typed domain commands
3. send those commands to one coordinator service
4. let the coordinator own lifecycle transitions and persistence

The daemon must not let transport tasks mutate request state directly.

Boundary rules:

- this daemon protocol remains project-owned even if `starmask-mcp` uses `rmcp`
- backend-specific transport logic remains outside the daemon client contract

## 7. System Methods

### 7.1 `system.ping`

Purpose:

- check daemon reachability

Params:

- `protocol_version`

Result:

- `ok`
- `daemon_protocol_version`
- `daemon_version`

### 7.2 `system.getInfo`

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
- `enabled_backend_kinds`

## 8. Wallet Methods

### 8.1 `wallet.status`

Purpose:

- return coordinator availability and wallet-instance summaries

Params:

- `protocol_version`

Result:

- `wallet_available`
- `default_wallet_instance_id`
- `wallet_instances`
  - `wallet_instance_id`
  - `backend_kind`
  - `transport_kind`
  - `approval_surface`
  - `connected`
  - `lock_state`
  - `capabilities`
  - `accounts_count`
  - `label`

### 8.2 `wallet.listInstances`

Purpose:

- return known wallet instances

Params:

- `protocol_version`
- `connected_only`: boolean, default `false`

Result:

- `wallet_instances`

### 8.3 `wallet.listAccounts`

Purpose:

- return visible accounts across one or more wallet instances

Params:

- `protocol_version`
- `wallet_instance_id`: optional
- `include_public_key`: boolean, default `false`

Result:

- `wallet_instances`
  - `wallet_instance_id`
  - `backend_kind`
  - `accounts`
    - `address`
    - `label`
    - `public_key`
    - `is_default`
    - `is_read_only`

### 8.4 `wallet.getPublicKey`

Purpose:

- return the public key for a known account

Params:

- `protocol_version`
- `address`
- `wallet_instance_id`: optional

Result:

- `wallet_instance_id`
- `backend_kind`
- `address`
- `public_key`
- `curve`

## 9. Request Methods

### 9.1 `request.createUnlock`

Purpose:

- create an unlock request for a backend that supports local unlock

Params:

- `protocol_version`
- `client_request_id`
- `wallet_instance_id`
- `account_address`: optional
- `ttl_seconds`: optional
- `client_context`: optional

Result:

- `request_id`
- `client_request_id`
- `kind`
- `status`
- `wallet_instance_id`
- `created_at`
- `expires_at`

Rules:

- passwords must not appear in params
- backends without `unlock` capability return `unsupported_operation`

### 9.2 `request.createSignTransaction`

Purpose:

- create an asynchronous transaction-signing request

Params:

- `protocol_version`
- `client_request_id`
- `account_address`
- `wallet_instance_id`: optional
- `chain_id`
- `raw_txn_bcs_hex`
- `tx_kind`
- `display_hint`: optional
- `client_context`: optional
- `ttl_seconds`: optional

Result:

- `request_id`
- `client_request_id`
- `kind`
- `status`
- `wallet_instance_id`
- `created_at`
- `expires_at`

### 9.3 `request.createSignMessage`

Purpose:

- create an asynchronous message-signing request

Params:

- `protocol_version`
- `client_request_id`
- `account_address`
- `wallet_instance_id`: optional
- `message_format`
- `message`
- `display_hint`: optional
- `client_context`: optional
- `ttl_seconds`: optional

Result:

- `request_id`
- `client_request_id`
- `kind`
- `status`
- `wallet_instance_id`
- `created_at`
- `expires_at`

### 9.4 `request.getStatus`

Purpose:

- return the current request lifecycle state and any bounded retained result

Params:

- `protocol_version`
- `request_id`

Result:

- `request_id`
- `client_request_id`
- `kind`
- `status`
- `wallet_instance_id`
- `backend_kind`
- `updated_at`
- `result_kind`
- `result_available`
- `result_expires_at`
- `error_code`
- `reason`
- `unlock_expires_at`: only for `unlock_granted`
- `signed_txn_bcs_hex`: only for `signed_transaction`
- `signature`: only for `signed_message`

### 9.5 `request.cancel`

Purpose:

- cancel a non-terminal request

Params:

- `protocol_version`
- `request_id`

Result:

- `request_id`
- `status`
- `cancelled_at`

## 10. Routing Rules

1. If `wallet_instance_id` is supplied, only that wallet instance may satisfy the request.
2. If `wallet_instance_id` is omitted and exactly one wallet instance exposes the target account
   and capability, the daemon may auto-route.
3. If multiple wallet instances match, the daemon must fail with `wallet_selection_required`.
4. A backend without the requested capability must never be auto-selected.
5. If the target backend is offline, return `wallet_unavailable`.
6. If the target backend is locked for a sign request, return `wallet_locked`.

## 11. Lifecycle Rules

The daemon owns canonical lifecycle state.

Supported statuses:

- `created`
- `dispatched`
- `pending_user_approval`
- `approved`
- `rejected`
- `cancelled`
- `expired`
- `failed`

Rules:

1. request creation is idempotent through `client_request_id`
2. conflicting payloads for the same `client_request_id` fail with `idempotency_conflict`
3. `request.getStatus` is the only RPC for reading retained results
4. cancellation is best-effort until the request reaches a terminal state

## 12. Error Codes

The daemon protocol should preserve shared error codes such as:

- `protocol_version_mismatch`
- `wallet_selection_required`
- `wallet_unavailable`
- `wallet_locked`
- `unsupported_operation`
- `invalid_request`
- `idempotency_conflict`
- `result_unavailable`

Transport failures should remain transport failures and must not be projected as fake request
statuses.

## 13. Security Rules

The daemon protocol must never carry:

- private keys
- account passwords
- backend-local unlock tokens

The daemon may carry:

- public keys
- signed transaction bytes
- message signatures

All of those remain subject to bounded retention and log redaction.

## 14. Ready-to-Implement Checklist

This protocol is implementation-ready when:

1. JSON-RPC DTOs exist for every method above
2. error mapping preserves shared error codes
3. idempotent request creation is tested
4. unlock, sign-transaction, sign-message, and status polling paths are all covered
