# Starmask Runtime Security Model

## Status

This document is the authoritative `v1` security model for the shipped
extension-backed daemon stack.

Repository status note: the in-tree `crates/starmask-runtime` adapter has been removed. References to
`starmask-runtime` in this document describe the historical MCP transport role or a possible future
external adapter; the current shipped Rust binaries are `starmaskd`, `starmask-native-host`, and
related daemon-side components.

Future multi-backend security evolution is tracked separately in:

- `docs/unified-wallet-coordinator-evolution.md`
- `docs/wallet-backend-security-model.md`

## 1. Purpose

This document defines the security model for the extension-backed `v1` deployment
centered on `starmaskd`.

The current stack includes:

- `starmaskd`
- `starmask-native-host`
- Starmask Chrome extension
- the local MCP host
- an optional external `starmask-runtime` adapter that terminates at the same
  daemon boundary

## 2. Security Goal

The primary security goal is:

The daemon stack must allow a local MCP host to request wallet actions without
granting that host signing authority or private key access.

## 3. Security Invariants

The following rules must remain true in the current implementation:

1. Private key material never leaves the extension.
2. The daemon never creates a signature.
3. The MCP host never becomes the source of truth for what the user is approving.
4. The extension renders the transaction or message from canonical payload bytes.
5. Every signing request is attributable to one local account and one logical wallet instance.
6. Non-terminal request state is durable across process restarts.
7. Local transport is restricted to the current OS user.

## 4. Trust Boundaries

### Trusted for signing

- Starmask extension

### Trusted for persistence and routing, but not signing

- `starmaskd`

### Trusted for transport adaptation only

- `starmask-native-host`
- any external `starmask-runtime` adapter

### Untrusted for security decisions

- MCP host output
- `display_hint`
- any transaction summary not derived by the extension itself

## 5. Assets to Protect

The design must protect:

1. private keys
2. unlock state
3. signed transaction results before submission
4. message signatures
5. request integrity
6. wallet-instance routing integrity
7. user intent at approval time

## 6. Threat Model

The current implementation should explicitly defend against:

1. a buggy or overly aggressive MCP host
2. a local process attempting to connect to daemon transport as the same user
3. stale or replayed request identifiers
4. daemon restart during an active signing flow
5. browser or extension restart during an active signing flow
6. extension mismatch across dev, staging, and production channels
7. misleading host-provided summaries

The current implementation does not attempt to defend against:

1. a fully compromised OS user account
2. a malicious browser runtime with arbitrary extension code execution
3. malware with the same OS-user privileges and direct access to the local
   daemon transport

## 7. Approval Security Rules

The extension approval UI is the final decision point.

Rules:

1. the extension must decode and present the transaction itself
2. the extension may show `display_hint`, but only as secondary context
3. the extension must show enough information for an informed decision
4. unsupported transaction payloads must not fall back to silent blind signing
5. every sign request requires explicit approval

### 7.1 Required transaction approval fields

- account address
- chain ID
- transaction kind
- receiver or called function
- amount where applicable
- gas budget and gas price
- expiration

### 7.2 Required message approval fields

- account address
- message format
- message content or a safe canonical preview
- origin context when known

## 8. Request Integrity Rules

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
- conflicting `client_request_id` replays with a different payload hash

## 9. Result Handling Rules

Signed outputs are sensitive even when private keys are not exposed.

Rules:

1. signed results must be retained for a limited time only
2. result availability policy must be explicit and documented
3. result retrieval must be keyed by `request_id`
4. daemon logs must never print full signed payloads by default

## 10. Routing Security

Wallet routing must be explicit and deterministic.

Rules:

1. if the caller names `wallet_instance_id`, only that instance may receive the request
2. if multiple wallet instances match and none is selected, the daemon must fail with
   `wallet_selection_required`
3. auto-routing is allowed only when exactly one matching instance exists
4. account identity alone is insufficient when more than one wallet instance exposes the same
   address

## 11. Local Transport Security

The design assumes local-only transports.

Required properties:

1. socket or pipe permissions limited to the current OS user
2. no network listener
3. no unauthenticated localhost HTTP bridge
4. protocol version negotiation on every extension registration

### 11.1 Unix socket and future named-pipe hardening

Required deployment rules:

These rules are product-grade closure requirements for the local transport boundary. They define
the required end state for production deployments and TUI supervision rather than asserting that
every current binary already enforces each check automatically.

1. the daemon listener must live inside a private per-user runtime directory
2. on POSIX, the socket parent directory must be locked to the current user and the socket itself
   must also be current-user only
3. deployments must not place the daemon socket directly in a shared writable directory such as
   `/tmp`; if such a base directory is unavoidable, the runtime must first create a private
   subdirectory
4. stale socket cleanup must happen only after a failed connect attempt and only for a path inside
   an owned private runtime directory
5. cleanup logic must not follow symlinks or future Windows reparse-point equivalents while
   removing stale transport artifacts
6. future Windows named pipes must use an ACL restricted to the current user or service SID and
   must not grant broad access to `Everyone` or similar groups

### 11.2 Native-host deployment hardening

Required deployment rules:

1. each release channel must use an exact `allowed_origins` allowlist with no wildcard entries
2. the Native Messaging manifest file must be owner-writable only
3. the manifest must point to an absolute native-host binary path
4. the native-host binary and its parent directories must not be writable by other OS users
5. production manifests must not point to development binaries or development channel IDs
6. operator-facing supervisors must not keep `starmask-native-host` alive outside the browser-owned
   lifecycle

## 12. Release Channel Separation

Development, staging, and production channels must remain isolated.

Rules:

1. each channel uses a distinct extension ID
2. each channel uses a distinct Native Messaging manifest
3. production binaries must not trust development extension IDs
4. each channel uses distinct runtime directories, daemon sockets or pipes, and databases

## 13. Rust Implementation Security Notes

The Rust workspace should default to:

- `#![forbid(unsafe_code)]`

If a platform shim requires unsafe code:

- isolate it outside the core lifecycle and persistence crates
- document the safety invariant
- keep the unsafe surface minimal and auditable

Additional Rust guidance:

1. request IDs, lease IDs, and wallet-instance IDs should be distinct newtypes
2. lifecycle states should be enums, not mutable free-form strings
3. redaction helpers should be used for sensitive log fields

## 14. Threat Scenarios and Expected Mitigations

### Host provides a misleading description

Mitigation:

- extension ignores `display_hint` as the source of truth
- extension renders from canonical payload bytes

### Duplicate sign request after host retry

Mitigation:

- host persists `request_id`
- daemon owns terminal state
- status polling happens before creating a replacement request
- conflicting `client_request_id` reuse fails with `idempotency_key_conflict`

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

### Dev extension accidentally talks to a production daemon

Mitigation:

- channel-specific manifest and ID allowlists
- version compatibility checks at registration time

## 15. Non-Goals

This security model does not define the planned generic signer-backend model or
the multi-backend `local_account_dir` trust extensions. That follow-on work is
tracked in `docs/unified-wallet-coordinator-evolution.md` and
`docs/wallet-backend-security-model.md`.
