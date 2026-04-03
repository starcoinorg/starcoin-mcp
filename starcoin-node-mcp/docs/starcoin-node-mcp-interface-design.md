# Starcoin Node MCP Interface Design

## 1. Purpose

This document defines the interface design for `starcoin-node-mcp`, a chain-facing MCP server for Starcoin.

The design assumptions are:

- the MCP host is Claude Code, Codex, or a similar local MCP-capable host
- `starcoin-node-mcp` may connect to a local node or a remote node
- `starcoin-node-mcp` does not hold private keys
- signing is delegated to a separate wallet-facing system such as `starmask-mcp`
- the first conforming implementation is written in Rust

The main goal is to give MCP hosts a stable task-oriented interface for:

- querying chain state
- querying node health
- resolving ABI and contract metadata
- preparing unsigned transactions
- simulating unsigned transactions
- submitting already signed transactions

Companion documents for this interface include:

- `starcoin-node-mcp/docs/security-model.md`
- `starcoin-node-mcp/docs/deployment-model.md`
- `starcoin-node-mcp/docs/configuration.md`
- `starcoin-node-mcp/docs/rpc-adapter-design.md`
- `starcoin-node-mcp/docs/rust-implementation-strategy.md`
- `starcoin-node-mcp/docs/design-closure-plan.md`
- `starcoin-node-mcp/docs/testing-and-acceptance.md`

## 2. Design Principles

1. `starcoin-node-mcp` is read-first.
2. Signing is out of scope.
3. The interface should be task-oriented, not a 1:1 mirror of JSON-RPC methods.
4. VM2-first semantics should be preferred in user-facing tools.
5. Unsafe node-management operations should not be enabled in the initial release.
6. Structured outputs must be optimized for MCP host orchestration.
7. Host-triggered work should stay bounded by default rather than creating unbounded queries, watch loops, or payload handling.
8. The first implementation should preserve explicit Rust boundaries among MCP transport, RPC adaptation, and core transaction logic.

## 3. Runtime Topology

```mermaid
flowchart LR
    H["MCP Host"] --> N["starcoin-node-mcp"]
    N --> R["Starcoin RPC Endpoint"]
```

### 3.1 Deployment Modes

#### Read-only mode

- intended for query, ABI, state, and simulation workflows
- no transaction submission tools enabled

#### Transaction mode

- includes all read-only tools
- includes unsigned transaction preparation
- includes signed transaction submission and transaction watch tools

#### Admin mode

- optional future mode
- may include sync, peer, and service diagnostics
- dangerous management tools remain opt-in

## 4. Responsibilities

`starcoin-node-mcp` is responsible for:

- normalizing Starcoin RPC into a smaller MCP tool surface
- preparing raw unsigned transactions with filled chain context
- simulating transactions before signing
- submitting signed transactions
- returning structured chain and node results

It is not responsible for:

- wallet account storage
- unlocking accounts
- message signing
- transaction signing

## 5. Tool Design Strategy

The tool surface should map to user tasks instead of raw RPC names.

For example:

- expose `prepare_transfer`
- not `account.sign_txn_request`

- expose `get_account_overview`
- not raw combinations of `state.get_resource`, `txpool.next_sequence_number`, and `chain.info`

## 6. MCP Tool Surface

### 6.0 Resource Governance

The tool surface should expose bounded work rather than raw unbounded access to the backing node.

Rules:

1. tools that accept caller-provided counts, limits, or time budgets should clamp those inputs to configuration-defined safe bounds
2. when a tool clamps a caller-provided bound, the result should expose the effective applied value
3. `rate_limited` means a local concurrency or request-budget guard fired before outbound RPC side effects occurred
4. `payload_too_large` means the request payload exceeded local policy and was rejected before decode or RPC submission
5. watch loops and blocking waits should remain opt-in and bounded by local policy

### 6.1 Chain Context and Health

#### `chain_status`

##### Purpose

Return the current high-level chain context.

##### Input

- no required parameters

