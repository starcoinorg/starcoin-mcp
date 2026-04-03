# Starcoin MCP

This repository groups Starcoin-related runtimes, host adapters, and workflow plugins in one
place.

Repository-level materials live under:

- `docs/`
- `shared/`

Key repository documents:

- `docs/architecture/overview.md`
- `docs/architecture/host-integration.md`
- `docs/architecture/design-closure-plan.md`
- `docs/architecture/deployment-model.md`
- `docs/testing-coverage-assessment.md`

## Subprojects

- `starmask-runtime/`
  - wallet-facing runtime and adapter design
  - includes the interface draft for `starmask-runtime`, `starmaskd`, and the Chrome extension bridge
- `starcoin-node/`
  - chain-facing Starcoin runtime and host-interface design
  - includes interface, security, deployment, configuration, RPC-adapter, and Rust implementation design docs for chain queries, transaction preparation, simulation, and signed transaction submission
  - the first conforming implementation is expected to be written in Rust

## Status

Wallet-facing and chain-facing subprojects both have implementation-oriented design documents for review.
