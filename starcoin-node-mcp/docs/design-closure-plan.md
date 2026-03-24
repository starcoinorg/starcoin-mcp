# Starcoin Node MCP Design Closure Plan

## Purpose

This document defines how `starcoin-node-mcp` design work should be completed before implementation starts.

The goal is to prevent the chain-facing MCP server from hard-coding assumptions about:

- which Starcoin RPC capabilities are required
- how chain context is pinned and validated
- which deployment profiles are safe for transaction-adjacent workflows
- how VM1 and VM2 differences are normalized behind one MCP tool surface

## Design Completion Standard

`starcoin-node-mcp` design is considered ready for implementation only when all of the following are true:

1. The runtime topology and deployment profiles are documented.
2. The chain-side trust boundary is explicit and consistent with repository-level host orchestration.
3. Transaction preparation, simulation, submission, and watch flows are closed end to end.
4. Configuration rules define how endpoint selection, chain pinning, and capability gating behave.
5. RPC compatibility and VM-version differences are isolated behind an adapter contract.
6. Result shapes and error mapping are stable enough for MCP hosts to orchestrate reliably.
7. The initial implementation scope is frozen.
8. The required implementation language is explicit and consistent across the subproject documents.
9. Rust type boundaries, async-runtime ownership, and crate responsibilities are explicit enough to implement without reopening architecture questions.

## Required Document Set

The following documents should exist before implementation starts:

1. `docs/architecture/host-integration.md`
2. `starcoin-node-mcp/docs/starcoin-node-mcp-interface-design.md`
3. `starcoin-node-mcp/docs/security-model.md`
4. `starcoin-node-mcp/docs/deployment-model.md`
5. `starcoin-node-mcp/docs/configuration.md`
6. `starcoin-node-mcp/docs/rpc-adapter-design.md`
7. `starcoin-node-mcp/docs/rust-implementation-strategy.md`
8. `starcoin-node-mcp/docs/testing-and-acceptance.md`

## Design Order

The documents should be completed in this order:

1. repository-level host orchestration
2. chain-side interface design
3. security and trust boundary rules
4. deployment profiles and runtime model
5. configuration and capability gating
6. RPC adapter and compatibility model
7. Rust implementation strategy
8. testing and acceptance criteria

This order keeps tool semantics constrained by earlier chain-boundary decisions instead of re-opening them during implementation.

## Closed-Loop Flow Requirement

At minimum, the repository must define complete flows for:

1. startup probe against a local node
2. startup probe against a remote endpoint
3. read-only chain query
4. ABI or contract metadata resolution
5. unsigned transaction preparation with a known public key
6. unsigned transaction preparation without a public key, followed by later simulation
7. signed transaction submission
8. transaction watch until terminal confirmation or timeout
9. endpoint outage during a query
10. chain mismatch at startup
11. chain mismatch detected before submission
12. lagging or unhealthy node in transaction mode
13. uncertain submission result after transport loss or timeout
14. prepared transaction expires before wallet approval finishes
15. sequence number becomes stale before submission
16. endpoint capability mismatch between VM profile and requested tool surface

A flow is incomplete if it documents only the happy path and does not explain:

- who initiates the action
- which chain snapshot or endpoint capability is used
- which error code is returned on failure
- whether the host may retry
- whether the server should degrade to a narrower capability profile

## First-Release Decisions

The first-release chain-side design is now closed on the following decisions:

1. One binary supports multiple capability profiles through configuration rather than separate binaries.
2. `read_only` is the default profile.
3. `transaction` mode is explicit opt-in and requires chain pinning.
4. `admin` operations remain out of scope for the first release.
5. When `sender_public_key` is available, preparation tools attempt simulation before returning.
6. The MCP surface stays version-neutral; VM1 and VM2 differences are handled by the internal adapter layer.
7. Transaction summaries are useful host hints but are not the security source of truth for wallet approval.
8. The server builds unsigned transaction bytes locally; it does not depend on node-side account-signing RPC.
9. One `starcoin-node-mcp` process targets one configured endpoint at a time in the first release.
10. Transaction mode should validate `genesis_hash` in addition to `chain_id` and network whenever the deployment can supply it.
11. `submit_signed_transaction` returns a deterministic `txn_hash` even when the endpoint outcome is uncertain, and retry logic must reconcile by hash before re-submission.
12. `transaction_expired` and `sequence_number_stale` require fresh preparation and fresh wallet approval rather than blind re-use of old signed bytes.
13. The first conforming implementation of `starcoin-node-mcp` must be written in Rust.

## First Implementation Scope Freeze

The first implementation should remain intentionally narrow.

In scope:

- local launch by an MCP host over stdio
- one configured Starcoin RPC endpoint per process
- `read_only` and `transaction` capability profiles
- query, ABI resolution, view execution, unsigned transaction preparation, simulation, submission, and watch flows
- local caching of endpoint metadata and ABI results
- VM2-first behavior with compatibility fallback in the adapter layer
- one Rust Cargo workspace implementing the chain-side server

Out of scope for the first implementation:

- multi-endpoint quorum reads
- active failover across RPC endpoints
- push subscriptions or event streaming into MCP hosts
- destructive node-management operations
- wallet storage, key management, or transaction signing
- policy-driven automatic re-submission or background relaying

## Review Checklist

Before implementation begins, review the design with the following checklist:

- Are deployment profiles explicit?
- Are chain pinning rules written down for transaction mode?
- Are all transaction-adjacent flows closed from preparation to submission?
- Are VM compatibility rules owned by one adapter layer?
- Are configuration defaults safe for remote endpoints?
- Are unsigned transaction envelopes strong enough to carry chain identity and freshness metadata as a stable contract?
- Are error codes mapped to shared repository vocabulary where possible?
- Are host-visible summaries clearly separated from wallet security decisions?
- Is uncertain submission reconciled by transaction hash before any retry?
- Is the Rust implementation requirement reflected consistently across interface, configuration, testing, and implementation docs?
- Are unsupported admin or signing behaviors explicitly blocked?

## Closure Status

The required document set for the first chain-side implementation now exists.

At this point, remaining pre-code work should focus on review and targeted refinement rather than adding new architectural layers.
