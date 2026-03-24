# Starmask MCP Security Model

## Purpose

This document defines the security model for the `starmask-mcp` stack.

The stack includes:

- `starmask-mcp`
- `starmaskd`
- `starmask-native-host`
- `Starmask` Chrome extension
- the local MCP host

## Security Goal

The primary security goal is:

`starmask-mcp` must allow a local MCP host to request wallet actions without granting that host signing authority or private key access.

## Security Invariants

The following rules must remain true in every implementation phase:

1. Private key material never leaves the extension.
2. The daemon never creates a signature.
3. The MCP host never becomes the source of truth for what the user is approving.
4. The extension renders the transaction or message from canonical payload bytes.
5. Every signing request is explicitly attributable to one local account and one logical wallet instance.
6. Non-terminal request state is durable across process restarts.
7. Local transport is restricted to the current OS user.

## Trust Boundaries

### Trusted for signing

- `Starmask` extension

### Trusted for persistence and routing, but not signing

- `starmaskd`

### Trusted for transport adaptation only

- `starmask-mcp`
- `starmask-native-host`

### Untrusted for security decisions

- MCP host output
- `display_hint`
- any transaction summary not derived by the extension itself

## Assets to Protect

The design must protect:

1. private keys
2. unlock state
3. signed transaction results before submission
4. message signatures
5. request integrity
6. wallet instance routing integrity
7. user intent at approval time

## Threat Model

The first implementation should explicitly defend against:

1. a buggy or overly aggressive MCP host
2. a local process attempting to connect to daemon transport as the same user
3. stale or replayed request identifiers
4. daemon restart during an active signing flow
5. browser or extension restart during an active signing flow
6. extension mismatch across dev, staging, and production release channels
7. misleading host-provided summaries

The first implementation does not attempt to defend against:

1. a fully compromised OS user account
2. a malicious browser runtime with arbitrary extension code execution
3. malware with the same OS-user privileges and direct access to local wallet runtime

Those remain important risks, but they are outside the practical trust model of a local wallet integration.

## Approval Security Rules

The extension approval UI is the final decision point.

Rules:

1. the extension must decode and present the transaction itself
2. the extension may show `display_hint`, but only as secondary context
3. the extension must show enough information for an informed decision
4. unsupported transaction payloads must not fall back to silent blind signing
5. the initial release requires explicit approval for every sign request

## Required UI Security Fields

For transaction signing, the extension must present:

- account address
- chain id
- transaction kind
- receiver or called function
- amount where applicable
- gas budget and gas price
- expiration

For message signing, the extension must present:

- account address
- message format
- message content or a safe canonical preview
- origin context if known

## Request Integrity Rules

Every signing request must have:

- a cryptographically strong `request_id`
- a deterministic `payload_hash`
- `created_at`
- `expires_at`
- one canonical status owner, which is `starmaskd`

The daemon must reject:

- requests missing required fields
- expired requests
- requests routed to unknown wallet instances
- malformed transaction payloads

## Result Handling Rules

Signed outputs are sensitive even when private keys are not exposed.

Rules:

1. signed results must be retained for a limited time only
2. result availability policy must be explicit and documented
3. result retrieval must be keyed by `request_id`
4. daemon logs must never print full signed payloads by default

## Routing Security

Wallet routing must be explicit and deterministic.

Rules:

1. if the caller names `wallet_instance_id`, only that instance may receive the request
2. if multiple wallet instances match and none is selected, the daemon must fail with `wallet_selection_required`
3. auto-routing is allowed only when exactly one matching instance exists
4. account identity alone is insufficient when more than one wallet instance exposes the same address

## Local Transport Security

The design assumes local-only transports.

Required properties:

1. socket or pipe permissions limited to the current OS user
2. no network listener in the first implementation
3. no unauthenticated localhost HTTP bridge
4. protocol version negotiation on every extension registration

## Release Channel Separation

Development, staging, and production channels must remain isolated.

Rules:

1. each channel uses a distinct extension ID
2. each channel uses a distinct Native Messaging manifest
3. production binaries must not trust development extension IDs

## Threat Scenarios and Expected Mitigations

### Host provides a misleading description

Mitigation:

- extension ignores `display_hint` as the source of truth
- extension renders from canonical payload bytes

### Duplicate sign request after host retry

Mitigation:

- host persists `request_id`
- daemon owns terminal state
- status polling happens before creating a replacement request

### Extension disconnects after UI presentation

Mitigation:

- request remains in daemon store
- recovery policy determines whether re-delivery is allowed
- transport loss alone does not imply approval or rejection

### Local process probes the daemon socket

Mitigation:

- OS-user-scoped local transport
- strict request validation
- no privileged signing operation without extension approval

### Dev extension accidentally talks to production daemon

Mitigation:

- channel-specific manifest and ID allowlists
- version compatibility checks at registration time

## Security Decisions That Must Be Closed Before Implementation

The following decisions must be finalized before coding starts:

1. whether account listing is gated by user approval
2. whether signed results are single-read or bounded multi-read
3. whether message signing may ever use relaxed policy
4. what exact transaction classes are considered unsupported for approval
5. what exact recovery rules apply after `request.presented`

## Non-Goals

This security model does not define:

- the exact daemon wire protocol
- the exact database schema
- the exact UI layout

Those belong in follow-up documents, but they must comply with the invariants defined here.
