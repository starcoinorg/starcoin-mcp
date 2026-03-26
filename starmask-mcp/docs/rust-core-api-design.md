# Starmask Core Rust API Design

## Status

This document is the authoritative `v1` core API design for the current extension-backed
implementation.

Future generic backend evolution is tracked in `docs/unified-wallet-coordinator-evolution.md`.

## 1. Purpose

This document defines the Rust-facing API shape for:

- `starmask-types`
- `starmask-core`
- the coordinator-facing part of `starmaskd`

## 2. Design Goal

The core API should make the following hard to do incorrectly:

1. mutate lifecycle state outside the coordinator path
2. mix protocol DTOs with domain types
3. confuse delivery-lease and presentation-lease flows
4. bypass policy checks when creating or resolving requests

## 3. Crate Roles

### `starmask-types`

Contains:

- IDs
- enums
- shared DTOs
- shared error code enum

Should not contain:

- persistence code
- coordinator logic
- transport code

### `starmask-core`

Contains:

- domain entities
- command types
- lifecycle transition rules
- policy checks
- repository traits
- coordinator service

Should not contain:

- `rmcp`
- Chrome Native Messaging framing
- direct SQLite SQL

## 4. Core Types

### ID newtypes

- `RequestId`
- `ClientRequestId`
- `WalletInstanceId`
- `DeliveryLeaseId`
- `PresentationId`
- `PayloadHash`

### Core enums

- `RequestKind`
  - `SignTransaction`
  - `SignMessage`
- `RequestStatus`
  - `Created`
  - `Dispatched`
  - `PendingUserApproval`
  - `Approved`
  - `Rejected`
  - `Expired`
  - `Cancelled`
  - `Failed`
- `ResultKind`
  - `SignedTransaction`
  - `SignedMessage`
  - `None`
- `LockState`
  - `Locked`
  - `Unlocked`
  - `Unknown`
- `Channel`
  - `Development`
  - `Staging`
  - `Production`

## 5. Current Entities

### `RequestRecord`

Current fields:

- `request_id`
- `client_request_id`
- `kind`
- `status`
- `wallet_instance_id`
- `account_address`
- `payload_hash`
- `payload`
- `result`
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
- `delivery_lease`
- `presentation`

### `RequestPayload`

Current variants:

- `SignTransaction(TransactionPayload)`
- `SignMessage(MessagePayload)`

### `RequestResult`

Current variants:

- `SignedTransaction { signed_txn_bcs_hex }`
- `SignedMessage { signature }`

### `WalletInstanceRecord`

Current fields:

- `wallet_instance_id`
- `extension_id`
- `extension_version`
- `protocol_version`
- `profile_hint`
- `lock_state`
- `connected`
- `last_seen_at`

### `WalletAccountRecord`

Current fields:

- `wallet_instance_id`
- `address`
- `label`
- `public_key`
- `is_default`
- `is_locked`
- `last_seen_at`

## 6. Boundary DTO vs Domain Types

Transport DTOs remain separate from domain entities.

Recommended layering:

1. transport DTO
2. validated command struct
3. domain entity mutation
4. persisted record
5. response projection

## 7. Current Coordinator Command Surface

The current coordinator is extension-oriented. Its command set includes:

- `SystemPing`
- `GetInfo`
- `WalletStatus`
- `ListWalletInstances`
- `ListWalletAccounts`
- `GetPublicKey`
- `CreateSignTransactionRequest`
- `CreateSignMessageRequest`
- `GetRequestStatus`
- `CancelRequest`
- `RegisterExtension`
- `HeartbeatExtension`
- `UpdateExtensionAccounts`
- `HasAvailableRequest`
- `PullNextRequest`
- `MarkRequestPresented`
- `ResolveRequest`
- `RejectRequest`
- `TickMaintenance`

## 8. Repository Traits

The current repository split remains:

- `RequestRepository`
- `WalletRepository`

`WalletRepository` is currently extension-centric because wallet-instance records persist extension
metadata directly.

## 9. Rust Safety Guidance

The workspace should default to:

- `#![forbid(unsafe_code)]`

Additional guidance:

1. request IDs, lease IDs, and wallet-instance IDs should be distinct newtypes
2. lifecycle states should be enums, not mutable free-form strings
3. log redaction helpers should be centralized

## 10. Deliberate `v1` Omissions

The current core API does not yet define:

- a generic `BackendKind`
- backend capability sets
- unlock request kinds or unlock results
- backend-generic wallet-instance metadata

Those additions belong to the planned multi-backend evolution in
`docs/unified-wallet-coordinator-evolution.md`.
