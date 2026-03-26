# Starmask MCP Persistence and Recovery

## 1. Purpose

This document defines:

- what `starmaskd` persists
- how delivery and presentation leases are represented
- how results are retained and evicted
- how recovery works across daemon and backend restarts

The model is backend-generic and applies to both browser and local-account signer backends.

## 2. Design Goals

Persistence must guarantee:

1. no lost request state after daemon restart
2. no duplicate approval prompt on safe retries
3. deterministic recovery after backend disconnect
4. bounded retention of signed outputs and unlock results

## 3. Canonical Storage Model

The first implementation should use one canonical request table with kind-specific payload and
result columns.

This resolves the design in favor of:

- one shared request table for `unlock`, `sign_transaction`, and `sign_message`

## 4. Rust Persistence Guidance

Recommended Rust approach:

1. keep lifecycle logic in a coordinator owned by `starmask-core`
2. expose persistence through repository traits
3. keep SQLite access behind one repository implementation
4. perform lifecycle writes inside explicit SQLite transactions

The first implementation should use `rusqlite` and keep writes on a dedicated blocking path rather
than issuing ad hoc SQL from arbitrary async tasks.

Recommended SQLite settings at startup:

- `journal_mode = WAL`
- `foreign_keys = ON`
- `busy_timeout` configured

## 5. Required Persistent Entities

### 5.1 `requests`

Required fields:

- `request_id`
- `client_request_id`
- `kind`
- `status`
- `wallet_instance_id`
- `account_address`
- `payload_hash`
- `payload_json`
- `result_json`
- `created_at`
- `expires_at`
- `updated_at`
- `approved_at`
- `rejected_at`
- `cancelled_at`
- `failed_at`
- `result_expires_at`
- `last_error_code`
- `last_error_message`
- `delivery_lease_id`
- `delivery_lease_expires_at`
- `presentation_id`
- `presentation_expires_at`

Constraints:

1. `request_id` is globally unique
2. `client_request_id` is unique within the active retention horizon
3. the same `client_request_id` with a mismatched `payload_hash` is invalid

### 5.2 `wallet_instances`

Required fields:

- `wallet_instance_id`
- `backend_kind`
- `transport_kind`
- `approval_surface`
- `protocol_version`
- `label`
- `lock_state`
- `connected`
- `capabilities_json`
- `backend_metadata_json`
- `last_seen_at`

### 5.3 `wallet_accounts`

Required fields:

- `wallet_instance_id`
- `address`
- `label`
- `public_key`
- `is_default`
- `is_read_only`
- `last_seen_at`

## 6. Lease Model

The daemon uses two lease types.

### 6.1 Delivery lease

Purpose:

- protect a request while it is being prepared for presentation

Fields:

- `delivery_lease_id`
- `delivery_lease_expires_at`

Rules:

1. valid only in `dispatched`
2. expires automatically if `request.presented` is not received in time
3. expiry returns the request to `created`

### 6.2 Presentation lease

Purpose:

- keep a presented request recoverable on the same wallet instance

Fields:

- `presentation_id`
- `presentation_expires_at`

Rules:

1. begins at `request.presented`
2. may be extended by backend heartbeats or equivalent reconnect evidence
3. is valid only for the presenting `wallet_instance_id`
4. after `request.presented`, the request is pinned to that instance

## 7. Default Timing Rules

Initial default values:

- `request_ttl_seconds = 300`
- `delivery_lease_seconds = 30`
- `presentation_lease_seconds = 45`
- `result_retention_seconds = 600`
- `terminal_record_retention_seconds = 86400`

Optional unlock defaults:

- `default_unlock_ttl_seconds = 300`
- `max_unlock_ttl_seconds = 1800`

The exact defaults are defined in `configuration.md`.

## 8. Result Retention Policy

The first implementation uses bounded multi-read retention.

Rules:

1. approved results may be read multiple times until `result_expires_at`
2. after `result_expires_at`, result payload bytes are deleted
3. terminal metadata remains until terminal record retention expires
4. after payload eviction, `request.getStatus` still returns:
   - terminal `status`
   - `result_kind`
   - `result_available = false`
   - `error_code = result_unavailable`

This applies equally to:

- `unlock_granted`
- `signed_transaction`
- `signed_message`

## 9. Request Creation Recovery

To avoid duplicate requests during transport uncertainty:

1. the daemon must persist the request before returning success
2. `client_request_id` must be queryable directly
3. if the client retries after a lost transport response, the daemon returns the original request
   when the `client_request_id` and `payload_hash` match
4. if the hashes differ, the daemon returns `idempotency_conflict`

## 10. Backend Disconnect Recovery

The persistence model must tolerate backend disconnects.

Rules:

1. a disconnected wallet instance remains known until cleanup policy removes stale metadata
2. requests already in `pending_user_approval` remain durable
3. only the same `wallet_instance_id` may resume a presented request
4. disconnect alone does not imply rejection
5. non-presented requests may be re-dispatched after delivery lease expiry

## 11. Daemon Restart Recovery

At startup, `starmaskd` should:

1. open the database
2. load all non-terminal requests
3. expire requests whose `expires_at < now`
4. clear stale delivery leases
5. retain presented requests for same-instance recovery
6. mark wallet instances disconnected until they re-register or reconnect
7. resume maintenance sweeps

## 12. Wallet Instance Snapshot Recovery

`wallet_instances` and `wallet_accounts` are snapshots, not long-term source-of-truth wallet state.

Rules:

1. backend re-registration may replace the snapshot for the same `wallet_instance_id`
2. account snapshots are replaced as a unit for one wallet instance
3. stale disconnected wallet instances may remain visible for diagnostics until retention cleanup
4. routing to disconnected instances must still fail with `wallet_unavailable`

## 13. Maintenance Tasks

The daemon should perform bounded incremental maintenance:

1. expire requests whose request TTL elapsed
2. release expired delivery leases
3. expire presented requests whose presentation lease elapsed
4. evict result payloads whose retention elapsed
5. delete terminal records older than terminal retention
6. mark wallet instances offline when heartbeat or reconnect deadlines are missed

Maintenance should avoid full-table scans where indexes can provide a bounded cursor.

## 14. Recovery Guarantees by Status

### `created`

- safe to re-dispatch after restart

### `dispatched`

- safe to return to `created` if the delivery lease expired

### `pending_user_approval`

- pinned to the same wallet instance
- recoverable if that same instance reconnects before lease expiry

### `approved`, `rejected`, `cancelled`, `expired`, `failed`

- terminal metadata remains until retention cleanup
- result payload remains only until result-retention expiry when applicable

## 15. Performance Considerations

Persistence should optimize for correctness first, but local interactive workloads still need
predictable latency.

Recommended rules:

1. use WAL mode
2. keep request creation and polling to small indexed transactions
3. avoid storing duplicate account snapshots unnecessarily
4. batch maintenance deletes when pruning old rows
5. keep payload and result JSON encoding owned by the repository layer

## 16. Closed First-Release Decisions

The first implementation is closed on these decisions:

1. one shared request table covers unlock and both signing flows
2. result retention is bounded multi-read
3. after `request.presented`, only the same wallet instance may resume the request
4. disconnected wallet instances remain visible for diagnostics until cleanup, but cannot satisfy
   routing