##### Output

- `network`
- `chain_id`
- `genesis_hash`
- `head_block_number`
- `head_block_hash`
- `head_state_root`
- `now_seconds`
- `peer_count`
- `sync_status`

#### `node_health`

##### Purpose

Return a summarized node health snapshot.

##### Input

- no required parameters

##### Output

- `node_available`
- `node_info`
- `sync`
- `peers_summary`
- `txpool_summary`
- `warnings`

### 6.2 Block and Transaction Queries

#### `get_block`

##### Purpose

Get a block by hash or block number.

##### Input

- one of:
  - `block_hash`
  - `block_number`
- `decode`: boolean, default `true`
- `include_raw`: boolean, default `false`

##### Output

- `block`
- `source`
  - `hash`
  - `number`

#### `list_blocks`

##### Purpose

Get a range-like recent block listing.

##### Input

- `from_block_number`: optional
- `count`
- `reverse`: boolean, default `true`

##### Output

- `blocks`
- `effective_count`

#### `get_transaction`

##### Purpose

Get a transaction and its execution context by transaction hash.

##### Input

- `txn_hash`
- `include_events`: boolean, default `true`
- `decode`: boolean, default `true`

##### Output

- `transaction`
- `transaction_info`
- `events`
- `status_summary`

#### `watch_transaction`

##### Purpose

Poll a transaction until the requested confirmation depth or timeout.

##### Input

- `txn_hash`
- `timeout_seconds`
- `poll_interval_seconds`
- `min_confirmed_blocks`: optional, default `2`

`min_confirmed_blocks` counts the inclusion block itself. The default value `2` therefore means:

- the inclusion block
- plus at least 1 additional block observed on top of it

##### Output

- `txn_hash`
- `found`
- `confirmed`
- `effective_timeout_seconds`
- `effective_poll_interval_seconds`
- `effective_min_confirmed_blocks`
- `confirmed_blocks`
- `inclusion_block_number`
- `transaction_info`
- `events`
- `status_summary`

`confirmed` means the requested confirmation depth was satisfied. `status_summary.confirmed`
continues to mean the transaction has chain-side transaction info, which is the inclusion signal
before depth policy is applied.

### 6.3 Events

#### `get_events`

##### Purpose

Query events by filter.

##### Input

- `from_block`
- `to_block`
- `event_keys`: optional
- `addresses`: optional
- `type_tags`: optional
- `limit`
- `decode`: boolean, default `true`

##### Output

- `events`
- `matched_count`
- `effective_limit`

### 6.4 Account and State Queries

#### `get_account_overview`

##### Purpose

Return a task-oriented summary of an account.

##### Input

- `address`
- `include_resources`: boolean, default `false`
- `include_modules`: boolean, default `false`
- `resource_limit`: optional
- `module_limit`: optional

##### Output

- `address`
- `onchain_exists`
- `sequence_number`
- `balances`
- `accepted_tokens`
- `resources`: optional
- `modules`: optional
- `applied_resource_limit`: optional
- `applied_module_limit`: optional
- `next_sequence_number_hint`

#### `list_resources`

##### Purpose

List resources for an account with optional type filter.

##### Input

- `address`
- `resource_type`: optional
- `start_index`: optional
- `max_size`: optional
- `decode`: boolean, default `true`
- `block_number`: optional

##### Output

- `address`
- `state_root`
- `resources`
- `effective_max_size`

#### `list_modules`

##### Purpose

List modules for an account and optionally resolve ABI.

##### Input

- `address`
- `resolve_abi`: boolean, default `true`
- `max_size`: optional
- `block_number`: optional

##### Output

- `address`
- `state_root`
- `modules`
- `effective_max_size`

### 6.5 ABI and Contract Introspection

#### `resolve_function_abi`

##### Purpose

Resolve a function ABI from a fully qualified function id.

##### Input

- `function_id`

##### Output

- `function_abi`

#### `resolve_struct_abi`

##### Purpose

