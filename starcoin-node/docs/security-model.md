# Starcoin Node Security Model

## Purpose

This document defines the security and safety model for `starcoin-node`.

The stack includes:

- the local MCP host
- `starcoin-node`
- one configured Starcoin RPC endpoint
- an external wallet-facing signer such as `starmask-runtime`

## Security Goal

The primary security goal is:

`starcoin-node` must let a local MCP host access chain state and prepare transaction payloads without creating false signing trust in the node endpoint or crossing the wallet boundary.

## Security Invariants

The following rules must remain true in every implementation phase:

1. `starcoin-node` never holds private keys.
2. `starcoin-node` never signs transactions.
3. Transaction mode always operates against an explicitly validated chain context.
4. Unsigned transaction bytes returned to the host are constructed locally and are the canonical payload for downstream signing.
5. Remote RPC responses may inform host workflows but are not the security source of truth for wallet approval.
6. Admin or node-management operations are disabled in the first release.
7. Endpoint credentials and sensitive payloads are not emitted to logs by default.
8. Expensive operations are bounded by local policy so one noisy host request does not create unbounded background work.

## Trust Boundaries

### Trusted for wallet approval and signing

- the wallet-facing system such as `starmask-runtime` and its extension

### Trusted for chain-tool policy and payload construction

- `starcoin-node`

### Partially trusted for chain data and simulation

- the configured Starcoin RPC endpoint

The endpoint may be honest, stale, buggy, or misconfigured. The design must not confuse endpoint output with wallet approval truth.

### Untrusted for security decisions

- MCP-host-generated prose or summaries
- any wallet approval hint not derived by the wallet itself
- arbitrary endpoint metadata that conflicts with configured chain pinning

## Assets to Protect

The design must protect:

1. endpoint authentication material
2. chain identity and network pinning
3. unsigned transaction payload integrity
4. signed transaction bytes before submission
5. ABI and module metadata used to summarize calls
6. user intent about which chain and endpoint a transaction targets

## Threat Model

The first implementation should explicitly defend against:

1. a stale or lagging RPC endpoint
2. a remote endpoint that points to the wrong network
3. an MCP host that asks for transaction preparation against an unintended chain
4. insecure remote transport accidentally used for transaction mode
5. accidental exposure of bearer tokens, raw package bytes, or signed payloads in logs
6. drift between the chain context used during preparation and the endpoint used during submission
7. misuse of raw RPC methods that bypass MCP capability gating
8. oversized query, watch, or publish-package requests that would otherwise exhaust local process resources

The first implementation does not attempt to defend against:

1. a fully compromised local user account
2. a malicious wallet runtime
3. a fully dishonest remote RPC endpoint that fabricates chain data while still satisfying basic probes

Those remain important risks, but they are outside the practical trust model of a local chain adapter.

## Chain Context Security Rules

Transaction-adjacent workflows require explicit chain context.

Rules:

1. transaction mode must pin at least `expected_chain_id`
2. the implementation should also pin the expected network name when available
3. `expected_genesis_hash` should be enforced for remote transaction mode and should be used whenever the deployment can provide it
4. startup probes must validate the active endpoint against configured chain pinning
5. transaction preparation must re-check chain identity before returning a signable payload
6. signed-transaction submission must re-check chain identity before sending bytes to txpool
7. chain mismatch returns `invalid_chain_context`

## Preparation Security Rules

Preparation is a safety-sensitive action even though it is not signing.

Rules:

1. unsigned transactions must be built locally from normalized inputs and validated chain context
2. sequence number derivation must be deterministic and documented
3. gas defaults must come from explicit policy or a documented endpoint lookup
4. if simulation runs, the result must be clearly labeled as endpoint-provided execution feedback, not a final guarantee
5. the returned envelope should include enough chain-context metadata for the host and wallet flow to reason about the target chain

## Submission Safety Rules

Submission must preserve the signer boundary while reducing chain mistakes.

Rules:

1. `starcoin-node` may submit only already signed transactions
2. submission must not mutate signed bytes
3. the server should decode enough of the signed transaction to validate chain id and basic structure before submission
4. the server should derive `txn_hash` locally before the RPC submission attempt so reconciliation never depends on a successful endpoint response
5. automatic retry or rebroadcast policy is out of scope for the first release
6. a transport failure during submission must not be reported as confirmed rejection unless the endpoint returned a structured failure result
7. `submission_unknown` must be treated as a reconcile-first state, not as a retryable blind re-submit
8. `transaction_expired` and `sequence_number_stale` require re-preparation and re-signing

