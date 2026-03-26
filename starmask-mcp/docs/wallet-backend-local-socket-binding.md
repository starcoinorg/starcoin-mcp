# Starmask Wallet Backend Local-Socket Binding

## Status

This document is the phase-2 implementation contract for the first generic backend-agent transport
binding.

It now describes the normative local-socket path used by the current `local_account_dir`
implementation.

## 1. Purpose

This document defines the concrete wire binding between `starmaskd` and a generic wallet backend
agent over local OS IPC.

It is the first concrete transport binding of:

- `docs/wallet-backend-agent-contract.md`

## 2. Binding Summary

Phase-2 local backends use:

- JSON-RPC 2.0
- Unix domain socket on macOS and Linux
- named pipe on Windows
- the same local daemon listener model as the current daemon protocol

Roles:

- `starmaskd` is the server
- the wallet backend agent is the client

The initial binding is deliberately conservative:

1. one request per local connection is sufficient
2. persistent connections are optional
3. server-push hints are not required for correctness
4. backend agents must poll after startup, after reconnect, and after each completed request

## 3. Scope

This binding is required for:

- `local_account_dir`

It may later be reused for:

- other local prompt-based backends
- development-only non-extension backends

It does not replace the current extension binding:

- `starmask_extension` continues to use `docs/native-messaging-contract.md`

## 4. Versioning and Compatibility

The first generic backend binding ships as part of daemon protocol `v2`.

Phase-2 decisions:

1. generic backend methods require `protocol_version = 2`
2. no additional wire-level backend-contract version field is introduced in phase 2
3. the first published generic backend contract is therefore identified by daemon protocol `v2`
4. `starmaskd` may support both extension-backed `v1` methods and generic `v2` methods on the same
   socket or pipe during migration

This keeps the first rollout simpler:

- one listener
- one JSON-RPC envelope
- one compatibility gate

## 5. Connection Model

The connection model is local and stateless.

Rules:

1. the backend agent opens a local socket or pipe connection
2. the backend agent writes one JSON-RPC request
3. `starmaskd` writes one JSON-RPC response
4. the connection closes

The implementation may later add persistent connections, but phase 2 must not require them.

## 6. Backend Identity Model

For local-socket backends, identity is configuration-backed.

Rules:

1. every enabled backend entry has one stable `backend_id`
2. for phase-2 local-socket agents, `wallet_instance_id` must equal configured `backend_id`
3. `starmaskd` rejects registration for unknown `backend_id` values
4. `starmaskd` rejects registration when configured `backend_kind` and presented `backend_kind` do
   not match
5. same-instance recovery uses this stable `wallet_instance_id`

This avoids ambiguous runtime identities and makes restart recovery deterministic.

## 7. Method Namespace

Phase-2 generic backend methods are:

- `backend.register`
- `backend.heartbeat`
- `backend.updateAccounts`
- `request.pullNext`
- `request.presented`
- `request.resolve`
- `request.reject`

`request.hasAvailable` remains optional and is not required in the initial local-socket binding.

The binding does not depend on server-push notifications such as:

- `request.available`
- `request.cancelled`

Those may be added later, but the backend agent must remain correct without them.

## 8. JSON-RPC Envelope

Every request uses JSON-RPC 2.0 with a daemon `protocol_version`.

Example:

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-1",
  "method": "backend.register",
  "params": {
    "protocol_version": 2,
    "wallet_instance_id": "local-main",
    "backend_kind": "local_account_dir",
    "transport_kind": "local_socket",
    "approval_surface": "tty_prompt",
    "instance_label": "Local Main",
    "lock_state": "locked",
    "capabilities": ["unlock", "get_public_key", "sign_message", "sign_transaction"],
    "backend_metadata": {
      "account_provider_kind": "local",
      "prompt_mode": "tty_prompt"
    },
    "accounts": []
  }
}
```

Successful registration response:

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-1",
  "result": {
    "accepted": true,
    "daemon_protocol_version": 2,
    "wallet_instance_id": "local-main"
  }
}
```

## 9. Method Semantics

### 9.1 `backend.register`

Required params:

- `protocol_version`
- `wallet_instance_id`
- `backend_kind`
- `transport_kind`
- `approval_surface`
- `instance_label`
- `lock_state`
- `capabilities`
- `backend_metadata`
- `accounts`

Coordinator rules:

1. `protocol_version` must be `2`
2. `transport_kind` must be `local_socket`
3. `wallet_instance_id` must resolve to one configured backend entry
4. `backend_kind` must match configured policy
5. channel policy must allow the backend kind
6. the supplied capability set must be a subset of the allowed capabilities for that backend kind
7. the account snapshot replaces the old snapshot atomically

### 9.2 `backend.heartbeat`

Required params:

- `protocol_version`
- `wallet_instance_id`

Optional params:

- `presented_request_ids`
- `lock_state`

Coordinator rules:

1. update `last_seen_at`
2. extend any valid presented-request recovery owned by this instance
3. update `lock_state` when supplied

### 9.3 `backend.updateAccounts`

Required params:

- `protocol_version`
- `wallet_instance_id`
- `lock_state`
- `accounts`

Coordinator rules:

1. replace all stored account rows for that wallet instance in one transaction
2. update routing eligibility immediately after commit
3. preserve same-instance recovery for already presented requests

### 9.4 `request.pullNext`

Required params:

- `protocol_version`
- `wallet_instance_id`

Rules:

1. the coordinator returns only work eligible for that exact `wallet_instance_id`
2. a first claim returns a delivery lease
3. a resumed request returns a presentation context
4. no other wallet instance may resume a presented request

Phase-2 local agents must call `request.pullNext`:

1. immediately after successful registration
2. after reconnect
3. after every `request.resolve` or `request.reject`
4. after any local prompt flow ends without a terminal daemon update

### 9.5 `request.presented`

Required params:

- `protocol_version`
- `wallet_instance_id`
- `request_id`
- `presentation_id`

Required on first presentation:

- `delivery_lease_id`

Rules:

1. the backend agent sends this only after the local prompt is actually visible and actionable
2. the daemon pins the request to that wallet instance for the rest of the presentation lifecycle

### 9.6 `request.resolve`

Required params:

- `protocol_version`
- `wallet_instance_id`
- `request_id`
- `presentation_id`
- `result_kind`

Result payload rules:

- `signed_transaction` returns `signed_txn_bcs_hex`
- `signed_message` returns `signature`

### 9.7 `request.reject`

Required params:

- `protocol_version`
- `wallet_instance_id`
- `request_id`
- `reason_code`

Optional params:

- `presentation_id`
- `reason_message`

## 10. Shared Failure Codes

Method-level JSON-RPC failures should use shared daemon error codes where possible.

Phase-2 required codes:

- `protocol_version_mismatch`
- `backend_not_allowed`
- `invalid_backend_registration`
- `request_not_found`
- `request_not_owned`
- `lease_mismatch`
- `wallet_locked`
- `unsupported_operation`

Phase-2 shared `request.reject` reason codes are:

- `request_rejected`
- `unsupported_operation`
- `invalid_transaction_payload`
- `invalid_message_payload`
- `wallet_locked`
- `backend_unavailable`
- `backend_policy_blocked`

These codes are the common cross-backend refusal vocabulary for phase 2.

## 11. Polling and Performance Rules

The local-socket binding must stay lightweight enough for interactive local use.

Required rules:

1. `request.hasAvailable` stays optional
2. the backend agent must not busy-loop on `request.pullNext`
3. idle polling must not be more frequent than the configured heartbeat interval
4. account snapshots should be pushed only when changed
5. only one in-flight approval prompt should exist per `wallet_instance_id` unless the backend kind
   explicitly declares safe local concurrency

The phase-2 default is conservative:

- one active prompt per `wallet_instance_id`

## 12. Security Binding Rules

The transport binding must enforce:

1. local-only OS IPC
2. same-user access boundaries
3. no TCP or localhost HTTP fallback
4. no password or raw key material in JSON-RPC payloads
5. canonical payload bytes as the source of truth for local approval rendering

`wallet_instance_id` registration must be validated against local configuration before the backend is
treated as routable.

## 13. Local Account Backend Notes

For `local_account_dir`, the concrete agent should:

1. read one configured backend entry by `backend_id`
2. register with `wallet_instance_id = backend_id`
3. expose accounts from Starcoin `AccountProvider`
4. keep unlock and password entry fully inside the agent process
5. in the current implementation, use `tty_prompt`
6. if the target account is locked, perform any password entry only inside the local prompt flow
   and reject with `wallet_locked` when unlock fails

## 14. Relationship to Other Documents

This document should be read with:

- `docs/wallet-backend-agent-contract.md`
- `docs/wallet-backend-configuration.md`
- `docs/wallet-backend-security-model.md`
- `docs/wallet-backend-persistence-and-schema.md`
