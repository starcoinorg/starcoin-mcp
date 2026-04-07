# Starcoin Node MCP RPC Adapter Design

## Purpose

This document defines how `starcoin-node-mcp` should map the Starcoin RPC surface into a stable MCP tool interface.

Repository status note: the workspace no longer ships an in-tree MCP adapter crate. The adapter
boundary described here remains the intended contract between shared Rust libraries and any future
external MCP transport.

The adapter layer exists to keep:

- raw RPC method names
- VM-version differences
- response-shape drift
- endpoint capability probing

out of the MCP-facing tool contract.

## Design Goals

The adapter layer should optimize for:

1. one stable task-oriented MCP surface
2. VM2-first behavior without exposing VM-specific tool names
3. clear capability gating between `read_only` and `transaction`
4. deterministic chain-context handling for transaction flows
5. minimal leakage of Starcoin JSON-RPC details into MCP hosts
6. bounded host-driven work with predictable local overload behavior

## Adapter Layers

Recommended internal modules:

- `endpoint_probe`
  - validates connectivity, chain identity, and method availability
- `chain_service`
  - maps chain and node status queries
- `state_service`
  - maps account, resource, and module lookups
- `contract_service`
  - maps ABI resolution and view execution
- `tx_builder_service`
  - derives sequence, gas defaults, and local raw transaction bytes
- `simulation_service`
  - runs dry-run calls and normalizes output
- `submission_service`
  - submits signed transactions and watches status
- `mapper`
  - converts RPC-native views into MCP result shapes

## Rust Trait Boundaries

In the Rust implementation, this adapter layer should be expressed through explicit trait and conversion boundaries instead of direct JSON-RPC calls from tool handlers.

Recommended Rust ownership model:

- the CLI or any future MCP adapter should depend on typed adapter traits, not raw RPC method names
- the adapter crate should own endpoint probing, RPC client setup, and VM-specific branching
- domain services should depend on typed adapter traits exposed as `Send + Sync` Rust interfaces
- RPC-native views should be converted into stable domain structs through `TryFrom` or dedicated mapper functions before host-facing serialization

The goal is to keep Rust ownership aligned with the design boundary:

- `starcoin-node-mcp-server`
  - historical MCP transport boundary; not currently shipped in-tree
- `starcoin-node-mcp-core`
  - policy and orchestration
- `starcoin-node-mcp-rpc`
  - RPC transport, probing, and view mapping

## Capability Discovery

Startup probing should classify endpoint support into three buckets:

### Required for `read_only`

- `chain.info`
- `node.status`
- `node.info`
- one block lookup path
- one transaction lookup path
- state or contract methods required by the enabled tools

### Required for `transaction`

All `read_only` requirements plus:

- txpool gas price lookup
- next-sequence-number lookup
- raw-transaction dry run
- signed transaction submission

### Optional Enhancements

- `sync.status` for richer health reporting
- `node.peers` for peer summaries
- VM2-specific decode methods for higher-fidelity outputs

If an optional method is missing, the tool result may degrade in detail but should not silently change the configured capability profile.

## RPC Surface Classification

The MCP tool surface remains version-neutral, but the backing Starcoin JSON-RPC surface does not.

The adapter should distinguish three categories:

1. shared RPC
2. VM1 RPC surface
3. VM2 RPC surface

Shared RPC methods are not VM-specific and should be treated as common infrastructure methods.
Examples include:

- `chain.info`
- `node.status`
- `node.info`
- `node.peers`
- `sync.status`
- `chain.get_block_by_hash`
- `chain.get_block_by_number`
- `chain.get_blocks_by_number`
- `chain.get_events`
- `txpool.gas_price`

VM1 RPC surface methods include names such as:

- `chain.get_transaction`
- `chain.get_transaction_info`
- `chain.get_events_by_txn_hash`
- `state.get_account_state`
- `state.list_resource`
- `state.list_code`
- `contract.resolve_function`
- `contract.resolve_module`
- `contract.resolve_struct`
- `contract.call_v2`
- `contract.dry_run_raw`
- `txpool.next_sequence_number`
- `txpool.submit_hex_transaction`

VM2 RPC surface methods include names such as:

