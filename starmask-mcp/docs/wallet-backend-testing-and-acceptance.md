# Starmask Wallet Backend Testing and Acceptance

## Status

This document is the phase-2 acceptance contract for the planned multi-backend implementation.

It is not part of the current `v1` release contract. The current extension-backed acceptance rules
remain defined by:

- `docs/testing-and-acceptance.md`

## 1. Purpose

This document defines the minimum evidence required to freeze implementation of:

- generic backend registration
- `local_account_dir`
- local-socket backend transport

It exists so coding can target one explicit phase-2 release gate instead of inferring behavior from
scattered design notes.

## 2. Phase-2 Scope

Phase 2 covers:

- generic backend registration through `backend.*`
- local-socket transport binding
- `local_account_dir` sign-transaction and sign-message flows
- coexistence with migrated extension-backed rows

Phase 2 does not require:

- `wallet_request_unlock`
- unlock request persistence
- unattended production signing

## 3. Required Design Documents

The project is not ready for phase-2 code freeze unless these documents exist and remain
consistent:

- `docs/wallet-backend-agent-contract.md`
- `docs/wallet-backend-local-socket-binding.md`
- `docs/wallet-backend-security-model.md`
- `docs/wallet-backend-persistence-and-schema.md`
- `docs/wallet-backend-configuration.md`
- `docs/wallet-backend-testing-and-acceptance.md`

## 4. Recommended Test Layers

Recommended test layout:

1. unit tests for generic coordinator routing and capability checks
2. local-socket transport integration tests over JSON-RPC `v2`
3. repository migration tests from schema `v1` to schema `v2`
4. config validation tests for backend entries
5. local-account backend integration tests using temporary account directories
6. compatibility tests proving the current extension-backed `v1` path still works

## 5. Transport Acceptance

The implementation must demonstrate:

1. `backend.register` round-trips with `protocol_version = 2`
2. `backend.heartbeat` updates liveness and presented-request recovery
3. `backend.updateAccounts` replaces the account snapshot atomically
4. `request.pullNext`, `request.presented`, `request.resolve`, and `request.reject` work for a
   local backend over the local-socket binding
5. unknown `wallet_instance_id` registration fails closed
6. disabled backend entries cannot register
7. `request.hasAvailable` remains optional and is not required for correctness

## 6. Local Account Acceptance

The implementation must demonstrate:

1. `local_account_dir` publishes accounts through `AccountProvider`
2. read-only accounts are listed but never routed for signing
3. `sign_transaction` returns a signed transaction without daemon-side signing
4. `sign_message` returns a message signature without daemon-side signing
5. a locked local backend fails signing safely with `wallet_locked`
6. a local prompt flow can approve and reject both request kinds

## 7. Security Acceptance

The implementation must demonstrate:

1. `local_account_dir` startup fails on insecure permissions
2. symlink escapes outside the canonical account directory are rejected
3. no password crosses MCP or daemon JSON-RPC boundaries
4. logs do not print plaintext passwords, private keys, or full signed payloads by default
5. canonical payload bytes, not host summaries, drive local approval rendering

## 8. Recovery Acceptance

The implementation must demonstrate:

1. daemon restart with a generic backend registration record present
2. daemon restart with a `created` request targeting `local_account_dir`
3. daemon restart with a `dispatched` request targeting `local_account_dir`
4. daemon restart with a `pending_user_approval` request targeting `local_account_dir`
5. local backend restart before `request.presented`
6. local backend restart after `request.presented` with same-instance resume
7. no cross-instance re-delivery after `request.presented`

## 9. Compatibility Acceptance

The implementation must demonstrate:

1. migrated extension-backed `v1` rows remain readable after schema upgrade
2. extension-backed routing still works for the current `v1` path
3. daemon protocol `v1` clients are not silently reinterpreted as generic `v2` clients

## 10. Configuration Acceptance

The implementation must demonstrate:

1. legacy `v1` config can be translated into one implicit extension backend when `wallet_backends`
   is absent
2. legacy top-level extension fields are rejected when `wallet_backends` is explicitly present
3. duplicate `backend_id` values fail config validation
4. invalid `approval_surface` for a backend kind fails config validation
5. invalid `local_account_dir` path fails config validation

## 11. Performance and Boundedness Acceptance

The implementation must demonstrate:

1. idle polling is bounded and does not busy-loop
2. account snapshot replacement is atomic
3. result retention remains bounded after phase-2 migration
4. one backend cannot resume another backend's presented request

At minimum, one automated test should prove that an empty `request.pullNext` path remains cheap and
stable under repeated polling.

## 12. Phase-2 Release Gate

The project is not ready for phase-2 implementation freeze unless:

1. every acceptance area above has at least one automated test or one manual verification record
2. schema migration from `v1` to `v2` has been exercised in tests
3. the compatibility path for current extension-backed `v1` behavior remains green

## 13. Phase-3 Additions

When explicit unlock flows are introduced later, the project must add acceptance coverage for:

1. `wallet_request_unlock`
2. unlock TTL expiry
3. unlock-state recovery after daemon restart
4. refusal to expose passwords over MCP or daemon transport even in unlock flows

When a future development-only backend such as `private_key_dev` is introduced, the project must
add separate acceptance coverage for channel gating and unattended signing restrictions.

## 14. Relationship to Other Documents

This document should be read together with:

- `docs/unified-wallet-coordinator-evolution.md`
- `docs/wallet-backend-local-socket-binding.md`
- `docs/wallet-backend-security-model.md`
- `docs/wallet-backend-persistence-and-schema.md`
