# Starmask Wallet Backend Persistence and Schema

## Status

This document is the phase-2 persistence and schema contract for the planned multi-backend
implementation.

It is not part of the current `v1` release contract. The current extension-backed persistence model
remains defined by:

- `docs/persistence-and-recovery.md`
- `docs/sqlite-schema-and-migrations.md`

## 1. Purpose

This document defines the minimum persistence and migration rules needed to implement:

- generic backend registration
- backend-aware routing
- same-instance recovery across backend kinds

It is intended to remove schema ambiguity before coding starts.

## 2. Scope

Phase 2 covers:

- `starmask_extension`
- `local_account_dir`
- request kinds `sign_transaction` and `sign_message`

Phase 2 does not yet add:

- unlock request rows
- `wallet_request_unlock`

Those remain phase-3 work.

## 3. Migration Strategy

Phase-2 should add one new migration:

- `0002_generic_wallet_backends.sql`

The phase-2 schema version becomes:

- `2`

Migration rules:

1. the migration must be additive
2. existing `v1` rows must remain readable
3. existing extension-backed rows must be backfilled into the new generic fields
4. no destructive column removal is allowed in the phase-2 migration

## 4. `wallet_instances` Table Evolution

Phase-2 keeps the current `v1` columns and adds generic ones.

Existing retained columns:

- `wallet_instance_id`
- `extension_id`
- `extension_version`
- `protocol_version`
- `profile_hint`
- `lock_state`
- `connected`
- `last_seen_at`

New columns:

- `backend_kind`
- `transport_kind`
- `approval_surface`
- `instance_label`
- `capabilities_json`
- `backend_metadata_json`

Phase-2 storage rules:

1. `backend_kind`, `transport_kind`, `approval_surface`, and `instance_label` are authoritative
   routing metadata
2. `capabilities_json` stores a canonical sorted JSON array
3. `backend_metadata_json` stores an opaque JSON object
4. `backend_metadata_json` is not query-critical and must not drive core routing by itself
5. schema DDL, v2 backfill, and the `user_version` bump must commit in one transaction

Backfill rules for existing extension rows:

- `backend_kind = "starmask_extension"`
- `transport_kind = "native_messaging"`
- `approval_surface = "browser_ui"`
- `instance_label = profile_hint` when available, otherwise `wallet_instance_id`
- `capabilities_json = ["get_public_key", "sign_message", "sign_transaction"]`
- `backend_metadata_json` includes `extension_id`, `extension_version`, and `profile_hint`

## 5. `wallet_accounts` Table Evolution

Phase-2 adds:

- `is_read_only`

Backfill rule:

- existing rows default to `false`

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

Phase-2 design choice:

- backend capability is derived from request kind, not stored as a separate request column

## 7. Repository and Index Requirements

Phase-2 repository access must support:

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

Phase-2 persistence must stay bounded.

Required rules:

1. `backend_metadata_json` must remain a small object, with a target upper bound of 4 KiB
2. `capabilities_json` should remain a short canonical array
3. result retention remains finite and unchanged from current coordinator policy
4. maintenance must continue evicting expired retained result payloads

## 10. Compatibility Story

Phase-2 migration must preserve two paths:

1. existing extension-backed `v1` rows continue to work after migration
2. new generic backend rows can coexist with migrated extension rows in the same database

The daemon must not require an empty database to adopt phase 2.

## 11. Phase-3 Reserved Delta

Phase-2 deliberately does not add unlock persistence.

When explicit unlock flows land later, the expected follow-on migration is:

- `0003_unlock_requests.sql`

That future migration may add:

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