- `chain.get_transaction2`
- `chain.get_transaction_info2`
- `chain.get_events_by_txn_hash2`
- `state2.list_resource`
- `state2.list_code`
- `state2.get_state_root`
- `state2.get_resource`
- `contract2.resolve_function`
- `contract2.resolve_module`
- `contract2.resolve_struct`
- `contract2.call_v2`
- `contract2.dry_run_raw`
- `txpool.next_sequence_number2`
- `txpool.submit_hex_transaction2`

These are separate RPC surfaces. The adapter may probe both and choose one per call, but it must
not imply semantic compatibility between VM1 and VM2 transaction payloads, token codes, resource
types, or contract semantics.

## `vm_profile` Routing Rules

Rules:

1. `vm_profile = auto` prefers VM2 RPC methods where the adapter supports both VM1 and VM2 names
2. `vm_profile = vm1_only` fails startup or tool gating if the endpoint lacks required VM1 paths
3. `vm_profile = vm2_only` fails startup or tool gating if the endpoint lacks required VM2 paths
4. shared RPC methods are profile-neutral
5. query tools may degrade more gracefully than transaction tools
6. transaction tools must fail closed when the endpoint cannot support the selected RPC surface

In Rust, this routing should be represented by typed enums such as a surface-selection or capability variant, not by scattered boolean flags.

## Tool-to-RPC Mapping

### Chain Context and Health

- `chain_status`
  - `chain.info`
  - `node.info`
  - `node.peers`
  - `sync.status` when available
- `node_health`
  - `node.status`
  - `node.info`
  - `sync.status`
  - `txpool.state` when available

### Block and Transaction Queries

- `get_block`
  - `chain.get_block_by_hash`
  - `chain.get_block_by_number`
- `list_blocks`
  - `chain.get_blocks_by_number`
- `get_transaction`
  - VM2 RPC surface:
    - `chain.get_transaction2`
    - `chain.get_transaction_info2`
    - `chain.get_events_by_txn_hash2`
  - VM1 RPC surface:
    - `chain.get_transaction`
    - `chain.get_transaction_info`
    - `chain.get_events_by_txn_hash`
- `watch_transaction`
  - repeated `get_transaction` and transaction-info lookups until terminal or timeout, subject to local watch-permit limits

### State and ABI

- `get_account_overview`
  - VM1 RPC surface:
    - `state.get_account_state`
    - `state.list_resource`
    - `state.list_code`
    - `txpool.next_sequence_number`
  - VM2 RPC surface:
    - `state2.list_resource`
    - `state2.list_code`
    - `txpool.next_sequence_number2`
- `list_resources`
  - VM1 RPC surface: `state.list_resource`
  - VM2 RPC surface: `state2.list_resource`
- `list_modules`
  - VM1 RPC surface:
    - `state.list_code`
    - `contract.resolve_module`
  - VM2 RPC surface:
    - `state2.list_code`
    - `contract2.resolve_module`
- `resolve_function_abi`
  - VM1 RPC surface: `contract.resolve_function`
  - VM2 RPC surface: `contract2.resolve_function`
- `resolve_struct_abi`
  - VM1 RPC surface: `contract.resolve_struct`
  - VM2 RPC surface: `contract2.resolve_struct`
- `resolve_module_abi`
  - VM1 RPC surface: `contract.resolve_module`
  - VM2 RPC surface: `contract2.resolve_module`
- `call_view_function`
  - VM1 RPC surface: `contract.call_v2`
  - VM2 RPC surface: `contract2.call_v2`

For account and resource reads, the adapter may synthesize or repair a summary shape from resource
listing results. That is response-shape normalization, not a VM semantic bridge.

### Preparation and Simulation

- `prepare_transfer`
  - shared RPC:
    - `chain.info`
    - `txpool.gas_price`
  - selected VM1 or VM2 RPC surface for account and sequence queries
  - local raw transaction construction
- `prepare_contract_call`
  - same as above plus selected VM1 or VM2 ABI resolution when summary enrichment is desired
- `prepare_publish_package`
  - same as above
- `simulate_raw_transaction`
  - VM1 RPC surface: `contract.dry_run_raw`
  - VM2 RPC surface: `contract2.dry_run_raw`

