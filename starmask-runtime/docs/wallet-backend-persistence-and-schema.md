# Starmask Wallet Backend Persistence and Schema

## Status

This document is the persistence and schema contract for the current pre-release multi-backend
implementation.

The baseline SQLite schema remains defined by:

- `docs/sqlite-schema-and-migrations.md`

## 1. Purpose

This document defines the minimum persistence and schema rules needed to implement:

- generic backend registration
- backend-aware routing
- same-instance recovery across backend kinds

It is intended to keep schema behavior explicit before the first release.

## 2. Scope

Phase 2 covers:

- `starmask_extension`
- `local_account_dir`
- request kinds `sign_transaction` and `sign_message`

Phase 2 does not yet add:

- unlock request rows
- `wallet_request_unlock`

Those remain phase-3 work.

## 3. Baseline Schema Strategy

This version has not been released, so the implementation does not maintain compatibility migrations
for older development databases.

Current rules:

1. `crates/starmaskd/schema.sql` is the current baseline schema
2. new empty databases are initialized directly from that schema
3. non-current schema versions are rejected instead of upgraded
4. unversioned non-empty databases are rejected instead of inferred or backfilled

## 4. `wallet_instances` Table Evolution

Current retained compatibility-facing columns:

- `wallet_instance_id`
- `extension_id`
- `extension_version`
- `protocol_version`
- `profile_hint`
- `lock_state`
- `connected`
- `last_seen_at`

Generic backend columns:

- `backend_kind`
- `transport_kind`
- `approval_surface`
- `instance_label`
- `capabilities_json`
- `backend_metadata_json`

Storage rules:

1. `backend_kind`, `transport_kind`, `approval_surface`, and `instance_label` are authoritative
   routing metadata
2. `capabilities_json` stores a canonical sorted JSON array
3. `backend_metadata_json` stores an opaque JSON object
4. `backend_metadata_json` is not query-critical and must not drive core routing by itself
5. schema DDL and the `user_version` bump must commit in one transaction

## 5. `wallet_accounts` Table Evolution

Current schema includes:

- `is_read_only`

Routing rule:

- accounts marked `is_read_only = true` must never be selected for signing requests

## 6. `requests` Table Rules

Phase-2 keeps the current request table shape unchanged unless a concrete implementation proves one
additional field is necessary.

Current request fields remain sufficient for:

1. request identity
2. payload integrity
3. wallet-instance pinning
4. delivery lease
5. presentation lease
6. result retention

Design choice:

- backend capability is derived from request kind, not stored as a separate request column

## 7. Repository and Index Requirements

Repository access must support:

1. load wallet instance by `wallet_instance_id`
2. list connected wallet instances with generic metadata
3. replace account snapshot atomically for one wallet instance
4. find routable accounts excluding `is_read_only = true`
5. resume presented requests by `wallet_instance_id`

Required uniqueness:

- `wallet_instances.wallet_instance_id`
- `wallet_accounts(wallet_instance_id, address)`

Recommended non-unique indexes:

- `wallet_instances(backend_kind, connected)`
- `wallet_instances(last_seen_at)`
- `wallet_accounts(address)`

`backend_metadata_json` and `capabilities_json` are intentionally not indexed in phase 2.

## 8. Recovery Rules

Same-instance recovery remains mandatory across backend kinds.

Rules:

1. daemon restart marks all wallet instances disconnected
2. backend metadata and account snapshots remain durable
3. requests already in `pending_user_approval` remain pinned to their recorded
   `wallet_instance_id`
4. re-registration with the same `wallet_instance_id` restores that instance's recovery rights
5. registration with the same `wallet_instance_id` but different `backend_kind` must be rejected

For `local_account_dir`, deterministic recovery depends on:

- `wallet_instance_id = backend_id`

## 9. Metadata Size and Boundedness

Persistence must stay bounded.

Required rules:

1. `backend_metadata_json` must remain a small object, with a target upper bound of 4 KiB
2. `capabilities_json` should remain a short canonical array
3. result retention remains finite and unchanged from current coordinator policy
4. maintenance must continue evicting expired retained result payloads

## 10. Compatibility Story

There is no database upgrade compatibility story before first release. The daemon preserves only two
runtime compatibility paths inside the current schema:

1. extension-backed wallet instances are represented as `starmask_extension` backend rows
2. local account-dir wallet instances are represented as `local_account_dir` backend rows

Existing development databases with older schema versions should be recreated.

## 11. Phase-3 Reserved Delta

Phase-2 deliberately does not add unlock persistence.

When explicit unlock flows land later, the expected schema delta may add:

- unlock request rows or request-kind support
- unlock expiry metadata
- terminal unlock outcome metadata

Those changes are not part of the phase-2 freeze.

## 12. Relationship to Other Documents

This document should be read together with:

- `docs/wallet-backend-agent-contract.md`
- `docs/wallet-backend-local-socket-binding.md`
- `docs/wallet-backend-configuration.md`
- `docs/wallet-backend-testing-and-acceptance.md`
