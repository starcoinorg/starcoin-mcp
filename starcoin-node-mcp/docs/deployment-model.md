# Starcoin Node MCP Deployment Model

## Purpose

This document defines the deployment and runtime model for `starcoin-node-mcp`, the chain-facing
Starcoin node integration layer.

Repository status note: the in-tree `starcoin-node-mcp-server` adapter has been removed. The
current workspace ships libraries plus `starcoin-node-cli`; server-specific sections below remain
as design guidance for a future external adapter.

The scope of this document is:

- the local MCP host, such as Codex or Claude Code
- `starcoin-node-mcp`
- one configured Starcoin RPC endpoint

## Design Goal

The deployment model must give MCP hosts a stable local entrypoint for chain access while preserving two boundaries:

- the chain-facing server may prepare, simulate, and submit transactions
- the chain-facing server does not own wallet keys or signing authority

## Runtime Topology

### Local Node Profile

```mermaid
flowchart LR
    H["MCP Host"] --> M["starcoin-node-mcp (stdio)"]
    M --> R["Local Starcoin RPC Endpoint"]
```

### Remote Endpoint Profile

```mermaid
flowchart LR
    H["MCP Host"] --> M["starcoin-node-mcp (stdio)"]
    M --> R["Remote Starcoin RPC Endpoint"]
```

## Deployment Profiles

### `read_only`

This is the default profile.

Allowed responsibilities:

- chain and node status
- block, transaction, event, state, and ABI queries
- view-function execution
- simulation of already prepared raw transactions

Blocked responsibilities:

- unsigned transaction preparation
- signed transaction submission
- any future admin or node-management tool

### `transaction`

This profile includes everything in `read_only` and additionally allows:

- unsigned transaction preparation
- signed transaction submission
- transaction watch flows

Additional requirements:

- the endpoint must pass chain pin validation
- the endpoint should expose or be preconfigured with a trusted `genesis_hash`
- RPC capability probing must confirm the required preparation and submission methods
- remote endpoints should use secure transport unless a development override is explicitly enabled

### `admin`

This is a future profile and is out of scope for the first release.

## Process Responsibilities

### MCP Host

- launches `starcoin-node-mcp`
- selects the intended workflow
- coordinates with `starmask-mcp` when signing is required
- handles retries and user-facing explanation

### `starcoin-node-mcp`

- speaks MCP over stdio
- validates tool inputs
- probes endpoint health and capabilities
- normalizes Starcoin RPC responses into MCP-friendly results
- builds unsigned transaction bytes locally
- never holds wallet private keys

### Starcoin RPC Endpoint

- provides chain, state, contract, sync, and txpool data
- simulates raw transactions
- accepts signed transaction submission
- remains outside the wallet trust boundary

## Rust Runtime Realization

The first implementation is a single Rust process and should realize this deployment model with one Tokio runtime.

Recommended Rust runtime shape for the current workspace plus any future external adapter:

- `starcoin-node-cli` owns the current Tokio runtime for RPC IO and watch polling
- one process-global `Arc<AppContext>` should hold normalized config, endpoint capabilities, shared RPC clients, in-memory caches, and concurrency guards
- startup probes should complete before CLI command handling or any future MCP transport begins serving requests
- `watch_transaction` should use `tokio::time::interval` and `tokio::time::timeout` rather than ad hoc sleep loops
- bounded `tokio::sync::Semaphore` guards should protect watch loops and other expensive request classes from unbounded fan-out
- tool cancellation should follow the Rust async task boundary so abandoned host requests do not leave orphaned watch loops running indefinitely

For embedded integrations, another Rust binary may own the Tokio runtime and tracing
initialization, as long as it reuses the same `AppContext` bootstrap flow instead of
reimplementing core or RPC logic.

The first release should not require a separate Rust daemon or any cross-process coordinator for chain-side state.

## Startup Model

The normal startup sequence is:

1. the MCP host launches `starcoin-node-mcp`
2. `starcoin-node-mcp` loads configuration
3. configuration validation resolves the desired capability profile
4. the server establishes an RPC client session to the configured endpoint
5. startup probes fetch chain identity, including `chain_id`, network, and `genesis_hash`, plus endpoint health and supported capabilities
6. if transaction mode is enabled, chain pinning and submission capabilities are verified
7. if `read_only` mode is using explicit autodetect override without configured chain pins, the server emits a high-severity warning before serving tools
8. the server begins serving MCP tools

