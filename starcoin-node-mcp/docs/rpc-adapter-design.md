# Starcoin Node MCP RPC Adapter Design

## Purpose

This document defines how `starcoin-node-mcp` should map the Starcoin RPC surface into a stable MCP tool interface.

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

## Capability Discovery

Startup probing should classify endpoint support into three buckets:

### Required for `read_only`

- `chain.info`
- `node.status`
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

## VM Compatibility Strategy

The MCP tool surface remains version-neutral.

Rules:

1. `vm_profile = auto` prefers VM2-compatible methods when available
2. `vm_profile = vm2_only` fails startup if the endpoint lacks required VM2 paths
3. `vm_profile = legacy_compatible` may use older RPC methods for read-only flows when needed
4. query tools may degrade more gracefully than transaction tools
5. transaction tools must fail closed when the endpoint cannot support the configured VM profile

The first release should treat VM2 as the preferred semantic baseline for transaction preparation and simulation.

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
  - `chain.get_transaction2` when available
  - fallback: `chain.get_transaction`
  - transaction info:
    - `chain.get_transaction_info2` when available
    - fallback: `chain.get_transaction_info`
  - events:
    - `chain.get_events_by_txn_hash2` when available
    - fallback: `chain.get_events_by_txn_hash`
- `watch_transaction`
  - repeated `get_transaction` and transaction-info lookups until terminal or timeout

### State and ABI

- `get_account_overview`
  - `state.get_account_state`
  - `state.list_resource`
  - `state.list_code`
  - `txpool.next_sequence_number` or `txpool.next_sequence_number2`
- `list_resources`
  - `state.list_resource`
- `list_modules`
  - `state.list_code`
  - `contract.resolve_module`
- `resolve_function_abi`
  - `contract.resolve_function`
- `resolve_struct_abi`
  - `contract.resolve_struct`
- `resolve_module_abi`
  - `contract.resolve_module`
- `call_view_function`
  - `contract.call_v2` when available
  - fallback: `contract.call`

### Preparation and Simulation

- `prepare_transfer`
  - `chain.info`
  - `txpool.gas_price`
  - account sequence from state plus txpool next sequence
  - local raw transaction construction
- `prepare_contract_call`
  - same as above plus `contract.resolve_function` when summary enrichment is desired
- `prepare_publish_package`
  - same as above
- `simulate_raw_transaction`
  - `contract.dry_run_raw`

### Submission

- `submit_signed_transaction`
  - `txpool.submit_transaction2` when available
  - fallback: `txpool.submit_transaction`

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

## Submission Reconciliation

The adapter layer should make uncertain submission outcomes explicit.

Rules:

1. compute `txn_hash` locally before calling the submission RPC
2. if the endpoint explicitly accepts the transaction, return `submission_state = accepted`
3. if the endpoint explicitly rejects the transaction as expired or stale, map to `transaction_expired` or `sequence_number_stale`
4. if transport breaks after the submission attempt may already have reached the endpoint, return `submission_state = unknown` and `submission_unknown`
5. on `submission_state = unknown`, the host should reconcile by `txn_hash` through `get_transaction` or `watch_transaction` before any retry
6. on `transaction_expired` or `sequence_number_stale`, the host should restart from fresh preparation and fresh signing
7. if reconciliation remains unresolved after timeout, preserve `submission_unknown` state and require explicit operator action instead of automatic blind re-submission

## Result Normalization Rules

Query results should prefer:

- concise summaries
- stable field names
- optional raw payloads when necessary

The adapter should avoid returning large raw RPC blobs by default when a narrower structured view is sufficient.

## Error Mapping Rules

Recommended mapping:

- transport connection failure:
  - `node_unavailable`
- endpoint timeout or upstream overload:
  - `rpc_unavailable`
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

Project-local errors may still exist for:

- `transaction_not_found`
- `missing_public_key`
- `invalid_package_payload`

## Non-Goals

The adapter design does not require:

- a 1:1 mirror of all Starcoin RPC methods
- separate MCP tool names for every VM generation
- exposing raw JSON-RPC request construction to MCP hosts
