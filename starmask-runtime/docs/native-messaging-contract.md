# Starmask Native Messaging Contract

## Status

This document is the authoritative `v1` contract for the extension-backed Native Messaging bridge.

Repository status note: the in-tree `crates/starmask-runtime` adapter has been removed. This contract
still applies to the remaining extension and daemon-side implementation.

It is intentionally specific to:

- Starmask extension
- `starmask-native-host`
- `starmaskd`

It is not the future generic backend-agent contract. That follow-on design is tracked in
`docs/unified-wallet-coordinator-evolution.md`.

## Purpose

This document defines the message contract between:

- `Starmask` Chrome extension
- `starmask-native-host`
- `starmaskd`

The native host is a transport shim. The daemon owns state. The extension owns signing and approval UI.

## Transport Model

The extension opens a long-lived Native Messaging connection.

Properties:

- messages are bidirectional JSON objects
- the extension may receive daemon hints through the same port
- correctness must not depend on hints; correctness depends on explicit claims through `request.pullNext`
- the host process should be kept alive through `connectNative()` rather than one-shot `sendNativeMessage()`

## Message Envelope

This contract uses a lightweight message envelope rather than JSON-RPC.

Rules:

1. every extension-originated command message must include:
   - `message_id`
   - `type`
2. every daemon response to an extension command must include:
   - `reply_to`
   - `type`
3. daemon notifications that are not direct replies do not include `reply_to`
4. the extension must tolerate notifications interleaving with replies
5. the extension may keep only a small number of in-flight commands, but correctness must not depend on strict request serialization

Recommended envelope fields:

- `type`
- `message_id`: for extension commands
- `reply_to`: for daemon responses

## Native Messaging Wire Format

Chrome Native Messaging requires:

- JSON messages
- UTF-8 encoding
- a 32-bit native-endian message length prefix

Chrome-side limits that the host must respect:

- maximum message size from host to Chrome: 1 MB
- maximum message size from Chrome to host: 64 MiB

Process rules:

- the first CLI argument identifies the caller origin
- stdout is reserved for protocol frames only
- stderr is the correct destination for diagnostics

Rust implementation guidance:

1. keep framing logic in one dedicated module
2. isolate stdout writes from logging completely
3. validate frame length before allocation
4. fail closed on malformed frames
5. if Windows needs a binary-mode stdio shim, isolate it in a tiny platform-specific module

## Native-Host Deployment Hardening

The bridge remains security-sensitive even though it does not sign.

Required deployment rules:

1. each channel must use its own manifest name and exact `allowed_origins` allowlist
2. wildcard origins must not be used
3. the manifest file must be owner-writable only and installed only in the OS-recognized Native
   Messaging manifest locations
4. the manifest `path` must be absolute
5. the native-host binary and its parent directories must not be writable by other OS users
6. the native host must validate the caller origin it receives from Chrome against configured
   channel policy before normal request handling begins
7. operators and TUIs must not pre-launch or daemonize `starmask-native-host`; Chrome owns that
   lifecycle
8. secrets must not be passed on argv, and stderr diagnostics must continue to avoid leaking
   sensitive daemon payloads

## Protocol Version

Initial native-bridge protocol version:

- `1`

Every registration message must include:

- `protocol_version`

If the daemon rejects the version, the native host must return:

- `protocol_version_mismatch`

and the extension must not continue normal request handling.

## Message Types

### Extension to Daemon

- `extension.register`
- `extension.heartbeat`
- `extension.updateAccounts`
- `request.pullNext`
- `request.presented`
- `request.resolve`
- `request.reject`

### Daemon to Extension

- `extension.registered`
- `extension.ack`
- `request.available`
- `request.next`
- `request.none`
- `request.cancelled`
- `extension.error`

## Registration

### `extension.register`

Purpose:

- establish logical wallet identity and compatibility

Payload:

- `message_id`
- `protocol_version`
- `wallet_instance_id`
- `extension_id`
- `extension_version`
- `profile_hint`
- `lock_state`
- `accounts_summary`

Daemon behavior:

1. validate extension ID against the configured channel allowlist
2. validate protocol compatibility
3. record `wallet_instance_id` as connected
4. refresh the wallet account cache

Response:

- `reply_to`
- `type = extension.registered`
- `wallet_instance_id`
- `daemon_protocol_version`
- `accepted`

## Heartbeats

### `extension.heartbeat`

Purpose:

- keep the wallet session and any presented requests alive

Payload:

- `message_id`
- `wallet_instance_id`
- `presented_request_ids`: optional array

Daemon behavior:

- update `last_seen_at`
- extend presentation lease for any listed presented requests owned by this instance
- respond with `extension.ack`

## Account Updates

### `extension.updateAccounts`

Purpose:

- synchronize account metadata and lock state

Payload:

- `message_id`
- `wallet_instance_id`
- `lock_state`
- `accounts`
  - `address`
  - `label`
  - `public_key`: optional
  - `is_default`

Daemon behavior:

- replace the visible account cache for that wallet instance
- update routing eligibility
- respond with `extension.ack`

## Request Delivery Model

The canonical model is:

1. daemon stores request in `created`
2. extension claims work through `request.pullNext`
3. daemon returns a request under a delivery lease
4. extension renders UI
5. extension confirms UI presentation
6. user approves or rejects
7. extension resolves the request

## `request.available`

Purpose:

- provide a best-effort hint that the wallet instance should pull work

Payload:

- `wallet_instance_id`

Rules:

- the extension must not rely on this hint for correctness
- if a hint is lost, the extension must still pull on connect and after finishing each request

## `request.pullNext`

Purpose:

- claim the next eligible request for one wallet instance

Payload:

- `message_id`
- `wallet_instance_id`

Response when work exists:

- `reply_to`
- `type = request.next`
- `request_id`
- `client_request_id`
- `kind`
- `account_address`
- `payload_hash`
- `display_hint`
- `client_context`
- `resume_required`
- first-presentation fields when `resume_required = false`
  - `delivery_lease_id`
  - `lease_expires_at`
- resume fields when `resume_required = true`
  - `presentation_id`
  - `presentation_expires_at`
- one of:
  - `raw_txn_bcs_hex`
  - `message`

Response when no work exists:

- `reply_to`
- `type = request.none`
- `wallet_instance_id`

Claim rules:

1. only requests eligible for this exact `wallet_instance_id` may be returned
2. `created -> dispatched` occurs when the daemon issues a first-presentation delivery lease
3. only one active delivery lease may exist for a request
4. if the delivery lease expires before `request.presented`, the daemon returns the request to `created`

`resume_required` rules:

- `false` for a first presentation
- `true` when the same wallet instance is reclaiming a request that had already been presented before disconnect

When `resume_required = true`:

1. the returned `presentation_id` is the active presentation identity
2. the extension must reopen the approval UI for that request
3. the extension must continue heartbeats for that `presentation_id`
4. the extension must not call `request.presented` a second time for the same presentation lifecycle

## `request.presented`

Purpose:

- tell the daemon that approval UI is now visible to the user

Payload:

- `message_id`
- `wallet_instance_id`
- `request_id`
- `delivery_lease_id`
- `presentation_id`

Daemon behavior:

1. validate lease ownership
2. move the request to `pending_user_approval`
3. bind the request to:
   - `wallet_instance_id`
   - `presentation_id`
4. start or extend the presentation lease
5. respond with `extension.ack`

Rules:

1. once `request.presented` is accepted, the request is pinned to that `wallet_instance_id`
2. after presentation, the request must never migrate to a different wallet instance
3. recovery after presentation is resume-on-same-instance only
4. `request.presented` is used for first presentation, not for reconnect resume

## `request.resolve`

Purpose:

- return an approved result

Payload for transaction signing:

- `message_id`
- `wallet_instance_id`
- `request_id`
- `presentation_id`
- `result_kind = signed_transaction`
- `signed_txn_bcs_hex`

Payload for message signing:

- `message_id`
- `wallet_instance_id`
- `request_id`
- `presentation_id`
- `result_kind = signed_message`
- `signature`

Daemon behavior:

1. validate presentation ownership
2. move the request to `approved`
3. store the result payload until `result_expires_at`
4. respond with `extension.ack`

## `request.reject`

Purpose:

- return a rejection or non-approvable outcome

Payload:

- `message_id`
- `wallet_instance_id`
- `request_id`
- `presentation_id`: optional when UI was not yet shown
- `reason_code`
- `reason_message`: optional

Typical `reason_code` values:

- `request_rejected`
- `wallet_locked`
- `request_expired`
- `unsupported_operation`
- `invalid_transaction_payload`

Daemon behavior:

1. validate request ownership
2. move the request to the appropriate terminal state:
   - `rejected`
   - `expired`
   - `failed`
3. respond with `extension.ack`

## `request.cancelled`

Purpose:

- notify the extension that the host or policy cancelled a request

Payload:

- `wallet_instance_id`
- `request_id`

Extension behavior:

1. close any matching approval UI if it is still open
2. stop local signing work for that request
3. ignore any stale user confirmation that arrives after cancellation

## Disconnect and Resume Rules

### Before `request.presented`

If the extension disconnects before `request.presented`:

- the daemon waits for the delivery lease to expire
- the request returns to `created`
- the request may later be claimed again

### After `request.presented`

If the extension disconnects after `request.presented`:

- the request remains `pending_user_approval`
- the request remains pinned to the same `wallet_instance_id`
- the daemon waits for the presentation lease to expire
- if the same instance reconnects before the presentation lease expires, it may reclaim the request with `resume_required = true`
- once the presentation lease expires, the daemon returns the request to `created`
- the request may then be claimed again only by the same `wallet_instance_id`
- if the same instance never reconnects, the request eventually expires

## Unsupported Payload Rule

If the extension cannot safely decode a payload for approval display:

- it must not blind-sign
- it must reject the request with `unsupported_operation` or `invalid_transaction_payload`

## Non-Goals

This contract does not define:

- the daemon database schema
- host-level MCP tool shapes

Those are defined in:

- `persistence-and-recovery.md`
- `starmask-interface-design.md`