Startup must fail fast when:

- the endpoint cannot be reached
- the configured profile requires capabilities the endpoint does not expose
- transaction mode is enabled but chain pinning fails

## Steady-State Model

In steady state:

- `starcoin-node-mcp` may be short-lived or tied to one MCP host session
- no durable request state is required in the first release
- endpoint metadata and ABI results may be cached in memory for short periods
- transaction-adjacent tools must use a fresh chain-context check before returning or submitting payloads

In the Rust implementation, these steady-state invariants should be represented as typed in-memory state rather than mutable free-form maps keyed by raw strings.

The first release assumes one endpoint per process. A single `starcoin-node-mcp` instance should not switch among multiple endpoints mid-session.

## Backpressure and Resource Governance

The first release should use simple local backpressure rather than hidden background queues.

Rules:

1. caller-supplied `count`, `limit`, `resource_limit`, `max_size`, and blocking timeout inputs must be clamped to configuration-defined safe bounds before adapter calls begin
2. `prepare_publish_package` must validate package size against a configured byte ceiling before decode, simulation, or RPC submission
3. watch loops and other expensive operations should acquire local permits before work starts
4. if no permit is available, the request should fail fast with `rate_limited` instead of queuing unbounded work
5. `rate_limited` from local policy should occur before outbound RPC side effects and therefore should not imply uncertain chain state
6. repeated ABI or chain-status reads may use bounded in-memory caches within TTL instead of re-fetching on every tool call

## Endpoint Capability Model

The deployment model distinguishes:

- connectivity
- chain identity
- RPC method availability
- RPC surface classification

These are related but must not be collapsed into one health bit.

Rules:

1. `node.status` success alone is insufficient for transaction mode readiness.
2. Read-only profile may degrade when optional health methods such as `sync.status` are unavailable.
3. Transaction profile must fail closed when required dry-run, txpool, or submission methods are missing.
4. shared/vm1/vm2 RPC surface selection is checked by the adapter layer, not inferred only from user intent.

## Shutdown Model

### MCP Host Exit

- `starcoin-node-mcp` may exit immediately
- no chain-side durable work queue must be recovered later
- the host can relaunch the server and re-run normal startup probes

### Endpoint Disconnect

- inflight tools fail with `node_unavailable` or `rpc_unavailable`
- no local transaction state is persisted for automatic retry
- the host decides whether to retry after the endpoint becomes reachable again

## Recovery Rules

The deployment model requires the following recovery behavior:

1. endpoint reconnection triggers capability refresh before transaction tools are used again
2. a chain identity change invalidates transaction mode until startup checks succeed again
3. transient query failures do not change the configured capability profile
4. a failed signed-transaction submission call does not imply that the node definitively rejected the transaction unless the endpoint returned a structured submission error
5. if submission outcome is uncertain, the server returns `submission_unknown` together with a deterministic `txn_hash`, and the host must reconcile by hash before any retry
6. if the endpoint rejects a signed transaction as expired or stale, the host must restart from unsigned transaction preparation rather than re-submit the same signed bytes
7. if reconciliation by `txn_hash` remains unresolved after timeout, the host should persist the unresolved submission state and require explicit operator action instead of automatic blind re-submission

## Local and Remote Transport Requirements

The first implementation should support:

- local IPC, HTTP, or WebSocket endpoints for development and colocated deployments
- remote HTTPS or secure WebSocket endpoints for hosted nodes
- optional remote endpoint allowlisting or certificate-pinning configuration for transaction mode

The first release should avoid:

- unauthenticated public-network transaction endpoints by default
- automatic downgrade from secure remote transport to insecure remote transport

## Observability Requirements

The deployment model requires:

- clear startup diagnostics about endpoint URL, profile, and detected chain id
- clear startup diagnostics about detected `genesis_hash`
- structured logs for capability probe failure
- clear differentiation between connectivity failure and chain mismatch
- a host-visible warning path when the node is reachable but unhealthy or lagging
- a host-visible reconciliation hint when submission outcome is uncertain
- structured diagnostics when request inputs are clamped or rejected by local resource policy
- a high-severity startup warning when `read_only` runs with explicit chain autodetection instead of configured pins

## Non-Goals

This document does not define:

- subscription-based event streaming
- daemonized multi-host service mode
- multi-endpoint routing or consensus reads
- wallet approval UX
