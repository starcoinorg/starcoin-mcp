# Starmask Core Rust API Design

## 1. Purpose

This document defines the Rust-facing API shape for the first unified implementation of:

- `starmask-types`
- `starmask-core`
- the coordinator-facing part of `starmaskd`

It bridges the protocol documents and the concrete Rust implementation after the architecture moves
from an extension-only model to a multi-backend wallet coordinator.

## 2. Design Goal

The core API should make the following hard to do incorrectly:

1. mutate lifecycle state outside the coordinator path
2. mix transport DTOs with domain types
3. confuse wallet-instance metadata with backend-specific metadata
4. bypass capability, lock-state, or idempotency checks when creating requests
5. let a backend resolve a request it does not own

## 3. Crate Roles

### 3.1 `starmask-types`

Contains:

- IDs
- enums
- shared DTOs
- shared error code enum

Should not contain:

- persistence code
- coordinator logic
- transport code

### 3.2 `starmask-core`

Contains:

- domain entities
- command types
- lifecycle transition rules
- policy checks
- repository traits
- coordinator service

Should not contain:

- `rmcp`
- Native Messaging framing
- direct SQLite SQL
- `AccountProvider` implementation details

## 4. Recommended Module Layout

```text
starmask-core/src/
  lib.rs
  ids.rs
  errors.rs
  status.rs
  policy.rs
  capabilities.rs
  entities/
    mod.rs
    request.rs
    wallet_instance.rs
    wallet_account.rs
  commands.rs
  services/
    mod.rs
    coordinator.rs
    request_service.rs
    wallet_service.rs
  repo/
    mod.rs
    request_repo.rs
    wallet_repo.rs
  time.rs
```

## 5. ID Newtypes

Recommended newtypes:

- `RequestId(String)`
- `ClientRequestId(String)`
- `WalletInstanceId(String)`
- `DeliveryLeaseId(String)`
- `PresentationId(String)`
- `PayloadHash(String)`

Rules:

1. derive `Clone`, `Debug`, `Eq`, `Ord`, `PartialEq`, `PartialOrd`, `Hash`
2. implement `Display`
3. implement `TryFrom<String>` when validation is needed

## 6. Core Enums

Recommended enums:

- `RequestKind`
  - `Unlock`
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
  - `UnlockGranted`
  - `SignedTransaction`
  - `SignedMessage`
  - `None`
- `LockState`
  - `Locked`
  - `Unlocked`
  - `Unknown`
- `BackendKind`
  - `StarmaskExtension`
  - `LocalAccountDir`
  - `PrivateKeyDev`
- `TransportKind`
  - `NativeMessaging`
  - `LocalSocket`
- `ApprovalSurface`
  - `BrowserUi`
  - `TtyPrompt`
  - `DesktopPrompt`
  - `None`
- `Channel`
  - `Development`
  - `Staging`
  - `Production`

## 7. Capability Model

Recommended capability enum:

- `WalletCapability`
  - `Unlock`
  - `GetPublicKey`
  - `SignMessage`
  - `SignTransaction`

Expose capabilities as a small set type or bitflags wrapper rather than free-form strings.

## 8. Entities

### 8.1 `RequestRecord`

Represents canonical persisted request state.

Recommended fields:

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
- `delivery_lease`
- `presentation`

### 8.2 `RequestPayload`

Recommended enum:

- `Unlock(UnlockPayload)`
- `SignTransaction(TransactionPayload)`
- `SignMessage(MessagePayload)`

`UnlockPayload` should contain:

- `requested_ttl_seconds`
- optional `account_address`
- optional `client_context`

`TransactionPayload` should contain:

- `chain_id`
- `raw_txn_bcs_hex`
- `tx_kind`
- optional `display_hint`
- optional `client_context`

`MessagePayload` should contain:

- `message_format`
- `message`
- optional `display_hint`
- optional `client_context`

### 8.3 `RequestResult`

Recommended enum:

- `UnlockGranted { unlock_expires_at: DateTime<Utc> }`
- `SignedTransaction { signed_txn_bcs_hex: String }`
- `SignedMessage { signature: String }`

### 8.4 `DeliveryLease`

Fields:

- `delivery_lease_id`
- `delivery_lease_expires_at`

### 8.5 `PresentationLease`

Fields:

- `presentation_id`
- `presentation_expires_at`

### 8.6 `WalletInstanceRecord`

Recommended fields:

- `wallet_instance_id`
- `backend_kind`
- `transport_kind`
- `approval_surface`
- `protocol_version`
- `label`
- `lock_state`
- `connected`
- `capabilities`
- `backend_metadata`
- `last_seen_at`

