# Starmask SQLite Schema and Migration Design

## 1. Purpose

This document defines the first SQLite schema for `starmaskd` and the migration strategy that
supports:

- durable request lifecycle state
- wallet-instance registration and account cache
- bounded result retention
- restart-safe recovery

The schema is backend-generic and must support both browser and local-account wallet instances.

## 2. Design Goals

The schema should optimize for:

1. correctness over throughput
2. simple restart recovery queries
3. explicit uniqueness constraints for idempotency
4. easy inspection during diagnostics
5. compatibility with multiple backend kinds

## 3. SQLite Engine Rules

At startup, the daemon should apply:

- `PRAGMA journal_mode = WAL`
- `PRAGMA foreign_keys = ON`
- `PRAGMA busy_timeout = 5000`

Optional:

- `PRAGMA synchronous = NORMAL`

## 4. Migration Strategy

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
2. there is no manual drift between Rust structs and SQL columns
3. startup fails clearly if the schema version is unsupported

## 5. Version Tracking

Recommended approach:

- use SQLite `user_version`
- also expose the schema version in `system.getInfo`

## 6. Tables

### 6.1 `requests`

Recommended columns:

- `request_id TEXT PRIMARY KEY`
- `client_request_id TEXT NOT NULL`
- `kind TEXT NOT NULL`
- `status TEXT NOT NULL`
- `wallet_instance_id TEXT NOT NULL`
- `account_address TEXT`
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
   - `unlock`
   - `sign_transaction`
   - `sign_message`
2. `status` constrained to canonical lifecycle values
3. `client_request_id` unique within the active retention horizon

Practical first implementation:

- create a unique index on `client_request_id`
- keep terminal records long enough that safe retry collisions remain valid

### 6.2 `wallet_instances`

Recommended columns:

- `wallet_instance_id TEXT PRIMARY KEY`
- `backend_kind TEXT NOT NULL`
- `transport_kind TEXT NOT NULL`
- `approval_surface TEXT NOT NULL`
- `protocol_version INTEGER NOT NULL`
- `label TEXT`
- `lock_state TEXT NOT NULL`
- `connected INTEGER NOT NULL`
- `capabilities_json TEXT NOT NULL`
- `backend_metadata_json TEXT NOT NULL`
- `last_seen_at INTEGER NOT NULL`

### 6.3 `wallet_accounts`

Recommended columns:

- `wallet_instance_id TEXT NOT NULL`
- `address TEXT NOT NULL`
- `label TEXT`
- `public_key TEXT`
- `is_default INTEGER NOT NULL`
- `is_read_only INTEGER NOT NULL`
- `last_seen_at INTEGER NOT NULL`

Recommended constraints:

- composite primary key:
  - `(wallet_instance_id, address)`
- foreign key to `wallet_instances(wallet_instance_id)`

## 7. Indexes

Recommended indexes:

### 7.1 `requests`

- index on `(status, expires_at)`
- index on `(wallet_instance_id, status)`
- index on `(wallet_instance_id, account_address)`
- index on `(result_expires_at)`
- unique index on `(client_request_id)`

### 7.2 `wallet_instances`

- index on `(connected, last_seen_at)`
- index on `(backend_kind, connected)`

### 7.3 `wallet_accounts`

- index on `(address)`
- index on `(wallet_instance_id, address)`

## 8. JSON Storage Strategy

The first implementation should store payload and result bodies as JSON strings:

- `payload_json`
- `result_json`
- `capabilities_json`
- `backend_metadata_json`

Why:

1. easier diagnostics
2. easier future schema evolution
3. enough for first-release local scale

Rules:

- the Rust repository layer owns encoding and decoding
- SQL callers do not partially manipulate JSON blobs
- typed enums and structs remain the source of truth in Rust

## 9. Canonical Query Patterns

The repository layer needs optimized support for:

1. find request by `request_id`
2. find request by `client_request_id`
3. claim next eligible `created` request for one wallet instance
4. list all non-terminal requests at daemon startup
5. evict results whose `result_expires_at < now`
6. delete old terminal records after retention
7. resolve wallet instances by account and capability

## 10. Migration Safety Rules

Schema changes must preserve:

1. existing terminal request records
2. idempotency guarantees for `client_request_id`
3. recoverability of presented requests
4. compatibility checks between schema version and daemon version

If a migration changes a JSON payload shape:

- add a Rust-layer compatibility path or migration transform
- do not silently reinterpret old payload blobs

## 11. Performance Notes

The first schema should stay simple, but it still needs predictable local latency.

Recommended rules:

1. avoid wide multi-table joins on hot status-polling paths
2. keep request polling satisfied by one indexed request lookup
3. replace account snapshots in bounded transactions
4. prune expired payloads and terminal rows incrementally

## 12. Closed First-Release Decisions

The first schema is closed on these decisions:

1. one shared `requests` table covers unlock and both signing flows
2. backend-specific metadata lives in `backend_metadata_json`
3. result payloads are retained for bounded multi-read access
4. schema migrations are numbered and append-only
