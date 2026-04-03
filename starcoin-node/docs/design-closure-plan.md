# Starcoin Node Design Closure Plan

## Purpose

This document defines how `starcoin-node` design work should be completed before implementation starts.

The goal is to prevent the chain-facing runtime from hard-coding assumptions about:

- which Starcoin RPC capabilities are required
- how chain context is pinned and validated
- which deployment profiles are safe for transaction-adjacent workflows
- how VM1 and VM2 differences are normalized behind one host tool surface

## Design Completion Standard

`starcoin-node` design is considered ready for implementation only when all of the following are true:

1. The runtime topology and deployment profiles are documented.
2. The chain-side trust boundary is explicit and consistent with repository-level host orchestration.
3. Transaction preparation, simulation, submission, and watch flows are closed end to end.
4. Configuration rules define how endpoint selection, chain pinning, and capability gating behave.
5. RPC compatibility and VM-version differences are isolated behind an adapter contract.
6. Result shapes and error mapping are stable enough for host tools to orchestrate reliably.
7. The initial implementation scope is frozen.
8. The required implementation language is explicit and consistent across the subproject documents.
9. Rust type boundaries, async-runtime ownership, and crate responsibilities are explicit enough to implement without reopening architecture questions.
10. Query bounds, payload-size limits, and local overload behavior are explicit enough to prevent unbounded host-driven work.

## Required Document Set

The following documents should exist before implementation starts:

1. `docs/architecture/host-integration.md`
2. `starcoin-node/docs/starcoin-node-interface-design.md`
3. `starcoin-node/docs/security-model.md`
4. `starcoin-node/docs/deployment-model.md`
5. `starcoin-node/docs/configuration.md`
6. `starcoin-node/docs/rpc-adapter-design.md`
7. `starcoin-node/docs/rust-implementation-strategy.md`
8. `starcoin-node/docs/testing-and-acceptance.md`

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

## Flow Definition Index

The following index records where each required flow is closed and which explicit attributes govern it.

