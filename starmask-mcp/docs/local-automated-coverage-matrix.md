# Starmask Local Automated Coverage Matrix

## Status

This note tracks local automated coverage for the current `v1` extension-backed Rust workspace.

It does not claim coverage for planned multi-backend features such as `local_account_dir` or
explicit unlock requests.

## Scope

Use this note together with:

- `docs/testing-and-acceptance.md`
- `docs/mcp-shim-coverage-matrix.md`
- `docs/mcp-shim-real-environment-runbook.md`

It answers:

1. which current acceptance areas are already backed by local automated tests
2. which remaining checks still require a live browser or extension environment

## Current Automated Coverage

### Protocol correctness

Covered locally by:

- `starmask-core`: idempotent retry returns the same `request_id`
- `starmask-core`: changed payload or signing target returns `idempotency_key_conflict`
- `starmask-core`: ambiguous routing returns `wallet_selection_required`
- `starmaskd`: daemon `protocol_version_mismatch` and shared-error propagation
- `starmask-native-host`: native bridge `protocol_version_mismatch`
- `starmaskd` transport tests: idempotent retry and idempotency conflict over the real Unix socket
  JSON-RPC server

### Lifecycle correctness

Covered locally by:

- `created -> dispatched -> pending_user_approval -> approved`
- `created -> dispatched -> pending_user_approval -> rejected`
- `created -> cancelled`
- `pending_user_approval -> cancelled`
- `dispatched -> created` on delivery-lease expiry
- non-terminal request expiry through maintenance
- locked-wallet failure path

Primary evidence:

- `crates/starmask-core/src/service.rs`

### Recovery and restart correctness

Covered locally by:

- host restart and status polling by `request_id`
- daemon restart with a `created` request
- daemon restart with a `dispatched` request
- daemon restart with a `pending_user_approval` request
- restart before `request.presented` by lease expiry and requeue
- restart after `request.presented` with same-instance resume
- no cross-instance redelivery after `request.presented`

Primary evidence:

- `crates/starmaskd/tests/recovery.rs`

### Result handling

Covered locally by:

- approved results are readable multiple times before retention expiry
- maintenance evicts expired result payloads
- terminal metadata remains available after payload eviction
- approved results persist across daemon restart until retention eviction

Primary evidence:

- `crates/starmask-core/src/service.rs`
- `crates/starmaskd/tests/recovery.rs`

### Native Messaging and daemon transport

Covered locally by:

- Native Messaging frame round trip
- truncated-header and oversize-frame rejection
- request/response mapping for `request.next`, `request.none`, `request.resolve`, and
  `request.reject`
- shared daemon errors mapped back to bridge errors with `reply_to`
- notification-state tracking for presented, resumed, resolved, and rejected requests
- local daemon client transport behavior, including oversize daemon response rejection
- real Unix socket daemon transport for register, create, get-status, allowlist reject, idempotent
  retry, and idempotency conflict
- daemon socket permission lockdown to `0700` parent directory and `0600` socket file

Primary evidence:

- `crates/starmask-native-host/src/framing.rs`
- `crates/starmask-native-host/src/bridge.rs`
- `crates/starmask-native-host/src/notify.rs`
- `crates/starmask-native-host/src/client.rs`
- `crates/starmaskd/tests/transport.rs`

### Configuration and diagnostics

Covered locally by:

- extension allowlist parsing, deduplication, and empty-entry rejection
- empty allowlist rejection
- default Native Messaging host naming
- request TTL clamp behavior
- missing database diagnosis in `starmaskctl`
- missing native host manifest diagnosis in `starmaskctl`
- manifest allowed-origin mismatch diagnosis in `starmaskctl`

Primary evidence:

- `crates/starmaskd/src/config.rs`
- `crates/starmask-core/src/service.rs`
- `crates/starmaskctl/src/main.rs`

## Real-Environment-Only Coverage

The following current acceptance evidence still requires a real browser, extension runtime, or MCP
Inspector session:

- MCP Inspector over stdio against the running `starmask-mcp`
- real Chrome or Chromium native host registration and manifest discovery
- approval UI rendering and state transitions such as `loading`, `ready`, `cancelled`, `expired`,
  and `unsupported`
- canonical payload rendering inside the real extension UI
- live browser disconnect and reconnect behavior as observed on screen

Use `docs/mcp-shim-real-environment-runbook.md` for those checks.

## Current Conclusion

For the currently implemented Rust crates, local automated coverage now covers the non-real-
environment protocol, lifecycle, recovery, transport, result-retention, and diagnostics flows
described in the current acceptance document.

Multi-backend features remain out of scope until the phase-2 design in
`docs/unified-wallet-coordinator-evolution.md` is implemented.
