# Starcoin MCP

This repository groups Starcoin-related MCP projects in one place.

Repository-level materials live under:

- `docs/architecture/`
- `shared/`

Key architecture documents:

- `docs/architecture/overview.md`
- `docs/architecture/host-integration.md`
- `docs/architecture/design-closure-plan.md`
- `docs/architecture/deployment-model.md`

## Subprojects

- `starmask-mcp/`
  - local wallet-facing MCP entrypoint design
  - includes the interface draft for `starmask-mcp`, `starmaskd`, and the Chrome extension bridge
- `starcoin-node-mcp/`
  - chain-facing Starcoin MCP design
  - includes interface, security, deployment, configuration, RPC-adapter, and Rust implementation design docs for chain queries, transaction preparation, simulation, and signed transaction submission
  - the first conforming implementation is expected to be written in Rust

## Status

Wallet-facing and chain-facing subprojects both have implementation-oriented design documents for review.
