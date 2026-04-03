# Starcoin Node

This subproject contains the chain-facing Starcoin node library/CLI implementation plus its design
set.

The repository no longer ships an in-tree stdio adapter for `starcoin-node`. The current Rust
implementation in this workspace is centered on shared libraries plus the `starcoin-node-cli`
binary.

The intended role of `starcoin-node` is:

- chain and node data access
- transaction preparation
- transaction simulation
- submission of already signed transactions

It does not hold private keys and does not perform wallet signing.

The first conforming implementation of `starcoin-node` is required to be written in Rust.

## Contents

- `docs/architecture/host-integration.md`: host-side orchestration from prepare to wallet approval and submission
- `docs/starcoin-node-interface-design.md`: host-facing tool surface and result semantics
- `docs/security-model.md`: chain-side trust boundary and safety rules
- `docs/deployment-model.md`: runtime topology and capability profiles
- `docs/configuration.md`: endpoint, chain pinning, and timeout configuration
- `docs/rpc-adapter-design.md`: shared/vm1/vm2 RPC surface classification and normalization strategy
- `docs/rust-implementation-strategy.md`: implementation structure for the first Rust version
- `docs/design-closure-plan.md`: implementation-readiness checklist for the chain-side design
- `docs/testing-and-acceptance.md`: acceptance criteria for probing, preparation, submission, reconciliation, and security

## Status

Shared library and CLI implementation with design references for a possible future external host
adapter.
