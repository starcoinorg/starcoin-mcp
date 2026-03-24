# Starmask SQLite Schema and Migration Design

## Purpose

This document defines the first SQLite schema for `starmaskd` and the migration strategy that supports:

- durable request lifecycle state
- wallet registration and account cache
- bounded result retention
- restart-safe recovery

## Design Goals

The schema should optimize for:

1. correctness over throughput
2. simple restart recovery queries
3. explicit uniqueness constraints for idempotency
4. easy inspection during diagnostics

## SQLite Engine Rules

At startup, the daemon should apply:

- `PRAGMA journal_mode = WAL`
- `PRAGMA foreign_keys = ON`
- `PRAGMA busy_timeout = 5000`

Optional:

- `PRAGMA synchronous = NORMAL`

## Migration Strategy

The first implementation should use numbered SQL migrations in source control.

Recommended layout:

```text
starmaskd/migrations/
  0001_initial.sql
  0002_indexes.sql
  0003_result_retention.sql
```

Rules:

1. migrations are append-only
2. no manual drift between Rust structs and SQL columns
3. startup must fail clearly if the schema version is unsupported

## Version Tracking

Recommended approach:

- use SQLite `user_version`
- also expose the schema version in `system.getInfo`

## Tables

## `requests`

Recommended columns:

- `request_id TEXT PRIMARY KEY`
- `client_request_id TEXT NOT NULL`
- `kind TEXT NOT NULL`
- `status TEXT NOT NULL`
- `wallet_instance_id TEXT NOT NULL`
- `account_address TEXT NOT NULL`
- `payload_hash TEXT NOT NULL`
- `payload_json TEXT NOT NULL`
- `result_json TEXT`
- `created_at INTEGER NOT NULL`
- `expires_at INTEGER NOT NULL`
- `updated_at INTEGER NOT NULL`
- `approved_at INTEGER`
- `rejected_at INTEGER`
- `cancelled_at INTEGER`
- `failed_at INTEGER`
- `result_expires_at INTEGER`
- `last_error_code TEXT`
- `last_error_message TEXT`
- `delivery_lease_id TEXT`
- `delivery_lease_expires_at INTEGER`
- `presentation_id TEXT`
- `presentation_expires_at INTEGER`

Recommended constraints:

1. `kind` constrained to:
   - `sign_transaction`
   - `sign_message`
2. `status` constrained to canonical lifecycle values
3. `client_request_id` unique within active retention horizon

Practical first implementation:

- create a unique index on `client_request_id`
- keep terminal records long enough that safe retry collisions are still valid

## `wallet_instances`

Recommended columns:

- `wallet_instance_id TEXT PRIMARY KEY`
- `extension_id TEXT NOT NULL`
- `extension_version TEXT NOT NULL`
- `protocol_version INTEGER NOT NULL`
- `profile_hint TEXT`
- `lock_state TEXT NOT NULL`
- `connected INTEGER NOT NULL`
- `last_seen_at INTEGER NOT NULL`

## `wallet_accounts`

Recommended columns:

- `wallet_instance_id TEXT NOT NULL`
- `address TEXT NOT NULL`
- `label TEXT`
- `public_key TEXT`
- `is_default INTEGER NOT NULL`
- `last_seen_at INTEGER NOT NULL`

Recommended constraints:

- composite primary key:
  - `(wallet_instance_id, address)`
- foreign key to `wallet_instances(wallet_instance_id)`

## Indexes

Recommended indexes:

### `requests`

- index on `(status, expires_at)`
- index on `(wallet_instance_id, status)`
- index on `(wallet_instance_id, account_address)`
- index on `(result_expires_at)`
- unique index on `(client_request_id)`

### `wallet_instances`

- index on `(connected, last_seen_at)`

### `wallet_accounts`

- index on `(address)`
- index on `(wallet_instance_id, address)`

## JSON Storage Strategy

The first implementation should store payload and result bodies as JSON strings:

- `payload_json`
- `result_json`

Why:

1. easier diagnostics
2. easier future schema evolution
3. enough for first-release local scale

Rule:

- the Rust repository layer owns encoding and decoding
- SQL callers should not partially manipulate JSON blobs

## Canonical Query Patterns

The repository layer needs optimized support for:

1. find request by `request_id`
2. find request by `client_request_id`
3. claim next eligible `created` request for one wallet instance
4. list all non-terminal requests at daemon startup
5. evict results whose `result_expires_at < now`
6. delete old terminal records after retention
7. find all wallet instances that expose one account

## Claim Query Rules

Claiming the next request for a wallet should be transactional.

Recommended flow:

1. begin immediate transaction
2. select one eligible `created` request for the target `wallet_instance_id`
3. update it to `dispatched`
4. assign `delivery_lease_id`
5. commit

This prevents concurrent claimers from taking the same request.

## Mutation Boundaries

Every lifecycle transition should happen in one explicit transaction.

Examples:

- create request
- cancel request
- mark presented
- approve request
- reject request
- expire request
- return expired delivery lease to `created`
- evict result payload

## Startup Recovery Queries

At daemon startup, the repository must support:

1. load all non-terminal requests
2. clear or reinterpret expired delivery leases
3. keep valid presentation leases
4. load connected and recently seen wallet instances
5. load account cache for all known wallet instances

## Garbage Collection Strategy

Recommended background jobs:

1. result payload eviction
2. old terminal record deletion
3. stale disconnected wallet cleanup

Each job should run as:

- coordinator maintenance command
- one explicit transaction per batch

## Migration Testing

The first implementation should test:

1. fresh database bootstrap
2. upgrade from previous schema version
3. startup failure on unsupported future version
4. WAL and FK settings applied successfully

## Diagnostic Requirements

`starmaskctl doctor` should be able to inspect:

- database path
- schema version
- count of non-terminal requests
- count of result payloads awaiting eviction
- count of connected wallet instances

## Non-Goals

This document does not define the Rust repository method signatures.

Those are defined in:

- `rust-core-api-design.md`