`backend_metadata` should be a typed enum rather than a generic JSON blob inside the core domain.

Recommended variants:

- `StarmaskExtensionMetadata`
  - `extension_id`
  - `extension_version`
  - `profile_hint`
- `LocalAccountDirMetadata`
  - `account_provider_kind`
  - `prompt_mode`
  - optional `path_hint`
- `PrivateKeyDevMetadata`
  - `secret_source_kind`
  - `unsafe_mode`

### 8.7 `WalletAccountRecord`

Fields:

- `wallet_instance_id`
- `address`
- `label`
- `public_key`
- `is_default`
- `is_read_only`
- `last_seen_at`

## 9. Boundary DTO vs Domain Types

Rule:

- transport DTOs are separate from domain entities

Recommended layering:

1. transport DTO
2. validated command struct
3. domain entity mutation
4. persisted record
5. response projection

`starmask-core` should operate on validated command structs and domain entities, not raw JSON DTOs.

## 10. Coordinator Command API

The coordinator should receive typed commands.

Recommended top-level command enum:

```text
CoordinatorCommand
  SystemPing
  GetInfo
  WalletStatus
  ListWalletInstances
  ListWalletAccounts
  GetPublicKey
  CreateUnlockRequest
  CreateSignTransactionRequest
  CreateSignMessageRequest
  GetRequestStatus
  CancelRequest
  RegisterBackend
  HeartbeatBackend
  UpdateBackendAccounts
  PullNextRequest
  MarkRequestPresented
  ResolveRequest
  RejectRequest
  TickMaintenance
```

### 10.1 Command structs

Recommended request-creation commands:

- `CreateUnlockRequest`
- `CreateSignTransactionRequest`
- `CreateSignMessageRequest`

Each should contain:

- `client_request_id`
- `wallet_instance_id`: optional at the boundary, resolved before persistence
- `account_address`
- request payload
- `ttl_seconds`

The coordinator service should resolve routing before persisting the final `RequestRecord`.

## 11. Repository Traits

Recommended repository traits:

- `RequestRepo`
- `WalletRepo`

`RequestRepo` should provide:

- create
- get by request ID
- get by client request ID
- update status atomically
- claim delivery lease
- claim presentation lease
- scan expiring records

`WalletRepo` should provide:

- upsert wallet instance
- upsert wallet accounts
- list wallet instances
- find wallet instances by account and capability
- mark wallet instance disconnected

## 12. Service Layer Rules

### 12.1 `CoordinatorService`

The coordinator service owns:

- route resolution
- policy checks
- idempotency checks
- status transitions
- maintenance tasks

### 12.2 `WalletService`

The wallet service owns:

- wallet-instance registration validation
- account snapshot replacement
- lock-state updates
- connection-status updates

### 12.3 `RequestService`

The request service owns:

- create request
- cancel request
- get status
- resolve and reject transitions
- expiry handling

## 13. Domain Rules

Required rules:

1. only the coordinator mutates request lifecycle state
2. only a backend with the correct `wallet_instance_id` may present, resolve, or reject a request
3. after `PendingUserApproval`, the request must never migrate to another wallet instance
4. a backend must advertise the required capability before a request can be created
5. if the same `client_request_id` arrives with the same payload hash, return the existing request
6. if the same `client_request_id` arrives with a different payload hash, fail with
   `IdempotencyConflict`

## 14. Projection Types

The core domain should expose projection types for daemon and adapter responses.

Recommended projections:

- `WalletStatusView`
- `WalletInstanceView`
- `WalletAccountView`
- `RequestStatusView`

Projection rules:

- domain entities remain internal to core services
- transport layers receive projection structs, not mutable entities

## 15. Rust Safety Guidance

The Rust workspace should default to:

- `#![forbid(unsafe_code)]`

If a platform integration requires unsafe code:

- isolate it outside core crates
- document the invariant
- keep the unsafe surface minimal

Additional guidance:

1. backend kinds and capability sets should be enums, not free-form strings
2. log redaction helpers should be reusable and centralized
3. secret-bearing types should avoid ordinary `String` storage where feasible

## 16. Ready-to-Implement Checklist

This document is implementation-ready when:

1. all newtypes and enums exist in `starmask-types`
2. `WalletInstanceRecord` is backend-generic
3. request payload and result enums cover unlock plus both sign flows
4. coordinator command structs exist
5. repository traits support generic backend registration and request lifecycle