1. `startup probe against a local node`: initiator `host tool` at process launch; chain snapshot or capability `startup probes` using `chain.info`, `node.status`, and `node.info`; failure codes `node_unavailable`, `rpc_unavailable`, `invalid_chain_context`, `unsupported_operation`; host retry `yes` after endpoint or config repair; degradation `read_only` may degrade on optional health methods but `transaction` fails closed. References: `deployment-model.md` startup and capability sections; `testing-and-acceptance.md` startup and capability acceptance.
2. `startup probe against a remote endpoint`: initiator `host tool` at process launch; chain snapshot or capability remote startup probes plus secure-transport and chain-pin validation; failure codes `node_unavailable`, `rpc_unavailable`, `invalid_chain_context`, `unsupported_operation`; host retry `yes` after transport, allowlist, or pin repair; degradation insecure remote transport does not degrade and must fail unless explicitly overridden. References: `deployment-model.md` startup and transport sections; `configuration.md` chain-pin and endpoint settings; `testing-and-acceptance.md` startup and configuration acceptance.
3. `read-only chain query`: initiator `host tool`; chain snapshot or capability bounded read-only tool calls against the active endpoint and current chain context; failure codes `node_unavailable`, `rpc_unavailable`, `rate_limited`; host retry `yes` for transient endpoint or local overload conditions; degradation optional health detail may narrow but query semantics stay read-only. References: `starcoin-node-interface-design.md` query tools and resource governance; `deployment-model.md` backpressure rules; `testing-and-acceptance.md` query and resource acceptance.
4. `ABI or contract metadata resolution`: initiator `host tool`; chain snapshot or capability contract and state methods with bounded ABI caching; failure codes `node_unavailable`, `rpc_unavailable`, `unsupported_operation`, `rate_limited`; host retry `yes` after transient failure; degradation cache miss falls back to endpoint fetch but optional decode richness may narrow. References: `rpc-adapter-design.md` state and ABI mapping; `rust-implementation-strategy.md` caching strategy; `testing-and-acceptance.md` query and resource acceptance.
5. `unsigned transaction preparation with a known public key`: initiator `host tool`; chain snapshot or capability fresh `chain.info`, sequence and gas sources, and dry-run capability; failure codes `invalid_chain_context`, `rpc_unavailable`, `simulation_failed`, `rate_limited`; host retry `yes` after transient RPC or overload failure, otherwise fix inputs; degradation none in `transaction` mode because required preparation capability fails closed. References: `host-integration.md` signed transaction flow; `starcoin-node-interface-design.md` preparation results; `testing-and-acceptance.md` preparation acceptance.
6. `unsigned transaction preparation without a public key, followed by later simulation`: initiator `host tool` then wallet-assisted follow-up; chain snapshot or capability fresh preparation plus later `simulate_raw_transaction`; failure codes `missing_public_key`, `invalid_chain_context`, `simulation_failed`, `rate_limited`; host retry `yes` after obtaining public key or recovering transient failure; degradation preparation may return `simulation_status = skipped_missing_public_key` but must not pretend simulation ran. References: `host-integration.md` phases B and C; `starcoin-node-interface-design.md` preparation and simulation sections; `testing-and-acceptance.md` preparation acceptance.
7. `signed transaction submission`: initiator `host tool` after wallet approval; chain snapshot or capability fresh pre-submit `chain.info` re-check and submission capability; failure codes `invalid_chain_context`, `submission_failed`, `submission_unknown`, `transaction_expired`, `sequence_number_stale`, `rate_limited`; host retry `yes` only for `rate_limited` or after reconcile-first `submission_unknown`; degradation none because `transaction` mode fails closed. References: `host-integration.md` phase E; `rpc-adapter-design.md` submission mapping and reconciliation; `testing-and-acceptance.md` submission acceptance.
8. `transaction watch until requested confirmation depth or timeout`: initiator `host tool`; chain snapshot or capability bounded polling through `watch_transaction`, plus confirmation-depth evaluation from transaction inclusion block and current head block; failure codes `rpc_unavailable`, `transaction_not_found`, `rate_limited`; host retry `yes` with backoff while preserving `txn_hash`; degradation time budgets and minimum confirmation blocks may be clamped but the effective values are returned. References: `starcoin-node-interface-design.md` watch tool and resource results; `deployment-model.md` runtime and backpressure rules; `testing-and-acceptance.md` resource and end-to-end acceptance.
9. `endpoint outage during a query`: initiator `MCP host`; chain snapshot or capability current endpoint connectivity during any read-only tool; failure codes `node_unavailable`, `rpc_unavailable`; host retry `yes` after endpoint recovery; degradation the configured capability profile does not silently change. References: `deployment-model.md` shutdown and recovery rules; `rpc-adapter-design.md` error mapping; `testing-and-acceptance.md` startup and query acceptance.
10. `chain mismatch at startup`: initiator `starcoin-node` during startup probes; chain snapshot or capability configured pins versus probed `chain.info`; failure codes `invalid_chain_context`; host retry `yes` only after config or endpoint correction; degradation `transaction` mode stays disabled and startup fails closed, while `read_only` may run unpinned only under explicit autodetect override with warning. References: `configuration.md` chain-pin settings; `deployment-model.md` startup model; `testing-and-acceptance.md` startup and configuration acceptance.
11. `chain mismatch detected before submission`: initiator `starcoin-node` during pre-submit validation; chain snapshot or capability fresh `chain.info` re-check against pinned context and prepared envelope metadata; failure codes `invalid_chain_context`; host retry `no` until the endpoint or config is corrected and a fresh transaction is prepared; degradation none because submission must abort before txpool contact. References: `security-model.md` chain context and submission rules; `rpc-adapter-design.md` submission mapping; `testing-and-acceptance.md` submission acceptance.
12. `lagging or unhealthy node in transaction mode`: initiator `starcoin-node` during health checks or transaction-adjacent flows; chain snapshot or capability `node.status`, optional `sync.status`, and configured lag thresholds; failure codes `rpc_unavailable` or host-visible warnings when below fail threshold; host retry `yes` after node health recovers; degradation `read_only` may continue with warnings but `transaction` may warn or fail according to policy. References: `deployment-model.md` capability and observability sections; `configuration.md` lag thresholds; `testing-and-acceptance.md` query and startup acceptance.
13. `uncertain submission result after transport loss or timeout`: initiator `starcoin-node` when submission response is not definitive; chain snapshot or capability locally derived `txn_hash` plus reconciliation query path; failure codes `submission_unknown`; host retry `not before reconcile-by-hash`; degradation no automatic blind re-submit or background relaying. References: `host-integration.md` phase E; `deployment-model.md` recovery rules; `testing-and-acceptance.md` submission and end-to-end acceptance.
14. `prepared transaction expires before wallet approval finishes`: initiator `host tool` discovers this on submission after async wallet approval; chain snapshot or capability prepared envelope freshness, endpoint rejection, and wallet orchestration state; failure codes `transaction_expired`; host retry `yes`, but only by re-preparing and re-signing; degradation none because old signed bytes must not be reused. References: `host-integration.md` phase E; `security-model.md` threat scenarios; `testing-and-acceptance.md` submission and end-to-end acceptance.
15. `sequence number becomes stale before submission`: initiator `host tool` discovers this on submission after other pending transactions advance sequence; chain snapshot or capability fresh txpool rejection and prepared envelope metadata; failure codes `sequence_number_stale`; host retry `yes`, but only by fresh preparation and fresh signing; degradation none because stale signed bytes must not be replayed. References: `host-integration.md` phase E; `rpc-adapter-design.md` submission reconciliation; `testing-and-acceptance.md` submission and end-to-end acceptance.
16. `endpoint capability mismatch between VM profile and requested tool surface`: initiator `starcoin-node` during startup probe or tool gating; chain snapshot or capability capability classification and VM profile selection; failure codes `unsupported_operation`; host retry `yes` only after profile or endpoint capability changes; degradation read-only may narrow on optional methods, but required `transaction` or `vm2_only` capabilities fail closed. References: `rpc-adapter-design.md` capability discovery and RPC surface classification; `deployment-model.md` capability model; `testing-and-acceptance.md` startup acceptance.