Resolve a struct ABI from a fully qualified struct tag.

##### Input

- `struct_tag`

##### Output

- `struct_abi`

#### `resolve_module_abi`

##### Purpose

Resolve a module ABI from a module id.

##### Input

- `module_id`

##### Output

- `module_abi`

#### `call_view_function`

##### Purpose

Execute a contract call without changing chain state.

##### Input

- `function_id`
- `type_args`
- `args`

##### Output

- `return_values`: optional raw return values when the underlying RPC exposes a stable raw representation
- `decoded_return_values`: decoded JSON return values; this is the canonical output in the first Rust implementation

### 6.6 Transaction Preparation

These tools produce unsigned transactions for a separate signer.

The canonical return shape should align with `shared/schemas/unsigned-transaction-envelope.schema.json`.

Simulation behavior:

- if `sender_public_key` is provided, the tool should attempt simulation before returning
- if `sender_public_key` is not provided, the tool may still prepare the unsigned transaction but must return `simulation_status = skipped_missing_public_key`

#### `prepare_transfer`

##### Purpose

Prepare an unsigned transfer transaction.

##### Input

- `sender`
- `sender_public_key`: optional
- `receiver`
- `amount`
- `token_code`: optional, default STC
- `sequence_number`: optional
- `max_gas_amount`: optional
- `gas_unit_price`: optional
- `expiration_time_secs`: optional
- `gas_token`: optional

##### Output

- envelope conforming to `shared/schemas/unsigned-transaction-envelope.schema.json`
- `transaction_kind`: `transfer`
- `simulation_status`
- `simulation`
- `next_action`
  - usually `sign_transaction`
  - `get_public_key_then_simulate_or_sign` when simulation could not run because the public key was not provided

#### `prepare_contract_call`

##### Purpose

Prepare an unsigned script-function or contract-call transaction.

##### Input

- `sender`
- `sender_public_key`: optional
- `function_id`
- `type_args`
- `args`
- `sequence_number`: optional
- `max_gas_amount`: optional
- `gas_unit_price`: optional
- `expiration_time_secs`: optional
- `gas_token`: optional

##### Output

- envelope conforming to `shared/schemas/unsigned-transaction-envelope.schema.json`
- `transaction_kind`: `contract_call`
- `simulation_status`
- `simulation`
- `next_action`

#### `prepare_publish_package`

##### Purpose

Prepare an unsigned package publish transaction.

##### Input

- `sender`
- `sender_public_key`: optional
- `package_bcs_hex`
- `sequence_number`: optional
- `max_gas_amount`: optional
- `gas_unit_price`: optional
- `expiration_time_secs`: optional
- `gas_token`: optional

Payloads above the configured size ceiling must be rejected locally with `payload_too_large`.

##### Output

- envelope conforming to `shared/schemas/unsigned-transaction-envelope.schema.json`
- `transaction_kind`: `publish_package`
- `simulation_status`
- `simulation`
- `next_action`

### 6.7 Simulation

#### `simulate_raw_transaction`

##### Purpose

Simulate a prepared raw transaction before signing.

This tool is the canonical follow-up when a preparation result returned `simulation_status = skipped_missing_public_key`.

##### Input

- `raw_txn_bcs_hex`
- `sender_public_key`

##### Output

- `simulation`
- `executed`
- `vm_status`
- `gas_used`
- `events`
- `write_set_summary`

### 6.8 Submission

#### `submit_signed_transaction`

##### Purpose

Submit an already signed transaction.

##### Input

- `signed_txn_bcs_hex`
- `prepared_chain_context`: the `chain_context` returned by the preparation result that produced the signed transaction
- `blocking`: boolean, default `false`
- `timeout_seconds`: optional when `blocking = true`
- `min_confirmed_blocks`: optional when `blocking = true`, default `2`

When `allow_submit_without_prior_simulation = false`, the implementation should require a local chain-side preparation or simulation record for the raw transaction and fail closed otherwise.

##### Output

