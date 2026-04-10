# Starmask Wallet Backend Agent Contract

## Status

This document is the logical contract for the current phase-2 multi-backend architecture.

The extension-backed `v1` behavior remains defined by:

- `docs/native-messaging-contract.md`
- `docs/daemon-protocol.md`
- `docs/starmask-interface-design.md`

This document exists to define the missing phase-2 layer between:

- `starmaskd` as coordinator
- one or more signer backends

## 1. Purpose

This document defines the transport-agnostic logical contract between `starmaskd` and a wallet
backend agent.

The contract must be usable by at least these backend families:

- `starmask_extension`
- `local_account_dir`
- `private_key_dev`

Its goal is to let the coordinator own routing, lifecycle, and recovery while each backend owns:

- key material
- local unlock state
- approval surface
- signature production

## 2. Design Constraints

The contract must preserve these invariants:

1. `starmaskd` never signs.
2. private keys never leave the selected backend.
3. passwords or unlock secrets never cross the MCP boundary.
4. canonical payload bytes remain the source of truth for approval.
5. request routing remains explicit and deterministic.
6. the same request lifecycle semantics work across different transports.

## 3. Layering

The intended layering is:

```text
MCP host
  -> starmask-runtime
  -> starmaskd
  -> wallet backend agent
     -> backend-specific signer runtime
```

Examples:

- `starmask_extension` backend agent:
  - `starmask-native-host`
  - Starmask extension
- `local_account_dir` backend agent:
  - local agent process
  - Starcoin `AccountProvider::Local`

## 4. Terminology

### Wallet backend

A wallet backend is one signer implementation family such as `starmask_extension` or
`local_account_dir`.

### Wallet instance

A wallet instance is one concrete runtime identity registered with the coordinator.

Examples:

- one browser profile connected through Native Messaging
- one local account agent serving one configured account directory

### Backend agent

A backend agent is the process or runtime component that speaks this contract to `starmaskd`.

### Approval surface

The approval surface is the local user-facing UI or prompt where consent is granted or denied.

## 5. Contract Scope

This contract covers:

- backend registration
- health and heartbeat signaling
- account snapshot updates
- request claiming
- presentation confirmation
- resolution and rejection
- same-instance recovery semantics

This contract does not define:

- MCP tools
- `starmaskd` JSON-RPC methods
- Native Messaging framing
- local-agent socket framing

Those are separate transport bindings or higher-level interfaces.

## 6. Capability Model

Each backend instance must advertise a capability set.

Phase-2 capability flags:

- `unlock`
- `get_public_key`
- `create_account`
- `sign_message`
- `sign_transaction`

Rules:

1. the coordinator must never route a request to a backend that does not advertise the required
   capability
2. capabilities are instance-scoped, not just backend-kind scoped
3. capability absence is authoritative and must fail closed
4. `unlock` allows a backend to accept sign requests while still requiring backend-local password
   entry before signature production
5. `create_account` allows a backend to accept an account-creation request where any new password is
   collected only on the backend-local approval surface

## 7. Wallet Instance Metadata

Each registered wallet instance should expose these logical fields:

- `wallet_instance_id`
- `backend_kind`
- `transport_kind`
- `approval_surface`
- `instance_label`
- `protocol_version`
- `lock_state`
- `connected`
- `capabilities`
- `backend_metadata`
- `accounts`

### 7.1 Backend metadata

`backend_metadata` is backend-specific and should stay out of the generic core tables until the
project decides on a stable schema.

Examples:

- `starmask_extension`
  - `extension_id`
  - `extension_version`
  - `profile_hint`
- `local_account_dir`
  - `account_provider_kind`
  - optional `path_hint`
  - `prompt_mode`
- `private_key_dev`
  - `secret_source_kind`
  - `unsafe_unattended`

## 8. Account Snapshot Model

Each backend instance must be able to publish an account snapshot.

Planned account fields:

- `address`
- `label`
- `public_key`
- `is_default`
- `is_read_only`
- `is_locked`

For `local_account_dir`, this should map cleanly to Starcoin `AccountInfo`:

- `address`
- `is_default`
- `is_readonly`
- `is_locked`
- `public_key`

## 9. Logical Operations

This contract defines logical operations, not transport-specific envelopes.

### 9.1 `backend.register`

Purpose:

- establish one wallet instance identity and its current snapshot

Required fields:

- `wallet_instance_id`
- `backend_kind`
- `transport_kind`
- `approval_surface`
- `protocol_version`
- `instance_label`
- `lock_state`
- `capabilities`
- `backend_metadata`
- `accounts`

Coordinator behavior:

1. validate protocol compatibility
2. validate backend-kind allowlist and channel policy
3. mark the instance connected
4. replace the current account snapshot

Result:

- `accepted`
- `coordinator_protocol_version`
- `wallet_instance_id`

### 9.2 `backend.heartbeat`

Purpose:

- keep the instance online and extend recovery state when applicable

Required fields:

- `wallet_instance_id`

Optional fields:

- `presented_request_ids`
- `lock_state` when the transport prefers piggyback updates

Coordinator behavior:

- update `last_seen_at`
- extend any valid presentation lease owned by this instance

### 9.3 `backend.updateAccounts`

Purpose:

- replace the visible account snapshot, current lock-state view, and advertised capabilities

Required fields:

- `wallet_instance_id`
- `lock_state`
- `capabilities`
- `accounts`

Coordinator behavior:

- replace account rows for that wallet instance atomically
- update routing eligibility from the latest lock-state and capability snapshot

### 9.4 `request.hasAvailable`

Purpose:

- provide a cheap polling path for transports that prefer ask-before-pull behavior

Required fields:

- `wallet_instance_id`

Result:

- `available`

This operation is optional for transport bindings. It must not be required for correctness.

### 9.5 `request.pullNext`

Purpose:

- claim the next eligible request for one wallet instance

Required fields:

- `wallet_instance_id`

Result when work exists:

- `request_id`
- `client_request_id`
- `kind`
- `account_address`
- `payload_hash`
- `display_hint`
- `client_context`
- `resume_required`
- `delivery_lease_id` and `lease_expires_at` for first presentation
- `presentation_id` and `presentation_expires_at` for same-instance resume
- payload body

Payload body rules:

- `create_account` carries only host display metadata; the backend must gather any new password
  locally
- `sign_transaction` carries canonical transaction bytes
- `sign_message` carries canonical message payload plus format
- future `unlock` requests should carry only unlock metadata, never passwords

Coordinator behavior:

1. only requests eligible for this exact `wallet_instance_id` may be returned
2. a fresh claim moves `created -> dispatched`
3. only one active delivery lease may exist per request
4. a resumed request remains bound to the same `wallet_instance_id`

### 9.6 `request.presented`

Purpose:

- confirm that the local approval surface is actually open and usable

Required fields:

- `wallet_instance_id`
- `request_id`
- `presentation_id`

Required on first presentation:

- `delivery_lease_id`

Coordinator behavior:

1. validate request ownership
2. validate delivery or resume context
3. move the request to `pending_user_approval`
4. pin the request to that wallet instance for the rest of the presentation lifecycle
5. blocking local prompt agents should send this immediately after rendering the prompt surface and
   before waiting on approval or password input so same-instance recovery stays active during the
   entire prompt

### 9.7 `request.resolve`

Purpose:

- report successful completion from the backend

Required fields:

- `wallet_instance_id`
- `request_id`
- `presentation_id`
- `result_kind`

Result payload rules:

- `signed_transaction` returns `signed_txn_bcs_hex`
- `signed_message` returns `signature`
- future `unlock_granted` may return `unlock_expires_at`

Coordinator behavior:

- validate ownership and presentation context
- store bounded retained result
- move request to `approved`

### 9.8 `request.reject`

Purpose:

- report explicit denial or backend-safe refusal

Required fields:

- `wallet_instance_id`
- `request_id`
- `reason_code`

Optional fields:

- `presentation_id`
- `reason_message`

Expected reason codes include:

- `request_rejected`
- `unsupported_operation`
- `invalid_transaction_payload`
- backend-specific safe refusal reasons that map to shared error codes

Coordinator behavior:

- validate ownership
- move request to `rejected`
- retain rejection metadata

## 10. Notifications and Hints

Transports may support coordinator-to-backend hints such as:

- `request.available`
- `request.cancelled`

Rules:

1. hints are best-effort only
2. correctness must not depend on receiving a hint
3. a backend must still pull on startup, reconnect, and after finishing work

## 11. Recovery Semantics

These semantics are shared across all transport bindings.

### Before `request.presented`

If the backend disconnects before `request.presented`:

- delivery lease expiry may return the request to `created`
- the same instance may claim it again later
- another instance must not claim it unless routing is explicitly recomputed before presentation

### After `request.presented`

If the backend disconnects after `request.presented`:

- the request remains pinned to the same `wallet_instance_id`
- transport loss alone does not imply approval or rejection
- only the same wallet instance may resume it

## 12. Transport Binding Rules

This logical contract may have more than one concrete binding.

### 12.1 Native Messaging binding

The existing `v1` extension contract is the first concrete binding.

Mapping examples:

- `extension.register` -> `backend.register`
- `extension.heartbeat` -> `backend.heartbeat`
- `extension.updateAccounts` -> `backend.updateAccounts`
- `request.pullNext`, `request.presented`, `request.resolve`, `request.reject` map directly

This binding keeps its current extension-specific field names for backward compatibility.

### 12.2 Local socket binding

A future local account agent should bind the same logical operations over a local transport such as:

- JSON-RPC over Unix socket
- framed JSON over Unix socket

The project should choose one binding and document it separately before implementation.

## 13. Security Requirements

Every backend transport binding must preserve these rules:

1. passwords are entered only inside the backend-local approval or unlock surface
2. backend registration is local-only and OS-user scoped
3. backend metadata must not be trusted as proof of signing authority by itself
4. canonical payload bytes, not host summaries, drive approval rendering
5. development-only backends remain channel-gated

## 14. Performance Requirements

The contract should support interactive local use without forcing high churn.

Required properties:

1. heartbeats are lightweight
2. account updates replace snapshots atomically
3. `request.pullNext` is cheap enough to call after every completion
4. the coordinator can bound concurrent active presentations per instance

## 15. Local Account Backend Notes

For `local_account_dir`, the backend agent should wrap Starcoin account storage and signing APIs.

Minimum functional requirements:

1. list accounts from `AccountProvider`
2. expose `is_default`, `is_read_only`, `is_locked`, and `public_key`
3. create a new local account when a `create_account` request is approved
4. sign canonical transaction bytes through `sign_txn`
5. sign canonical messages through `sign_message`
6. keep unlock and password entry entirely inside the backend agent
7. if the backend advertises `unlock`, any password prompt must happen only after local approval is
   displayed and must never cross daemon transport

## 16. Phase-2 Decisions Closed by Companion Documents

The previously open phase-2 decisions are now closed as follows:

1. the concrete local-socket binding is defined in `docs/wallet-backend-local-socket-binding.md`
2. the first generic backend contract ships under daemon protocol `v2`
3. `request.hasAvailable` remains optional and is not required for the local-socket binding
4. the shared backend-safe refusal codes are defined in
   `docs/wallet-backend-local-socket-binding.md`
5. generic backend metadata persistence is defined in
   `docs/wallet-backend-persistence-and-schema.md`

## 17. Relationship to Other Documents

This document should be read together with:

- `docs/unified-wallet-coordinator-evolution.md`
- `docs/wallet-backend-local-socket-binding.md`
- `docs/wallet-backend-security-model.md`
- `docs/wallet-backend-persistence-and-schema.md`
- `docs/wallet-backend-configuration.md`
- `docs/wallet-backend-testing-and-acceptance.md`
- `docs/security-model.md` for current `v1` invariants
- `docs/native-messaging-contract.md` for the existing concrete transport binding

This document is now paired with the implemented phase-2 transport, configuration, security, and
acceptance documents.