## First-Release Decisions

The first-release chain-side design is now closed on the following decisions:

1. One binary supports multiple capability profiles through configuration rather than separate binaries.
2. `read_only` is the default profile.
3. `transaction` mode is explicit opt-in and requires chain pinning.
4. `admin` operations remain out of scope for the first release.
5. When `sender_public_key` is available, preparation tools attempt simulation before returning.
6. The host surface stays version-neutral; VM1 and VM2 differences are handled by the internal adapter layer.
7. Transaction summaries are useful host hints but are not the security source of truth for wallet approval.
8. The server builds unsigned transaction bytes locally; it does not depend on node-side account-signing RPC.
9. One `starcoin-node` process targets one configured endpoint at a time in the first release.
10. Transaction mode should validate `genesis_hash` in addition to `chain_id` and network whenever the deployment can supply it.
11. `submit_signed_transaction` returns a deterministic `txn_hash` even when the endpoint outcome is uncertain, and retry logic must reconcile by hash before re-submission.
12. `transaction_expired` and `sequence_number_stale` require fresh preparation and fresh wallet approval rather than blind re-use of old signed bytes.
13. The first conforming implementation of `starcoin-node` must be written in Rust.
14. List-like queries, watch loops, and package-publish inputs are governed by configuration-defined bounds, and local overload must fail fast before outbound RPC side effects occur.

## First Implementation Scope Freeze

The first implementation should remain intentionally narrow.

In scope:

- local launch by an MCP host over stdio
- one configured Starcoin RPC endpoint per process
- `read_only` and `transaction` capability profiles
- query, ABI resolution, view execution, unsigned transaction preparation, simulation, submission, and watch flows
- local caching of endpoint metadata and ABI results
- adapter-owned routing across shared RPC plus VM1/VM2 RPC surfaces
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
- Are shared/vm1/vm2 RPC surface routing rules owned by one adapter layer?
- Are configuration defaults safe for remote endpoints?
- Are unsigned transaction envelopes strong enough to carry chain identity and freshness metadata as a stable contract?
- Are error codes mapped to shared repository vocabulary where possible?
- Are host-visible summaries clearly separated from wallet security decisions?
- Is uncertain submission reconciled by transaction hash before any retry?
- Is the Rust implementation requirement reflected consistently across interface, configuration, testing, and implementation docs?
- Are query-size limits, watch budgets, payload-size ceilings, and local overload semantics explicit enough to avoid unbounded work under a noisy host?
- Are unsupported admin or signing behaviors explicitly blocked?

## Closure Status

The required document set for the first chain-side implementation now exists.

At this point, remaining pre-code work should focus on review and targeted refinement rather than adding new architectural layers.
