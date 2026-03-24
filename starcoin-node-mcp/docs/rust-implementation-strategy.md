# Starcoin Node MCP Rust Implementation Strategy

## Purpose

This document defines how the required first `starcoin-node-mcp` implementation should be structured in Rust.

It translates the chain-side interface, deployment, configuration, and adapter documents into Rust-specific implementation constraints.

## Language Requirement

The first conforming implementation is:

- `starcoin-node-mcp`: Rust

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

## Workspace Shape

The first implementation should use one Cargo workspace with a small number of crates.

Recommended shape:

```text
starcoin-node-mcp/
  Cargo.toml
  crates/
    starcoin-node-mcp-types/
    starcoin-node-mcp-core/
    starcoin-node-mcp-rpc/
    starcoin-node-mcp-server/
```

Recommended responsibilities:

- `starcoin-node-mcp-types`
  - public DTOs
  - ids, enums, config structs, error code mapping
- `starcoin-node-mcp-core`
  - capability gating
  - chain-context validation
  - preparation and simulation orchestration
  - transaction summary generation
- `starcoin-node-mcp-rpc`
  - endpoint probing
  - Starcoin RPC client abstraction
  - VM compatibility handling
  - RPC-native view mapping
- `starcoin-node-mcp-server`
  - `rmcp` integration
  - tool registration
  - process startup and config loading

## Runtime Model

`starcoin-node-mcp` should use Tokio because it needs:

- stdio MCP transport
- RPC IO
- watch-loop polling
- bounded in-memory caches

Recommended model:

1. `starcoin-node-mcp-server` receives MCP tool calls
2. tool handlers deserialize boundary DTOs
3. handlers call typed domain services in `starcoin-node-mcp-core`
4. core services use trait-based adapters from `starcoin-node-mcp-rpc`
5. results are mapped back into MCP tool outputs

The first implementation does not need a dedicated coordinator task or durable state machine.

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

1. typed domain errors in `starcoin-node-mcp-core`
2. adapter-specific errors in `starcoin-node-mcp-rpc`
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

The first release should not require a persistent cache database.

## Testing Strategy

The Rust implementation must cover:

1. endpoint probe and capability classification
2. chain pin validation
3. sequence-number derivation rules
4. simulation result normalization
5. submission error mapping
6. MCP tool snapshots for major result shapes

Recommended test styles within the required Rust implementation:

- unit tests for core orchestration
- fixture-driven adapter tests for RPC responses
- snapshot tests for MCP outputs

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