## Remote Endpoint Security

Remote endpoints introduce additional safety risk.

Rules:

1. secure remote transport should be the default for transaction mode
2. insecure remote transport requires an explicit development override
3. endpoint TLS or authentication settings must come from configuration, not from host tool inputs
4. host-visible results must never echo secrets such as bearer tokens or full auth headers
5. remote transaction mode should support endpoint allowlisting or certificate pinning where deployments require stronger trust than DNS and CA validation alone

## Resource Exhaustion and Overload Safety

The first release should defend against accidental local overload without turning the chain-side server into a complex scheduler.

Rules:

1. host-supplied query sizes, watch timeouts, and poll intervals must be clamped or rejected against configuration-defined bounds before outbound RPC work begins
2. package publish payloads above the configured byte ceiling must be rejected locally with `payload_too_large`
3. long-running watch loops and other expensive operations should consume bounded local permits rather than spawning unbounded async work
4. when local concurrency or request-budget limits are exhausted, the server should return `rate_limited` before outbound RPC side effects occur
5. task cancellation and timeout paths must release permits promptly

## Logging and Redaction Rules

Logs must be useful without leaking sensitive material.

Rules:

1. redact endpoint credentials
2. avoid logging full signed transaction bytes by default
3. avoid logging full publish-package payloads by default
4. include chain id, network, endpoint profile, and tool name in diagnostic logs
5. log chain mismatch and capability mismatch as first-class structured events
6. log local clamp and `rate_limited` decisions as first-class structured events without echoing oversized payload contents

## Rust Security Implementation Notes

The first conforming implementation is Rust, so these safety rules should also be reflected in Rust-native constructs.

Required Rust-oriented guidance:

1. the workspace should default to `#![forbid(unsafe_code)]`
2. endpoint credentials and auth headers should use redaction-aware secret wrappers rather than plain `String`
3. chain identity, endpoint identity, and transaction hash values should use typed newtypes or domain structs rather than free-form strings
4. MCP boundary DTOs, domain models, and logging views should remain separate so secrets and large payloads are not accidentally serialized or logged
5. structured logs should be emitted through `tracing` with redacted fields for endpoint configuration and submission payload metadata

## Safety Relationship to Wallet Approval

`starcoin-node` may produce:

- transaction summaries
- simulation summaries
- ABI-derived call explanations

These are helpful host artifacts, but they are not the wallet's security source of truth.

Rules:

1. wallet approval should render from canonical transaction bytes
2. the wallet may ignore or overwrite host-side summaries
3. the host must not treat successful simulation as equivalent to wallet approval or on-chain success

## Threat Scenarios and Expected Mitigations

### Wrong network endpoint configured

Mitigation:

- startup chain pin validation fails
- transaction tools remain unavailable
- host receives `invalid_chain_context`

### Public remote endpoint is healthy but far behind

Mitigation:

- node health result surfaces lag or sync warnings
- transaction mode may warn or fail according to configuration thresholds

### Host tries to use chain-side server as a signer

Mitigation:

- no signing tool exists
- no account unlock or wallet key RPC is exposed
- tool surface only returns unsigned bytes or accepts already signed bytes

### Endpoint authentication token leaks into logs

Mitigation:

- configuration loader stores secrets separately from rendered diagnostics
- redaction is applied before logging config and request metadata

### Host or agent requests too many concurrent watches or oversized payloads

Mitigation:

- request sizes are clamped or rejected against config-defined ceilings
- watch and expensive-operation concurrency is protected by local permits
- overload returns `rate_limited` before outbound RPC side effects occur

### Submission response is lost after the node may already have accepted the transaction

Mitigation:

- `txn_hash` is derived locally before submission
- the server returns `submission_unknown`
- the host reconciles by hash before any retry

### Wallet approval finishes after the prepared transaction has already become stale

Mitigation:

- submission maps expiry and stale-sequence failures to explicit error codes
- the host restarts from fresh preparation and fresh wallet approval
