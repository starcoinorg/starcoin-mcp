# Starmaskd Daemon Protocol

## Status

This document is the authoritative client-facing daemon protocol contract for the current runtime.

It matches the stable JSON-RPC methods used by `starmask-native-host` and any external wallet or
MCP adapter, where `DAEMON_PROTOCOL_VERSION = 1` in `starmask-types`.

Repository status note: the in-tree `crates/starmask-mcp` adapter has been removed.

The daemon also implements the generic backend-agent binding described in
`docs/wallet-backend-local-socket-binding.md`. Those backend methods use
`GENERIC_BACKEND_PROTOCOL_VERSION = 2` and are referenced here only where they affect
client-visible routing.

## 1. Purpose

This document defines the local RPC contract used by:

- `starmask-mcp`
- `starmask-native-host`
- `starmaskd`

The protocol is local-only and versioned independently from MCP itself.

## 2. Goals

The daemon protocol must provide:

1. deterministic request creation
2. safe retry behavior
3. explicit wallet routing
4. durable status polling
5. no direct signing capability outside the extension

## 3. Transport

The daemon protocol uses JSON-RPC 2.0 over:

- Unix domain socket on macOS and Linux
- named pipe on Windows

The daemon must reject non-local access.

The current implementation may use one request per local connection:

1. the client opens a local socket or pipe connection
2. the client writes one JSON-RPC request
3. the daemon writes one JSON-RPC response
4. the connection closes

Persistent local connections may be added later without changing the request or response envelope.

## 4. Protocol Version

Current client-facing daemon protocol version:

- `1`

Generic backend-agent methods use:

- `2`

Every client request must include:

- `protocol_version`

If the version is unsupported, the daemon returns:

- `protocol_version_mismatch`

## 5. Envelope

Every JSON-RPC request follows this shape:

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

Every error response contains a shared code where applicable:

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

## 6. System Methods

### `system.ping`

Result:

- `ok`
- `daemon_protocol_version`
- `daemon_version`

### `system.getInfo`

Result:

- `daemon_protocol_version`
- `daemon_version`
- `socket_scope`
- `db_schema_version`
- `result_retention_seconds`
- `default_request_ttl_seconds`

## 7. Wallet Methods

### `wallet.status`

Result:

- `wallet_available`
- `wallet_online`
- `default_wallet_instance_id`
- `wallet_instances`

### `wallet.listInstances`

Params:

- `connected_only`: boolean, default `false`

Result fields per instance:

- `wallet_instance_id`
- `extension_connected`
- `lock_state`
- `profile_hint`
- `last_seen_at`

`extension_connected` is the legacy field name retained for compatibility. It indicates whether the
wallet instance is currently connected to `starmaskd`, including generic backend agents.

### `wallet.listAccounts`

Params:

- `wallet_instance_id`: optional
- `include_public_key`: boolean, default `false`

Result fields per wallet group:

- `wallet_instance_id`
- `extension_connected`
- `lock_state`
- `accounts`
  - `address`
  - `label`
  - `public_key`
  - `is_default`
  - `is_locked`

### `wallet.getPublicKey`

Params:

- `address`
- `wallet_instance_id`: optional

Result:

- `wallet_instance_id`
- `address`
- `public_key`
- `curve`

Resolution rules:

1. if `wallet_instance_id` is provided, the daemon routes only to that instance
2. if `wallet_instance_id` is omitted and exactly one instance exposes the account, the daemon may
   auto-select
3. otherwise the daemon fails with `wallet_selection_required`

## 8. Request Creation Methods

### Idempotency rule

Both request-creation methods require:

- `client_request_id`

The daemon must enforce:

1. replaying the same `client_request_id` with the same payload returns the original request
2. replaying the same `client_request_id` with a different payload fails with
   `idempotency_key_conflict`

### `request.createSignTransaction`

Params:

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

### `request.createSignMessage`

Params:

- `client_request_id`
- `account_address`
- `wallet_instance_id`: optional
- `message`
- `format`
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

### `request.getStatus`

Params:

- `request_id`

Result:

- `request_id`
- `client_request_id`
- `kind`
- `status`
- `wallet_instance_id`
- `created_at`
- `expires_at`
- `result_kind`
- `result_available`
- `result_expires_at`
- `error_code`
- `error_message`
- `result`

### `request.cancel`

Params:

- `request_id`

Result:

- `request_id`
- `status`
- `error_code`

### `request.hasAvailable`

Purpose:

- let the Native Messaging bridge cheaply ask whether work exists for one wallet instance

Params:

- `wallet_instance_id`

Result:

- `wallet_instance_id`
- `available`

## 9. Extension Session Methods

These methods are used by `starmask-native-host` on behalf of the extension.

### `extension.register`

Params:

- `wallet_instance_id`
- `extension_id`
- `extension_version`
- `profile_hint`
- `lock_state`
- `accounts_summary`

Result:

- `wallet_instance_id`
- `daemon_protocol_version`
- `accepted`

### `extension.heartbeat`

Params:

- `wallet_instance_id`
- `presented_request_ids`

Result:

- `ok`

### `extension.updateAccounts`

Params:

- `wallet_instance_id`
- `lock_state`
- `accounts`

Result:

- `ok`

### `request.pullNext`

Params:

- `wallet_instance_id`

Result:

- `request.next`
- `request.none`

### `request.presented`

### `request.resolve`

### `request.reject`

These methods drive the extension-side approval lifecycle and are further constrained by
`docs/native-messaging-contract.md`.

## 10. Routing and Failure Rules

1. if `wallet_instance_id` is supplied, only that instance may satisfy the request
2. if `wallet_instance_id` is omitted and exactly one wallet instance exposes the account, the
   daemon may auto-route
3. if multiple wallet instances match, the daemon must fail with `wallet_selection_required`
4. if the target wallet is offline, the daemon returns `wallet_unavailable`
5. if the target wallet is locked and cannot perform backend-local unlock for the requested signing
   flow, the daemon returns `wallet_locked`
6. if the target wallet is locked but advertises backend-local `unlock` capability, the daemon may
   still create the signing request and the backend performs approval and password entry locally

## 11. Error Codes

The current protocol preserves shared error codes such as:

- `protocol_version_mismatch`
- `wallet_selection_required`
- `wallet_unavailable`
- `wallet_locked`
- `invalid_account`
- `request_not_found`
- `result_unavailable`
- `idempotency_key_conflict`
- `unsupported_operation`

Transport failures remain transport failures and must not be projected as fake request states.

## 12. Deliberate `v1` Omissions

This client-facing `v1` surface still does not define:

- `request.createUnlock`
- any password-bearing daemon method
- backend-kind metadata in `wallet.status` or `wallet.listInstances`

Generic backend registration and backend-agent request lifecycle methods are defined separately in
`docs/wallet-backend-local-socket-binding.md`.
