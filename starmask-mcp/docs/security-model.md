# Starmask MCP Unified Wallet Security Model

## 1. Purpose

This document defines the security model for the `starmask-mcp` stack after expanding it from a
browser-extension-only design into a unified local wallet coordination system.

The stack now includes:

- `starmask-mcp`
- `starmaskd`
- wallet backend agents
- `starmask-native-host` for browser transport
- the Starmask browser extension when that backend is enabled
- a local account backend built on Starcoin `AccountProvider` when enabled
- the local MCP host

## 2. Primary Security Goal

The primary goal is:

`starmask-mcp` must let a local MCP host request wallet operations without granting that host
signing authority, password access, or private key access.

## 3. Security Invariants

The following rules must remain true in every implementation phase:

1. Private key material never leaves the selected signer backend.
2. `starmaskd` never creates a signature.
3. `starmask-mcp` never creates a signature and never stores unlock secrets.
4. The MCP host never becomes the source of truth for what the user is approving.
5. Approval surfaces render from canonical payload bytes or canonical parsed fields derived by the
   backend itself.
6. Every signing request is attributable to exactly one wallet instance and one account address.
7. Non-terminal request state is durable across process restarts.
8. Local IPC is restricted to the current OS user.
9. Development-only backends remain explicitly isolated from production channels.

## 4. Trust Boundaries

### 4.1 Trusted for signing

Only the selected wallet backend is trusted for signing.

Examples:

- the Starmask extension for `starmask_extension`
- the local account agent for `local_account_dir`
- the dev-only private-key agent for `private_key_dev`

### 4.2 Trusted for routing and persistence, but not signing

- `starmaskd`

### 4.3 Trusted for transport adaptation only

- `starmask-mcp`
- `starmask-native-host`

### 4.4 Untrusted for security decisions

- MCP host output
- `display_hint`
- free-form client context
- any human-readable transaction summary not derived by the signer backend

## 5. Assets to Protect

The design must protect:

1. private keys
2. unlock passwords and unlock tokens
3. signed transaction results before submission
4. message signatures
5. account directory contents
6. request integrity and idempotency
7. wallet routing integrity
8. user intent at approval time

## 6. Threat Model

The first implementation should explicitly defend against:

1. a buggy or over-eager MCP host
2. misleading `display_hint` or client-generated transaction summaries
3. replayed or conflicting `client_request_id` values
4. daemon restart during active requests
5. backend restart during active requests
6. ambiguous routing when the same address appears in multiple wallet instances
7. accidental use of development signer backends in production
8. insecure local account directory permissions
9. local prompt flooding caused by excessive request fan-out

The first implementation does not attempt to defend against:

1. a fully compromised OS user account
2. malware with the same OS-user privileges and direct filesystem access
3. a malicious browser runtime with arbitrary extension code execution
4. a hostile local administrator

Those remain real risks, but they are outside the practical trust model of a local wallet
integration.

## 7. Backend-Specific Security Expectations

### 7.1 `starmask_extension`

Required properties:

- the extension remains the signing authority
- the extension renders transactions and messages from canonical payload bytes
- the extension may show `display_hint`, but only as secondary context
- production daemons trust only production extension IDs

### 7.2 `local_account_dir`

Required properties:

- the local account agent is the signing authority
- the account directory path is resolved to a real path before use
- owner-only filesystem permission checks are enforced before opening the vault
- unlock happens inside the local account agent, not in `starmaskd` or `starmask-mcp`
- the approval surface is a trusted local prompt, not MCP chat output

### 7.3 `private_key_dev`

Required properties:

- disabled by default
- blocked in production channel
- clearly marked unsafe in status output and logs
- never silently enabled by environment-variable presence alone

## 8. Approval Security Rules

The approval surface is the final user decision point.

Rules:

1. the signer backend must decode and present the transaction or message itself
2. unsupported transaction payloads must be rejected rather than blindly signed
3. the signer backend may display `display_hint`, but must not trust it as the source of truth
4. explicit approval is required for every sign request in the first release
5. unlock approval and sign approval are separate flows

### 8.1 Required transaction approval fields

Transaction approval must present:

- account address
- chain ID
- transaction kind
- receiver or called function
- amount where applicable
- gas budget and gas price
- expiration

### 8.2 Required message approval fields

Message approval must present:

- account address
- message format
- message content or a safe canonical preview
- origin or client context when known

### 8.3 Required unlock approval fields

Unlock approval must present:

- wallet instance label
- target account if one account is being unlocked
- requested unlock TTL
- backend-local warning that unlock grants future signing authority for a bounded period

## 9. Password and Unlock Handling

Passwords and unlock secrets are especially sensitive.

Rules:

1. MCP tools must never accept account passwords as parameters.
2. `starmaskd` must never persist account passwords.
3. Local account unlock must happen entirely inside the local account backend.
4. Unlock state is backend-local and must not be inferred from MCP-host memory.
5. Unlock TTL must be bounded and clamped.
6. Process exit must clear backend-local unlock state where feasible.

### 9.1 Starcoin `AccountProvider` hardening note

The current Starcoin `AccountManager` caches unlock passwords in memory as ordinary `String`
values. That implementation detail is acceptable for local development, but production-grade
`local_account_dir` support should harden it with zeroizing secret containers or an equivalent
approach.

Until that hardening lands, the local-account backend must:

- keep unlock TTL bounded
- avoid copying passwords across layers
- avoid logging any password-bearing structures

## 10. Request Integrity Rules

Every request must have:

- a cryptographically strong `request_id`
- a deterministic `payload_hash`
- `created_at`
- `expires_at`
- one canonical status owner, which is `starmaskd`

The coordinator must reject:

- requests missing required fields
- malformed payloads
- expired requests
- requests routed to unknown wallet instances
- requests that target a backend without the required capability
- duplicate `client_request_id` values with conflicting payload hashes

## 11. Routing Security

Wallet routing must be explicit and deterministic.

Rules:

1. if the caller names `wallet_instance_id`, only that instance may receive the request
2. if multiple wallet instances match and none is selected, the coordinator must fail with
   `wallet_selection_required`
3. auto-routing is allowed only when exactly one matching instance exists
4. account identity alone is insufficient when more than one wallet instance exposes the same
   address
5. after `pending_user_approval`, the request must remain bound to the same wallet instance

## 12. Result Handling Rules

Signed outputs are sensitive even when private keys are not exposed.

Rules:

1. result retention must be bounded and documented
2. result lookup must be keyed by `request_id`
3. logs must never print raw signatures or signed transaction blobs by default
4. cancellation and expiry must eventually clear retained results

## 13. Filesystem and Local Transport Security

The first release assumes local-only transports.

Required properties:

1. socket or pipe permissions are limited to the current OS user
2. there is no network listener
3. there is no unauthenticated localhost HTTP bridge
4. backend registration includes protocol-version checking
5. local account directories must reject obviously insecure ownership or permission states

For account-directory safety checks, the local backend should reject at least:

- world-writable vault directories
- symlink-swapped directory roots
- missing parent directories created by a different user when ownership cannot be verified

## 14. Release Channel Separation

Development, staging, and production channels must remain isolated.

Rules:

1. each channel uses a distinct backend allowlist and trust policy
2. Native Messaging manifests are channel-specific
3. production binaries must not trust development extension IDs
4. `private_key_dev` must be blocked in production

## 15. Performance as a Security Property

Some performance controls are also security controls.

Required behavior:

1. per-instance approval concurrency must be bounded to avoid prompt flooding
2. maintenance sweeps must expire abandoned requests promptly
3. bounded unlock TTL reduces the blast radius of a stolen local session
4. bounded result retention reduces exposure of signed artifacts
5. metadata caching is allowed, but decrypted key material must not become a long-lived cache

## 16. Threat Scenarios and Required Mitigations

### 16.1 Host provides a misleading transaction summary

Mitigation:

- signer backend ignores `display_hint` as the source of truth
- signer backend renders from canonical payload bytes

### 16.2 Duplicate request replay after host retry

Mitigation:

- `client_request_id` and `payload_hash` are checked together
- the coordinator returns the existing request when the replay is identical
- conflicting replays fail closed

### 16.3 Backend disconnects after approval UI is shown

Mitigation:

- the request remains durable in coordinator storage
- only the same wallet instance may resume it
- transport loss alone never implies approval

### 16.4 Local process probes the daemon socket

Mitigation:

- OS-user-scoped local transport
- strict request validation
- no privileged signing without backend-local approval

### 16.5 Production daemon accidentally enables a dev-only signer backend

Mitigation:

- explicit channel gating
- explicit unsafe-backend configuration
- startup validation rejects forbidden backend kinds

### 16.6 Multiple requests target the same local account simultaneously

Mitigation:

- bound active presentations per wallet instance or account
- prefer serialized interactive approval
- document sender-level serialization when composing with `starcoin-node-mcp`

## 17. First-Release Closed Decisions

The first release is closed on these security decisions:

1. account listing is not gated by an interactive approval step
2. public-key lookup may use cached metadata
3. passwords do not cross MCP
4. unsupported transaction payloads are rejected rather than blindly signed
5. after `request.presented`, only the same `wallet_instance_id` may resume the request
6. dev-only signer backends are disabled by default

## 18. Non-Goals

This security model does not define:

- the exact daemon wire protocol
- the exact database schema
- the exact UI layout

Those belong in follow-up documents, but they must comply with the invariants defined here.
