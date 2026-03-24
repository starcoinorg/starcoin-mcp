# Starmask Core Rust API Design

## Purpose

This document defines the Rust-facing API shape for the first implementation of:

- `starmask-types`
- `starmask-core`
- the coordinator-facing part of `starmaskd`

It is the missing bridge between the protocol documents and concrete Rust code.

## Design Goal

The core API should make the following hard to do incorrectly:

1. mutate lifecycle state outside the coordinator path
2. mix protocol DTOs with domain types
3. confuse delivery-lease and presentation-lease flows
4. bypass policy checks when creating or resolving requests

## Crate Roles

### `starmask-types`

Contains:

- ids
- enums
- shared DTOs
- shared error code enum
- time wrappers if adopted

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

## Recommended Module Layout

Recommended first-pass layout:

```text
starmask-core/src/
  lib.rs
  ids.rs
  errors.rs
  status.rs
  policy.rs
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

## Core Types

## Id Newtypes

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

## Core Enums

Recommended enums:

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
- `RejectReasonCode`
  - shared reason codes used by the extension and daemon

## Entities

### `RequestRecord`

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

### `RequestPayload`

Recommended enum:

- `SignTransaction(TransactionPayload)`
- `SignMessage(MessagePayload)`

### `RequestResult`

Recommended enum:

- `SignedTransaction { signed_txn_bcs_hex: String }`
- `SignedMessage { signature: String }`

### `DeliveryLease`

Fields:

- `delivery_lease_id`
- `delivery_lease_expires_at`

### `PresentationLease`

Fields:

- `presentation_id`
- `presentation_expires_at`

### `WalletInstanceRecord`

Fields:

- `wallet_instance_id`
- `extension_id`
- `extension_version`
- `protocol_version`
- `profile_hint`
- `lock_state`
- `connected`
- `last_seen_at`

### `WalletAccountRecord`

Fields:

- `wallet_instance_id`
- `address`
- `label`
- `public_key`
- `is_default`
- `last_seen_at`

## Boundary DTO vs Domain Types

Rule:

- transport DTOs are separate from domain entities

Recommended layering:

1. transport DTO
2. validated command struct
3. domain entity mutation
4. persisted record
5. response projection

`starmask-core` should operate on validated command structs and domain entities, not raw JSON DTOs.

## Coordinator Command API

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
  CreateSignTransactionRequest
  CreateSignMessageRequest
  GetRequestStatus
  CancelRequest
  RegisterExtension
  HeartbeatExtension
  UpdateExtensionAccounts
  PullNextRequest
  MarkRequestPresented
  ResolveRequest
  RejectRequest
  TickMaintenance
```

Each command should carry:

- validated inputs
- a `oneshot` responder typed to that command's result

## Service API

Recommended services inside `starmask-core`:

### `RequestService`

Responsibilities:

- create requests
- apply request lifecycle transitions
- enforce idempotency
- evaluate expiry and result retention

Recommended methods:

- `create_sign_transaction(...)`
- `create_sign_message(...)`
- `get_request_status(...)`
- `cancel_request(...)`
- `claim_next_request(...)`
- `mark_presented(...)`
- `resolve_request(...)`
- `reject_request(...)`
- `run_maintenance(...)`

### `WalletService`

Responsibilities:

- register wallet instances
- update account visibility
- resolve account-to-wallet routing
- expose public key lookup

Recommended methods:

- `register_extension(...)`
- `heartbeat_extension(...)`
- `update_accounts(...)`
- `list_instances(...)`
- `list_accounts(...)`
- `get_public_key(...)`
- `resolve_wallet_route(...)`

### `Coordinator`

Responsibilities:

- receive commands
- call `WalletService` and `RequestService`
- keep the single mutation path

Recommended entrypoint:

- `async fn run(self, rx: mpsc::Receiver<CoordinatorCommand>)`

## Repository Traits

The repository layer should be trait-based.

### `RequestRepository`

Recommended methods:

- `insert_request`
- `find_request_by_id`
- `find_request_by_client_request_id`
- `update_request`
- `claim_next_created_request_for_wallet`
- `list_non_terminal_requests`
- `list_terminal_requests_for_gc`
- `evict_result_payload`
- `delete_terminal_request`

### `WalletRepository`

Recommended methods:

- `upsert_wallet_instance`
- `mark_wallet_disconnected`
- `update_wallet_last_seen`
- `replace_wallet_accounts`
- `find_wallet_instance`
- `list_wallet_instances`
- `find_wallet_accounts`
- `find_wallets_for_account`

## Policy Interface

Policy should be explicit and injectable.

Recommended trait:

- `PolicyEngine`

Recommended methods:

- `can_list_accounts`
- `can_get_public_key`
- `can_create_sign_request`
- `can_auto_route`
- `can_resume_presented_request`
- `is_supported_payload`

The first implementation may ship with one concrete `DefaultPolicyEngine`.

## Time Interface

Time should be injectable for tests.

Recommended trait:

- `Clock`

Recommended methods:

- `now() -> DateTime<Utc>`

This avoids ad hoc direct wall-clock reads inside lifecycle logic.

## ID Generation Interface

Id and lease generation should also be injectable.

Recommended trait:

- `IdGenerator`

Recommended methods:

- `new_request_id`
- `new_delivery_lease_id`
- `new_presentation_id`

This makes restart, retry, and snapshot-style tests deterministic.

## Result Projection API

The core should expose projection helpers instead of forcing adapters to reconstruct status payloads manually.

Recommended projectors:

- `RequestStatusView`
- `WalletStatusView`
- `WalletAccountsView`

Adapters can serialize these views without needing direct access to internal state.

## Error Boundaries

Recommended error layers:

- `DomainError`
- `PolicyError`
- `RepositoryError`
- `CoordinatorError`

At protocol boundaries, these should be mapped into:

- shared error code
- user-facing message
- retryable hint

## First Implementation Freeze

The first implementation should not yet add:

- plugin policy engines
- multiple repository backends
- event sourcing
- streaming callbacks to the MCP host

## Ready-to-Implement Checklist

This document is implementation-ready when:

1. the exact command structs are created
2. the repository traits are created
3. the status projector structs are created
4. the coordinator command loop is created

At that point, `starmask-core` can be implemented without reopening transport or persistence semantics.
