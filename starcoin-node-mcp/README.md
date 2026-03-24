# Starcoin Node MCP

This subproject contains the design set for the chain-facing Starcoin MCP server.

The intended role of `starcoin-node-mcp` is:

- chain and node data access
- transaction preparation
- transaction simulation
- submission of already signed transactions

It does not hold private keys and does not perform wallet signing.

## Contents

- `docs/starcoin-node-mcp-interface-design.md`: MCP tool surface and result semantics
- `docs/security-model.md`: chain-side trust boundary and safety rules
- `docs/deployment-model.md`: runtime topology and capability profiles
- `docs/configuration.md`: endpoint, chain pinning, and timeout configuration
- `docs/rpc-adapter-design.md`: VM compatibility and RPC normalization strategy
- `docs/rust-implementation-strategy.md`: implementation structure for the first Rust version
- `docs/design-closure-plan.md`: implementation-readiness checklist for the chain-side design

## Status

Implementation-oriented design set for review as a subproject of `starcoin-mcp`.