### Submission

- `submit_signed_transaction`
  - shared RPC:
    - `chain.info` to re-validate pinned chain identity immediately before submission
  - VM1 RPC surface: `txpool.submit_hex_transaction`
  - VM2 RPC surface: `txpool.submit_hex_transaction2`
  - return `invalid_chain_context` if the pre-submit chain re-check fails

## Deterministic Transaction Preparation

Preparation tools should not simply mirror node RPC calls.

Rules:

1. derive the sender sequence number from documented sources
2. choose the maximum of on-chain sequence and txpool next sequence when both are available
3. record which source determined the final sequence
4. derive gas defaults from explicit config or txpool gas price
5. build raw transaction bytes locally using Starcoin transaction types

The returned envelope should include additional metadata beyond the shared schema when useful, such as:

- `chain_context`
- `prepared_at`
- `sequence_number_source`
- `gas_unit_price_source`

The `chain_context` snapshot should include:

- `chain_id`
- `network`
- `genesis_hash`
- `head_block_hash`
- `head_block_number`
- `observed_at`

## Request Shaping and Backpressure

The adapter layer should not fetch or poll more data than the MCP contract actually allows.

Rules:

1. list-style query bounds must be normalized before any RPC request is constructed
2. the adapter should request only the effective bounded page or window from the endpoint rather than fetching unbounded data and truncating locally
3. publish-package payload size must be checked against local policy before decode or dry-run work begins
4. watch and reconciliation loops should acquire local permits before polling starts
5. if local request budgets are exhausted, the adapter should surface `rate_limited` before outbound RPC side effects occur
6. repeated chain-context and ABI fetches may use bounded in-memory caches within TTL

## Submission Reconciliation

The adapter layer should make uncertain submission outcomes explicit.

Rules:

1. re-check pinned chain identity with a fresh `chain.info` read before calling the submission RPC
2. if the pre-submit chain re-check fails, abort with `invalid_chain_context` and do not contact txpool
3. compute `txn_hash` locally before calling the submission RPC
4. if the endpoint explicitly accepts the transaction, return `submission_state = accepted`
5. if the endpoint explicitly rejects the transaction as expired or stale, map to `transaction_expired` or `sequence_number_stale`
6. if transport breaks after the submission attempt may already have reached the endpoint, return `submission_state = unknown` and `submission_unknown`
7. on `submission_state = unknown`, the host should reconcile by `txn_hash` through `get_transaction` or `watch_transaction` before any retry
8. on `transaction_expired` or `sequence_number_stale`, the host should restart from fresh preparation and fresh signing
9. if reconciliation remains unresolved after timeout, preserve `submission_unknown` state and require explicit operator action instead of automatic blind re-submission

## Result Normalization Rules

Query results should prefer:

- concise summaries
- stable field names
- optional raw payloads when necessary

The adapter should avoid returning large raw RPC blobs by default when a narrower structured view is sufficient.

In Rust terms, host-facing outputs should come from dedicated `serde` DTOs rather than serializing RPC client structs directly.

## Error Mapping Rules

Recommended mapping:

- transport connection failure:
  - `node_unavailable`
- endpoint timeout or upstream overload:
  - `rpc_unavailable`
- local request-budget or concurrency exhaustion:
  - `rate_limited`
- configured chain pin mismatch:
  - `invalid_chain_context`
- submission may have reached the endpoint but the caller did not receive a definitive response:
  - `submission_unknown`
- dry run returns failed VM status:
  - `simulation_failed`
- signed submission rejected by txpool:
  - `submission_failed`
- endpoint rejects a signed transaction because its expiration has passed:
  - `transaction_expired`
- endpoint rejects a signed transaction because its sequence number is stale:
  - `sequence_number_stale`
- required method missing for the selected profile:
  - `unsupported_operation`
- request payload exceeds local safety ceiling before endpoint contact:
  - `payload_too_large`

Project-local errors may still exist for:

- `transaction_not_found`
- `missing_public_key`
- `invalid_package_payload`

## Non-Goals

The adapter design does not require:

- a 1:1 mirror of all Starcoin RPC methods
- separate MCP tool names for every VM generation
- exposing raw JSON-RPC request construction to MCP hosts
