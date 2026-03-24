# Shared Schemas

This directory contains repository-level shared envelopes intended to be reused by multiple Starcoin MCP subprojects.

Current schemas:

- `unsigned-transaction-envelope.schema.json`
  - canonical unsigned transaction object returned by chain-facing preparation tools
- `wallet-sign-request.schema.json`
  - canonical asynchronous wallet approval request shape
- `wallet-sign-result.schema.json`
  - canonical asynchronous wallet approval result shape

These schemas also capture:

- host-visible idempotency through `client_request_id`
- bounded result retention through `result_available` and `result_expires_at`

These schemas are intentionally narrow and may be extended as additional subprojects are added.