- `txn_hash`
- `submission_state`
  - `accepted`
  - `unknown`
  - `rejected`
- `submitted`
- `prepared_simulation_status`: optional, echoed from the node-side local preparation or simulation record when present
- `error_code`: optional
- `effective_timeout_seconds`: optional
- `next_action`
  - `watch_transaction`
  - `reconcile_by_txn_hash`
  - `reprepare_then_resign`
- `watch_result`: optional

When `blocking = true`, any built-in wait should reuse `watch_transaction` semantics and return the
same confirmation-depth metadata through `watch_result`.

## 7. Result Semantics

Outputs should be stable, structured, and tool-friendly.

### 7.1 Preparation Results

All preparation tools should return:

- the raw unsigned transaction in BCS hex
- a structured transaction view
- a human-readable transaction summary
- a `chain_context` snapshot describing the target endpoint and chain identity
- `prepared_at`
- `sequence_number_source`
- `gas_unit_price_source`
- `simulation_status`
- simulation output when available
- a `next_action` field indicating the expected wallet step

If `sender_public_key` is unavailable during preparation:

- return `simulation_status = skipped_missing_public_key`
- omit or null the `simulation` field
- set `next_action = get_public_key_then_simulate_or_sign`

### 7.2 Rust Boundary DTOs

Because the first conforming implementation is Rust, public MCP inputs and outputs should be defined as explicit `serde` DTOs rather than assembled from untyped maps.

Required Rust-oriented rules:

- `simulation_status`, `next_action`, and `submission_state` should map to Rust enums serialized in stable snake_case values
- large payloads such as raw transactions, simulation details, and decoded summaries should be carried by dedicated Rust structs rather than `serde_json::Value` blobs where avoidable
- DTOs used at the MCP boundary should remain separate from core orchestration types so domain logic is not coupled to transport serialization

### 7.3 Query Results

Query tools should prefer concise summaries plus raw structured objects, rather than only raw RPC payloads.

Health and transaction-adjacent query results should also make chain context explicit enough for the host to reason about endpoint identity, lag, and retry behavior.

### 7.4 Resource Governance Results

Whenever the caller supplies a query size or time budget that is clamped by local policy, the result should return the effective applied value.

Rules:

- `watch_transaction` should return `effective_timeout_seconds` and `effective_poll_interval_seconds`
- `watch_transaction` should also return `effective_min_confirmed_blocks`
- list-like query tools should return `effective_count`, `effective_limit`, `effective_max_size`, or similar applied-limit metadata when caller-provided values were clamped
- `rate_limited` should mean the local process rejected the request before outbound RPC side effects, so the host may retry later with backoff
- `payload_too_large` should mean the request was rejected before decode or endpoint contact and requires a smaller payload or an explicit config change

### 7.5 Submission Results

Submission tools should compute and return `txn_hash` even before the endpoint confirms acceptance.

When a blocking submission includes a watch result, that nested result should be interpreted the
same way as a direct `watch_transaction` call:

- `status_summary.confirmed` means the transaction has been included on chain
- top-level `confirmed` means the requested `min_confirmed_blocks` threshold was reached
- `confirmed_blocks` means `head_block_number - inclusion_block_number + 1`

Recommended interpretation:

- `submission_state = accepted`
  - the endpoint explicitly accepted the transaction
  - `submitted = true`
- `submission_state = unknown`
  - the endpoint may already have received the transaction, but the caller did not receive a definitive response
  - `submitted = false`
  - the host should reconcile by `txn_hash` before any retry
  - if reconciliation still times out, the host should preserve unresolved state and avoid automatic blind re-submission
- `submission_state = rejected`
  - the endpoint explicitly rejected the transaction
  - `submitted = false`
  - `transaction_expired` and `sequence_number_stale` require fresh preparation and fresh signing

### 7.6 Errors

Errors should reuse shared repository-level error codes where applicable.

Likely shared errors:

- `node_unavailable`
- `rpc_unavailable`
- `invalid_chain_context`
- `submission_unknown`
- `simulation_failed`
- `submission_failed`
- `transaction_expired`
- `sequence_number_stale`
- `rate_limited`
- `payload_too_large`
- `unsupported_operation`

Project-local errors may include:

- `missing_sender`
- `missing_public_key`
- `invalid_package_payload`
- `transaction_not_found`

## 8. Internal Adapter Layer

`starcoin-node-mcp` may internally use Starcoin JSON-RPC clients, but the MCP surface should remain stable even if the underlying RPC method set evolves.

Recommended internal modules:

- `chain_service`
- `state_service`
- `contract_service`
- `tx_service`
- `node_service`
- `mapper`
  - maps RPC responses to MCP-friendly outputs

The RPC surface classification and normalization rules for this layer are defined in `starcoin-node-mcp/docs/rpc-adapter-design.md`.

In the Rust implementation, these modules should be separate crates or crate-local modules with `TryFrom` or mapper boundaries rather than direct tool-handler access to RPC response structs.

## 9. Signing Boundary

`starcoin-node-mcp` must not:

- unlock local accounts
- call account-signing RPC as part of the default design
- access wallet private key material

Instead, it should integrate with wallet-facing tools by returning unsigned transactions.

The intended pairing is:

- `starcoin-node-mcp.prepare_*`
- optional `starcoin-node-mcp.simulate_raw_transaction`
- `starmask-mcp.wallet_request_sign_transaction`
- `starcoin-node-mcp.submit_signed_transaction`

## 10. Deployment Model

The canonical deployment and profile rules are defined in `starcoin-node-mcp/docs/deployment-model.md`.

Summary:

- `read_only` is the default profile
- `transaction` mode is explicit opt-in and requires chain pin validation
- `admin` mode remains out of scope for the first release

## 11. Safety Constraints

1. The initial design should exclude destructive node-management tools.
2. Admin capabilities should be separated from default user-facing capabilities.
3. Preparation tools should simulate before returning a signing payload whenever `sender_public_key` is available.
4. The returned transaction summary should be descriptive but not treated as the security source of truth by the wallet.
5. `submit_signed_transaction` should derive `txn_hash` locally and must not allow blind re-submission when the prior submission outcome is uncertain.
6. List-style queries, watch loops, and package-publish payloads should be bounded by local policy and fail fast when that policy is exceeded.
7. When prior simulation is required by policy, `submit_signed_transaction` should rely on a local preparation or simulation attestation rather than host-asserted metadata.

## 12. Relationship to Repository Structure

Repository-wide materials:

- shared error codes: `shared/protocol/error-codes.md`
- shared async request lifecycle: `shared/protocol/request-lifecycle.md`

Project-specific materials:

- `starcoin-node-mcp/docs/design-closure-plan.md`
- `starcoin-node-mcp/docs/security-model.md`
- `starcoin-node-mcp/docs/deployment-model.md`
- `starcoin-node-mcp/docs/configuration.md`
- `starcoin-node-mcp/docs/rpc-adapter-design.md`
- `starcoin-node-mcp/docs/rust-implementation-strategy.md`
- `starcoin-node-mcp/docs/testing-and-acceptance.md`
- this interface design

## 13. First-Release Decisions

1. Preparation tools attempt simulation whenever `sender_public_key` is available.
2. `call_view_function` and `simulate_raw_transaction` remain version-neutral MCP tools; VM differences are handled by the adapter layer.
3. Transaction summaries may include normalized fields helpful to the host or wallet flow, but they remain descriptive hints rather than wallet security truth.
4. Read-only and transaction-enabled behavior ship as configuration profiles of one binary rather than separate binaries.
5. Transaction mode should validate `genesis_hash` in addition to `chain_id` and network whenever that identity is available.
6. Uncertain submission outcomes are reconciled by transaction hash before any retry.
7. Caller-provided bounds are clamped to configuration-defined safe limits, and local overload returns `rate_limited` before outbound RPC side effects occur.
