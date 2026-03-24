# Starmask MCP Persistence and Recovery

## Purpose

This document defines:

- what `starmaskd` persists
- how leases are represented
- how results are retained and evicted
- how recovery works across restarts and disconnects

## Design Goals

Persistence must guarantee:

1. no lost request state after daemon restart
2. no duplicate approval prompt on safe retries
3. deterministic recovery after browser or extension disconnect
4. bounded retention of signed outputs

## Canonical Storage Model

The first implementation should use one canonical request table with kind-specific payload and result columns.

This resolves the earlier design question in favor of:

- one shared request table for `sign_transaction` and `sign_message`

## Required Persistent Entities

### `requests`

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
3. `client_request_id` plus mismatched `payload_hash` is invalid

### `wallet_instances`

Required fields:

- `wallet_instance_id`
- `extension_id`
- `extension_version`
- `protocol_version`
- `profile_hint`
- `lock_state`
- `connected`
- `last_seen_at`

### `wallet_accounts`

Required fields:

- `wallet_instance_id`
- `address`
- `label`
- `public_key`
- `is_default`
- `last_seen_at`

## Lease Model

The daemon uses two lease types.

### Delivery Lease

Purpose:

- protect a request while it is being prepared for presentation

Fields:

- `delivery_lease_id`
- `delivery_lease_expires_at`

Rules:

1. only valid in `dispatched`
2. expires automatically if `request.presented` is not received in time
3. expiry returns the request to `created`

### Presentation Lease

Purpose:

- keep a presented request recoverable on the same wallet instance

Fields:

- `presentation_id`
- `presentation_expires_at`

Rules:

1. begins at `request.presented`
2. is extended by `extension.heartbeat`
3. is valid only for the presenting `wallet_instance_id`
4. after `request.presented`, the request is pinned to that instance

## Default Timing Rules

Initial default values:

- `request_ttl_seconds = 300`
- `delivery_lease_seconds = 30`
- `presentation_lease_seconds = 45`
- `heartbeat_interval_seconds = 10`
- `wallet_offline_after_seconds = 25`
- `result_retention_seconds = 600`
- `terminal_record_retention_seconds = 86400`

The exact defaults are defined in `configuration.md`.

## Result Retention Policy

The first implementation uses bounded multi-read retention.

Rules:

1. approved results may be read multiple times until `result_expires_at`
2. after `result_expires_at`, payload result bytes are deleted
3. terminal metadata remains until terminal record retention expires
4. after payload eviction, `wallet_get_request_status` still returns:
   - `status = approved`
   - `result_kind`
   - `result_available = false`
   - `error_code = result_unavailable`

This resolves the earlier design question in favor of:

- bounded multi-read, not single-read

## Request Creation Recovery

To avoid duplicate requests during transport uncertainty:

1. create methods require `client_request_id`
2. the daemon persists `client_request_id` before returning success
3. a retried create call with the same `client_request_id` and `payload_hash` returns the original `request_id`

## Request Cancellation Recovery

Cancellation rules:

1. cancelling `created`, `dispatched`, or `pending_user_approval` moves the request to `cancelled`
2. cancellation is persisted before the daemon notifies the extension
3. late resolve or reject messages for a cancelled request must be ignored

## Restart Recovery

### Daemon Restart

On startup, the daemon must:

1. load all non-terminal requests
2. clear stale transport session markers
3. keep delivery leases only if still within expiry
4. keep presentation leases only if still within expiry
5. return expired delivery leases to `created`
6. keep `pending_user_approval` requests pinned to their `wallet_instance_id`

### Extension Restart

On reconnect, the extension must:

1. re-register `wallet_instance_id`
2. refresh account cache
3. begin heartbeats
4. call `request.pullNext`

Recovery behavior:

1. if a request was only `dispatched`, it may be claimed again
2. if a request was `pending_user_approval` and pinned to this instance, `request.pullNext` may return it with:
   - `resume_required = true`
   - the active `presentation_id`

### Host Restart

The host should:

1. persist `request_id`
2. resume polling the same request
3. reuse `client_request_id` only when retrying request creation after an uncertain failure

## Re-Delivery Rules

This section closes the earlier unresolved question about re-delivery after presentation.

### Before Presentation

Requests may be re-delivered if:

- the request is still non-terminal
- the delivery lease expired
- the request has not reached `pending_user_approval`

### After Presentation

Requests may only be resumed by the same `wallet_instance_id`.

Rules:

1. after `request.presented`, the request must never migrate to a different wallet instance
2. if the same wallet instance reconnects before `expires_at`, it may resume
3. if the same wallet instance never reconnects, the request remains pending until:
   - the user rejects
   - the host cancels
   - the request expires

This resolves the re-delivery policy in favor of:

- same-instance resume only
- no cross-instance re-delivery after presentation

## Expiry Rules

Expiry applies regardless of connectivity.

Rules:

1. if `now >= expires_at` and the request is non-terminal, the daemon moves it to `expired`
2. expiry is terminal
3. expiry removes any active lease

## Garbage Collection

The daemon should periodically:

1. evict expired payload results
2. delete terminal records older than retention
3. remove disconnected wallet-instance cache entries that exceed the configured stale threshold

Garbage collection must never delete:

- a non-terminal request
- terminal metadata before its retention period expires

## Observable Recovery Outcomes

The host should be able to distinguish:

- request still pending and recoverable
- request approved but result evicted
- request cancelled
- request expired
- request failed

This is surfaced through:

- `status`
- `result_available`
- `result_expires_at`
- `error_code`

## Non-Goals

This document does not define SQL statements or a concrete migration tool.

It defines required persistence semantics that any storage implementation must preserve.
