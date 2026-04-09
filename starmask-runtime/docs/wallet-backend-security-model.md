# Starmask Wallet Backend Security Model

## Status

This document is the phase-2 security model for the current multi-backend implementation.

It supplements, rather than replaces, the current extension-backed security model in:

- `docs/security-model.md`

## 1. Purpose

This document defines the security rules for adding non-extension signer backends without breaking
the core trust boundaries of the stack.

It applies to:

- `starmaskd` as coordinator
- `starmask-native-host` or an external MCP transport adapter
- generic wallet backend agents
- `local_account_dir`

## 2. Primary Security Goal

The primary phase-2 goal is:

`starmaskd` must allow a local MCP host to request signatures from more than one
backend kind without giving the MCP host private key access, password access, or
authority over the approval surface.

## 3. Non-Negotiable Invariants

These rules remain mandatory:

1. the host transport adapter, such as `starmask-native-host` or an external MCP adapter, never signs
2. `starmaskd` never signs
3. private keys remain inside the selected backend
4. passwords or unlock secrets never cross the MCP boundary
5. canonical payload bytes remain the source of truth for approval
6. ambiguous routing fails closed
7. local OS transport remains same-user scoped

## 4. Trust Boundaries

Trusted for signing and unlock decisions:

- the selected wallet backend agent

Trusted for persistence, routing, and recovery, but not signing:

- `starmaskd`

Trusted for protocol adaptation only:

- `starmask-native-host`
- any external MCP transport adapter

Untrusted for approval truth:

- MCP host output
- `display_hint`
- any summary not derived by the backend from canonical payload bytes

## 5. Backend-Specific Security Rules

### 5.1 `starmask_extension`

Phase-2 keeps the existing `v1` extension trust model.

### 5.2 `local_account_dir`

Required properties:

1. password entry happens only in a backend-local prompt
2. transaction and message rendering happen in that same local prompt
3. decrypted key material is never written to the coordinator database
4. unlock cache is bounded by a short TTL
5. the current implementation uses `tty_prompt`; richer local prompt surfaces remain future work

## 6. Password and Unlock Boundaries

Phase-2 design choice:

1. there is no MCP-visible password field
2. there is no daemon JSON-RPC method that accepts a password
3. phase 2 does not require `wallet_request_unlock`
4. a locked backend without `unlock` capability must fail signing requests with `wallet_locked`
5. a locked backend with `unlock` capability may collect the password locally during signing
6. if that backend-local unlock fails or is cancelled, the request still terminates with
   `wallet_locked`

This keeps password handling out of MCP and out of the coordinator while phase 3 unlock flows are
still pending.

## 7. Local Approval Rules

Every backend approval surface must render from canonical payload bytes.

Required transaction fields:

- account address
- chain ID
- transaction kind
- receiver or called function
- amount where applicable
- gas budget and gas price
- expiration

Required message fields:

- account address
- message format
- safe content preview
- origin context when known

Rules:

1. `display_hint` may be shown only as secondary context
2. unsupported payloads must not degrade to blind signing
3. local prompts must present enough information for an informed decision

## 8. Filesystem Security for `local_account_dir`

Phase-2 `local_account_dir` support must fail fast on insecure local storage.

Required checks:

1. canonicalize the configured `account_dir`
2. reject missing or non-directory paths
3. reject paths owned by another OS user
4. on POSIX, reject group-writable or world-writable account directories
5. reject symlink escapes that move secret material outside the canonical account directory

Windows implementations must enforce the equivalent current-user-only ACL policy.

Operational deployment rules:

1. backend-agent logs, pid files, and copied diagnostics must not be written into the account
   directory itself unless those files follow the same owner-only policy
2. password prompts, temporary exports, or crash artifacts must not create world-readable copies of
   local-account secrets

## 9. Local Transport Security

The local backend transport must preserve:

1. Unix socket or named pipe only
2. same-user access boundary
3. no network listener
4. explicit configured backend identity
5. no secrets in wire payloads
6. daemon socket or pipe discovery from shared configuration rather than from untrusted host input
7. refusal to use a daemon transport path or future pipe ACL that is broader than current-user
   scope

`starmaskd` must reject backend registration for unknown or disabled backend IDs.

Product-grade deployment rules:

These rules define the required production posture for backend-to-daemon transport discovery. The
current local-development implementation still allows some convenience shortcuts, so this section
should be read as the required end state for hardened deployments and TUI closure.

1. backend agents must connect to the exact configured daemon socket or pipe rather than scanning
   public locations in production deployments
2. backend launchers and TUIs must not place passwords or other unlock material on argv
3. stale-socket cleanup, when present, must follow the same owned-private-directory rule as the
   daemon-side transport

## 10. Logging and Result Redaction

Phase-2 logs must never include:

1. plaintext passwords
2. private keys
3. full signed transaction bytes by default
4. full secret-file contents

Allowed log data:

- backend kind
- backend ID
- request ID
- payload hash
- terminal status
- truncated result identifiers

## 11. Memory and Cache Rules

Performance must not quietly weaken security.

Required rules:

1. account metadata may be cached
2. decrypted key material must not be cached longer than the local unlock TTL
3. password buffers should use zeroizing storage where practical
4. the coordinator must not store backend-local unlock material

## 12. Threat Scenarios and Expected Mitigations

### MCP host sends misleading transaction text

Mitigation:

- local backend prompt renders from canonical bytes
- `display_hint` is secondary only

### A same-user process tries to register a fake backend

Mitigation:

- daemon accepts only configured backend IDs
- backend kind must match the configured backend entry
- private keys still remain inside the real signer backend

### `local_account_dir` is group-writable

Mitigation:

- startup fails before the backend becomes routable

### Password leaks into logs

Mitigation:

- no password field exists in MCP or daemon RPC
- local prompt implementation owns password collection

## 13. Non-Goals

This document does not define:

- the current `v1` extension-only release contract
- package-manager deployment steps
- hardware-wallet support
- future development-only backends such as `private_key_dev`

## 14. Relationship to Other Documents

This document should be read together with:

- `docs/wallet-backend-agent-contract.md`
- `docs/wallet-backend-local-socket-binding.md`
- `docs/wallet-backend-configuration.md`
- `docs/wallet-backend-persistence-and-schema.md`
