# Starmask SQLite Schema and Migration Design

## Status

This document is the authoritative `v1` schema design for the current extension-backed
implementation.

Repository status note: the in-tree `crates/starmask-mcp` adapter has been removed. Schema guidance
remains current for the daemon-owned persistence layer.

Planned generic wallet-backend schema changes are tracked in
`docs/unified-wallet-coordinator-evolution.md`.

## 1. Purpose

This document defines the first SQLite schema for `starmaskd` and the migration strategy that
supports:

- durable request lifecycle state
- wallet registration and account cache
- bounded result retention
- restart-safe recovery

## 2. Design Goals

The schema should optimize for:

1. correctness over throughput
2. simple restart recovery queries
3. explicit uniqueness constraints for idempotency
4. easy inspection during diagnostics

## 3. SQLite Engine Rules

At startup, the daemon should apply:

- `PRAGMA journal_mode = WAL`
- `PRAGMA foreign_keys = ON`
- `PRAGMA busy_timeout = 5000`
- optional `PRAGMA synchronous = NORMAL`

## 4. Migration Strategy

The current implementation uses numbered SQL migrations in source control.

Current layout:

```text
starmaskd/migrations/
  0001_initial.sql
```

Future migrations should continue the same append-only numbering scheme.

Rules:

1. migrations are append-only
2. there is no manual drift between Rust structs and SQL columns
3. startup fails clearly if the schema version is unsupported

## 5. Current Tables

### `requests`

Current columns:

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
- `reject_reason_code TEXT`
- `delivery_lease_id TEXT`
- `delivery_lease_expires_at INTEGER`
- `presentation_id TEXT`
- `presentation_expires_at INTEGER`

Current kind values:

- `sign_transaction`
- `sign_message`

### `wallet_instances`

Current columns:

- `wallet_instance_id TEXT PRIMARY KEY`
- `extension_id TEXT NOT NULL`
- `extension_version TEXT NOT NULL`
- `protocol_version INTEGER NOT NULL`
- `profile_hint TEXT`
- `lock_state TEXT NOT NULL`
- `connected INTEGER NOT NULL`
- `last_seen_at INTEGER NOT NULL`

### `wallet_accounts`

Current columns:

- `wallet_instance_id TEXT NOT NULL`
- `address TEXT NOT NULL`
- `label TEXT`
- `public_key TEXT`
- `is_default INTEGER NOT NULL`
- `is_locked INTEGER NOT NULL`
- `last_seen_at INTEGER NOT NULL`

## 6. Current Indexes

Recommended indexes:

- unique index on `requests(client_request_id)`
- index on `requests(status, expires_at)`
- index on `requests(wallet_instance_id, status)`
- index on `requests(result_expires_at)`
- index on `wallet_instances(connected, last_seen_at)`
- index on `wallet_accounts(address)`

## 7. JSON Storage Strategy

The current implementation stores payload and result bodies as JSON strings:

- `payload_json`
- `result_json`

Why:

1. easier diagnostics
2. easier future schema evolution
3. sufficient for first-release local scale

## 8. Canonical Query Patterns

The repository layer needs optimized support for:

1. find request by `request_id`
2. find request by `client_request_id`
3. claim the next eligible `created` request for one wallet instance
4. list all non-terminal requests at daemon startup
5. evict results whose `result_expires_at < now`
6. load known wallet instances
7. load account cache for one or more wallet instances

## 9. Deliberate `v1` Omissions

The current schema does not yet define:

- backend-kind metadata
- generic backend capability metadata
- unlock request rows

Those belong to the planned multi-backend evolution in
`docs/unified-wallet-coordinator-evolution.md`.
