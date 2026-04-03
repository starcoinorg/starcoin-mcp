# Starmask MCP Persistence and Recovery

## Status

This document is the authoritative `v1` persistence and recovery design for the current
extension-backed implementation.

Repository status note: the in-tree `crates/starmask-mcp` adapter has been removed. Persistence and
recovery rules remain current for `starmaskd` and the remaining daemon-side components.

Future generic wallet-backend persistence changes are tracked in:

- `docs/unified-wallet-coordinator-evolution.md`
- `docs/wallet-backend-persistence-and-schema.md`

## 1. Purpose

This document defines:

- what `starmaskd` persists
- how leases are represented
- how results are retained and evicted
- how recovery works across restarts and disconnects

## 2. Design Goals

Persistence must guarantee:

1. no lost request state after daemon restart
2. no duplicate approval prompt on safe retries
3. deterministic recovery after browser or extension disconnect
4. bounded retention of signed outputs

## 3. Canonical Storage Model

The current implementation uses one canonical request table with kind-specific payload and result
columns.

Current request kinds are:

- `sign_transaction`
- `sign_message`

## 4. Required Persistent Entities

### `requests`

Current fields:

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
- `reject_reason_code`
- `delivery_lease_id`
- `delivery_lease_expires_at`
- `presentation_id`
- `presentation_expires_at`

### `wallet_instances`

Current fields:

- `wallet_instance_id`
- `extension_id`
- `extension_version`
- `protocol_version`
- `profile_hint`
- `lock_state`
- `connected`
- `last_seen_at`

### `wallet_accounts`

Current fields:

- `wallet_instance_id`
- `address`
- `label`
- `public_key`
- `is_default`
- `is_locked`
- `last_seen_at`

## 5. Lease Model

The current daemon uses two lease types.

### Delivery lease

Purpose:

- protect a request while it is being prepared for presentation

Rules:

1. valid only in `dispatched`
2. expires automatically if `request.presented` is not received in time
3. expiry returns the request to `created`

### Presentation lease

Purpose:

- keep a presented request recoverable on the same wallet instance

Rules:

1. begins at `request.presented`
2. is extended by extension heartbeat
3. is valid only for the presenting `wallet_instance_id`
4. after `request.presented`, the request is pinned to that wallet instance

## 6. Current Timing Rules

Current defaults:

- `request_ttl_seconds = 300`
- `delivery_lease_seconds = 30`
- `presentation_lease_seconds = 45`
- `heartbeat_interval_seconds = 10`
- `wallet_offline_after_seconds = 25`
- `result_retention_seconds = 600`

## 7. Result Retention Policy

The current implementation uses bounded multi-read retention.

Rules:

1. approved results may be read multiple times until `result_expires_at`
2. after `result_expires_at`, result payload bytes are deleted
3. terminal metadata remains queryable after payload eviction
4. after payload eviction, `request.getStatus` still reports terminal status with
   `result_available = false`

## 8. Request Creation Recovery

To avoid duplicate requests during transport uncertainty:

1. the daemon must persist the request before returning success
2. `client_request_id` must be queryable directly
3. if the client retries after a lost transport response, the daemon returns the original request
   when `client_request_id` and `payload_hash` match
4. if the payload differs, the daemon returns `idempotency_key_conflict`

## 9. Browser and Extension Recovery

The persistence model must tolerate extension disconnects.

Rules:

1. requests already in `pending_user_approval` remain durable
2. only the same `wallet_instance_id` may resume a presented request
3. disconnect alone does not imply rejection
4. non-presented requests may be re-dispatched after delivery-lease expiry

## 10. Daemon Restart Recovery

At startup, `starmaskd` should:

1. open the database
2. load all non-terminal requests
3. expire requests whose `expires_at < now`
4. clear stale delivery leases
5. retain presented requests for same-instance recovery
6. mark wallet instances disconnected until they re-register

## 11. Deliberate `v1` Omissions

The current persistence design does not yet define:

- unlock request storage
- backend-kind metadata
- generic backend snapshots

Those changes belong to the planned multi-backend evolution in
`docs/unified-wallet-coordinator-evolution.md`.
