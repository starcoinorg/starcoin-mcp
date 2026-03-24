# Starcoin Node MCP Testing and Acceptance

## Purpose

This document defines the minimum acceptance criteria before `starcoin-node-mcp` can be considered implementation-ready and release-ready.

## Test Areas

The first release must cover:

1. startup and capability probing
2. query and ABI correctness
3. transaction preparation and simulation correctness
4. submission and reconciliation behavior
5. security behavior
6. configuration safety

## Rust Test Layers

Recommended Rust test layout:

1. unit tests in `starcoin-node-mcp-core` for chain pinning, sequence derivation, and reconciliation policy
2. fixture-driven adapter tests for RPC capability classification and response normalization
3. integration tests for MCP tool outputs and shared-schema compatibility
4. end-to-end tests against one local or test RPC endpoint for preparation, simulation, submission, and watch flows

## Startup and Capability Acceptance

The implementation must demonstrate:

1. startup succeeds in `read_only` mode against a healthy local endpoint
2. startup succeeds in `transaction` mode only when required preparation and submission capabilities are present
3. startup fails safely on `chain_id` mismatch
4. startup fails safely on network mismatch
5. remote `transaction` mode fails safely on `genesis_hash` mismatch when genesis matching is required
6. capability refresh happens after endpoint reconnect before transaction tools are re-enabled

## Query and ABI Acceptance

The implementation must demonstrate:

1. `chain_status` returns `chain_id`, `network`, and `genesis_hash`
2. `node_health` distinguishes connectivity failure from lagging or unhealthy endpoint states
3. `resolve_function_abi`, `resolve_struct_abi`, and `resolve_module_abi` return stable normalized outputs
4. `call_view_function` remains version-neutral even when the underlying endpoint uses different VM-specific RPC methods

## Preparation and Simulation Acceptance

The implementation must demonstrate:

1. every preparation result conforms to `shared/schemas/unsigned-transaction-envelope.schema.json`
2. every preparation result includes `chain_context` and `prepared_at`
3. sequence-number derivation documents and returns the selected source
4. gas-price derivation documents and returns the selected source
5. preparation with `sender_public_key` attempts simulation before returning
6. preparation without `sender_public_key` returns `simulation_status = skipped_missing_public_key`
7. `simulate_raw_transaction` is the canonical follow-up after skipped simulation

## Submission and Reconciliation Acceptance

The implementation must demonstrate:

1. `submit_signed_transaction` derives `txn_hash` locally before contacting the endpoint
2. accepted submission returns `submission_state = accepted`
3. uncertain submission after transport loss returns `submission_state = unknown` and `submission_unknown`
4. `submission_state = unknown` leads to reconciliation by `txn_hash` before any retry
5. explicit expiry rejection maps to `transaction_expired`
6. explicit stale-sequence rejection maps to `sequence_number_stale`
7. `transaction_expired` and `sequence_number_stale` require fresh preparation and fresh signing instead of blind re-submit
8. unresolved reconciliation after timeout preserves `submission_unknown` state and blocks automatic blind re-submission

## Security Acceptance

The implementation must demonstrate:

1. chain-side tools cannot sign transactions or unlock accounts
2. insecure remote transaction mode is blocked unless explicitly overridden
3. endpoint credentials are redacted from logs
4. signed transaction bytes are not logged in full by default
5. `chain_context` values shown to the host are derived from probed endpoint identity, not host-supplied inputs
6. wallet-facing approval remains the security source of truth over host-side transaction summaries
7. blind re-submission after `submission_unknown` is blocked by policy

## Configuration Acceptance

The implementation must demonstrate:

1. missing `expected_chain_id` in transaction mode fails safely
2. missing `expected_network` in transaction mode fails safely
3. missing `expected_genesis_hash` in remote transaction mode fails safely when genesis matching is required
4. disallowed endpoint hosts are rejected when allowlisting is configured
5. unsafe timeout and TTL values are clamped
6. insecure remote transport without explicit override is rejected

## End-to-End Scenarios

The first release must pass these end-to-end scenarios:

1. read-only query flow against one healthy endpoint
2. prepare, simulate, sign through wallet, submit, and watch one transaction successfully
3. prepare without public key, simulate later, then sign and submit
4. reconcile a transaction after uncertain submission result
5. re-prepare and re-sign after expiry or stale sequence rejection
6. persist and surface an unresolved submission after reconciliation timeout without blind retry
7. fail safely on remote chain identity mismatch before any transaction tool is served

## Release Gate

The project is not ready for implementation freeze unless every document in:

- `starcoin-node-mcp/docs/design-closure-plan.md`

exists and the first-release policy decisions in that document remain unchanged.

The project is not ready for release unless every acceptance area above has at least one passing test or manual verification record.
