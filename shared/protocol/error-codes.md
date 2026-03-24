# Shared Error Codes

## Purpose

This document defines a baseline shared error taxonomy for Starcoin MCP subprojects.

Subprojects may define additional project-local errors, but shared errors should preserve the same meaning across projects.

## Wallet and Signing Errors

- `wallet_unavailable`
  - No reachable wallet backend is available for the current user session.
- `wallet_locked`
  - The wallet is reachable but locked and cannot complete the requested operation.
- `wallet_selection_required`
  - More than one wallet instance can satisfy the request and the caller must select one explicitly.
- `wallet_instance_not_found`
  - The referenced wallet instance does not exist or is not currently known to the local daemon.
- `extension_not_connected`
  - The browser extension or wallet frontend is not connected to the local bridge.
- `invalid_account`
  - The referenced account does not exist or is not exposed to the current session.
- `request_not_found`
  - The referenced signing request does not exist.
- `request_expired`
  - The request exceeded its TTL and is no longer actionable.
- `request_rejected`
  - The user explicitly rejected the request.
- `request_cancelled`
  - The request was cancelled before completion.
- `invalid_transaction_payload`
  - The supplied transaction payload is malformed or unsupported.
- `unsupported_chain`
  - The request references a chain that the wallet cannot handle.
- `internal_bridge_error`
  - A local transport or bridge failure occurred between MCP, daemon, native host, or extension.
- `result_unavailable`
  - A request exists but no terminal result is available yet for retrieval.
- `idempotency_key_conflict`
  - The caller retried a create operation with the same client idempotency key but a different payload.
- `protocol_version_mismatch`
  - The caller and callee do not support a mutually compatible local bridge protocol version.

## Node and Chain Errors

- `node_unavailable`
  - The target Starcoin node cannot be reached.
- `rpc_unavailable`
  - The backing RPC service is unavailable or unhealthy.
- `invalid_chain_context`
  - The requested operation conflicts with the active network or chain configuration.
- `simulation_failed`
  - Transaction simulation completed but returned a failed execution status.
- `submission_failed`
  - The signed transaction could not be accepted by the txpool or submission endpoint.

## Policy and Access Errors

- `permission_denied`
  - The operation is not allowed by local policy.
- `approval_required`
  - The operation requires explicit user approval before proceeding.
- `rate_limited`
  - The operation has been rejected due to local rate-limiting policy.
- `unsupported_operation`
  - The requested operation is recognized but not implemented in the current environment.

## Error Shape Guidance

Shared errors should be represented with:

- `code`
- `message`
- optional `details`
- optional `retryable`

Example:

```json
{
  "code": "wallet_unavailable",
  "message": "No connected Starmask wallet instance was found.",
  "retryable": true
}
```
