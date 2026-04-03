# Starcoin Node Rust Implementation Strategy

## Purpose

This document defines how the required first `starcoin-node` implementation should be structured in Rust.

It translates the chain-side interface, deployment, configuration, and adapter documents into Rust-specific implementation constraints.

Repository status note: the in-tree `starcoin-node-server` crate has been removed. The current
workspace ships shared libraries plus `starcoin-node-cli`; server-specific guidance below should be
read as design guidance for any future external adapter.

## Language Requirement

The first conforming implementation is:

- `starcoin-node`: Rust

No alternative implementation language is in scope for the first release.

No separate daemon is required for the first release because the chain-facing server does not need durable asynchronous state.

## MCP SDK Selection

The first Rust implementation should prefer the official Rust MCP SDK from `modelcontextprotocol/rust-sdk`.

Recommended crate choice:

- `rmcp`

Scope rule:

- use `rmcp` only at the MCP stdio boundary
- do not let `rmcp` types leak into RPC adapters, domain services, or transaction builders

## Rust Design Goals

The Rust implementation should optimize for:

1. strong type boundaries between MCP DTOs, Starcoin RPC DTOs, and domain models
2. deterministic transaction preparation
3. explicit capability gating
4. small, replaceable RPC adapter layers
5. minimal unsafe surface
6. bounded async concurrency and explicit backpressure

## Workspace Shape

The first implementation should use one Cargo workspace with a small number of crates.

Recommended shape:

```text
starcoin-node/
  Cargo.toml
  crates/
    starcoin-node-types/
    starcoin-node-core/
    starcoin-node-rpc/
    starcoin-node-cli/
```

Recommended responsibilities:

- `starcoin-node-types`
  - public DTOs
  - ids, enums, config structs, error code mapping
- `starcoin-node-core`
  - capability gating
  - chain-context validation
  - preparation and simulation orchestration
  - transaction summary generation
- `starcoin-node-rpc`
  - endpoint probing
  - Starcoin RPC client abstraction
  - shared/vm1/vm2 RPC surface routing
  - RPC-native view mapping
- `starcoin-node-cli`
  - process startup and config loading
  - command dispatch into typed app services
  - tracing and runtime bootstrap for the current executable surface
- any future MCP adapter crate
  - `rmcp` integration
  - tool registration
  - optional embeddable library entrypoints for host binaries that want to reuse the same MCP adapter in-process

## Runtime Model

`starcoin-node` should use Tokio because it needs:

- stdio MCP transport
- RPC IO
- watch-loop polling
- bounded in-memory caches

Recommended model:

1. `starcoin-node-cli` receives command requests
2. CLI handlers deserialize boundary DTOs
3. handlers call typed domain services in `starcoin-node-core`
4. core services use trait-based adapters from `starcoin-node-rpc`
5. results are mapped back into JSON command outputs

The standalone binary should own runtime setup and config loading. If an MCP adapter is added back,
it may expose a small library facade so another Rust binary can bootstrap config, initialize
tracing, and call the same stdio MCP adapter without copying handler wiring.

Recommended library-facing surface:

- `AppContext::bootstrap(config)`
- CLI-facing helpers should keep DTO decoding at the executable boundary
- any future MCP adapter should expose its own `serve_stdio(app)` helper

The first implementation does not need a dedicated coordinator task or durable state machine.

Runtime guardrails should include:

- `tokio::sync::Semaphore` or equivalent bounded guards for watch loops and other expensive operations
- no hidden unbounded queue between MCP tool handlers and outbound RPC work
- cancellation paths that release permits through normal Rust drop semantics

## Type System Strategy

Protocol strings should exist only at boundaries.

Recommended internal types:

- `Mode`
- `VmProfile`
- `ChainPin`
- `ChainContext`
- `SimulationStatus`
- `SequenceNumberSource`
- `GasPriceSource`
- `EndpointHealth`

Recommended rule:

- deserialize boundary DTOs with `serde`
- convert them into domain types with `TryFrom`
- keep policy and orchestration logic in typed domain services

## Starcoin Dependency Strategy

Use Starcoin crates for:

- address and transaction types
- BCS encoding and decoding
- ABI resolution models

But isolate those dependencies behind project-owned traits and mapper modules so that upstream SDK or RPC changes stay localized.

Rules:

1. do not let raw JSON-RPC method names spread through the server crate
2. do not build transaction bytes in the MCP shim
3. do not let VM1 and VM2 branching leak into host-facing DTOs

## Error Model

Use a layered error model.

Recommended approach:

1. typed domain errors in `starcoin-node-core`
2. adapter-specific errors in `starcoin-node-rpc`
3. shared error-code mapping at the MCP boundary

Recommended crates:

- `thiserror`
- `anyhow` only at binary entrypoints and tests

## Caching Strategy

The first implementation should prefer bounded in-memory caches for:

- chain status
- module ABI
- struct ABI
- function ABI

Recommended properties:

- TTL-based expiry
- per-endpoint cache scoping
- explicit disable switch from config
- bounded entry counts so caches cannot grow without limit

The first release should not require a persistent cache database.

## Backpressure Strategy

The first implementation should use small, explicit backpressure primitives instead of a large scheduler.

Rules:

1. clamp caller-provided list sizes and time budgets before domain services allocate work
2. reject publish-package payloads above the configured size ceiling with `payload_too_large`
3. acquire permits before starting watch loops or other expensive request classes
4. return `rate_limited` when local permits are exhausted rather than queueing unbounded tasks
5. ensure permit guards are released on cancellation, timeout, and normal completion
6. keep overload decisions local and deterministic so `rate_limited` never implies uncertain chain side effects

## Testing Strategy

The Rust implementation must cover:

1. endpoint probe and capability classification
2. chain pin validation
3. sequence-number derivation rules
4. simulation result normalization
5. submission error mapping
6. MCP tool snapshots for major result shapes
7. clamp, payload-size, and permit-release behavior under local overload

Recommended test styles within the required Rust implementation:

- unit tests for core orchestration
- fixture-driven adapter tests for RPC responses
- snapshot tests for MCP outputs
- Tokio-aware concurrency tests for semaphore release and `rate_limited` behavior

## Security and Safety Notes

The Rust workspace should default to:

- `#![forbid(unsafe_code)]`

If unsafe code is needed for a platform transport dependency:

- isolate it outside core transaction and adapter logic
- document the invariant
- keep the unsafe surface minimal

Additional Rust guidance:

1. chain pinning should use typed structs, not free-form string pairs
2. redaction helpers should be used before logging config or endpoint metadata
3. transaction summaries should be derived from typed decoded values where possible

## Non-Goals

This document does not require:

- a separate chain-side daemon
- a persistent job queue
- a full Starcoin RPC reimplementation inside the MCP server
