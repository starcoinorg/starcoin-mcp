# Starmask MCP RMCP Adapter Design

## 1. Purpose

This document defines how `starmask-mcp` should use the Rust MCP SDK `rmcp` without leaking SDK
concerns into the unified wallet coordination core.

The adapter remains thin even after the product broadens beyond a browser-extension-only wallet.

## 2. Design Goal

The MCP adapter should do only four things:

1. expose tools to the MCP host
2. validate and deserialize tool input
3. call `starmaskd` through one local daemon client abstraction
4. map daemon results into MCP tool responses

It must not own:

- lifecycle state transitions
- wallet policy
- persistence
- backend registration logic
- Native Messaging logic
- local account prompt logic

## 3. Layering

Recommended layering inside the `starmask-mcp` crate:

```text
main
  -> rmcp server setup
  -> tool handler methods
  -> daemon client
  -> starmask-core typed views and error mapping
```

The adapter should know that multiple backend kinds exist, but it should not encode backend-local
rules that belong in the daemon or backend agent.

## 4. Crate Shape

Recommended module layout:

```text
starmask-mcp/src/
  lib.rs
  main.rs
  server.rs
  tools.rs
  dto.rs
  daemon_client.rs
  error_mapping.rs
  views.rs
```

The first implementation may keep a smaller file layout if the adapter remains thin, but once
multiple tool DTOs and output projections exist, a dedicated `tools.rs` and `views.rs` layout is
preferable.

## 5. RMCP Responsibilities

`rmcp` should own:

- stdio server transport
- MCP tool registration
- MCP request dispatch

Project code should own:

- tool argument structs
- tool-to-daemon mapping
- daemon client protocol
- result projection
- domain error mapping

## 6. Tool Registration

Recommended tools:

- `wallet_status`
- `wallet_list_accounts`
- `wallet_get_public_key`
- `wallet_request_unlock`
- `wallet_request_sign_transaction`
- `wallet_sign_message`
- `wallet_get_request_status`
- `wallet_cancel_request`

These tools must mirror the coordinator contract exactly and should not invent MCP-only semantics
that the daemon does not understand.

## 7. Input Mapping

Recommended flow for each tool:

1. `rmcp` deserializes tool params into a tool DTO
2. the tool DTO validates obvious shape errors
3. the tool DTO converts into one daemon-client request struct
4. the daemon client performs one local RPC call
5. the daemon response converts into one MCP-visible result DTO

Rules:

- keep one conversion step in each direction
- avoid building JSON by hand in tool handlers
- preserve optional fields rather than flattening everything into strings

## 8. Daemon Client Boundary

The adapter should depend on one daemon client abstraction.

Recommended trait:

- `DaemonClient`

Recommended methods:

- `system_ping`
- `system_get_info`
- `wallet_status`
- `wallet_list_instances`
- `wallet_list_accounts`
- `wallet_get_public_key`
- `create_unlock_request`
- `create_sign_transaction_request`
- `create_sign_message_request`
- `get_request_status`
- `cancel_request`

The first implementation may ship with one concrete local JSON-RPC client.

## 9. Tool DTO Design

DTOs should stay close to the external MCP schema.

Examples:

- `WalletStatusParams`
- `WalletListAccountsParams`
- `WalletGetPublicKeyParams`
- `WalletRequestUnlockParams`
- `WalletRequestSignTransactionParams`
- `WalletSignMessageParams`
- `WalletGetRequestStatusParams`
- `WalletCancelRequestParams`

Validation that belongs in DTOs:

- required-field presence
- obvious hex and length checks
- enum parsing
- TTL shape validation before daemon-side clamping

Validation that does not belong in DTOs:

- route resolution
- backend capability checks
- lock-state checks
- idempotency checks

## 10. Output Shape

Tool outputs should preserve structured coordinator fields.

Important fields that must remain structured:

- `request_id`
- `client_request_id`
- `wallet_instance_id`
- `backend_kind`
- `status`
- `result_kind`
- `result_available`
- `result_expires_at`
- `error_code`

Do not compress these into plain English summaries.

## 11. Error Mapping

The adapter should centralize error translation.

Recommended layers:

1. daemon transport error
2. daemon protocol error
3. MCP tool error or result mapping

Mapping rules:

1. shared error codes should remain visible in structured outputs where possible
2. transport failures should become explicit tool failures, not fake domain statuses
3. retryable hints from the daemon should be preserved
4. input validation errors should map to invalid-request style tool errors

## 12. Library Packaging

The `starmask-mcp` crate may be packaged as both:

- a standalone binary entrypoint
- an embeddable Rust library used by another local host binary

Recommended public facade:

- `DaemonClient`
- `LocalDaemonClient`
- `StarmaskMcpServer<C>`
- `serve_stdio(client)`
- `serve_stdio_with_config(client, config)`
- `default_socket_path()`

Packaging rules:

- `main.rs` stays a thin CLI and tracing wrapper
- library callers may own Tokio runtime setup and tracing initialization
- the same server wiring is reused in both standalone and embedded modes

## 13. Runtime Model

The adapter should use a small async runtime.

Recommended model:

- one async main
- one `rmcp` server
- daemon client calls over local IPC

Because the adapter does not own mutable lifecycle state, it does not need a coordinator or
background sweeper logic inside the MCP process.

## 14. Logging

The MCP adapter should log:

- tool name
- request ID where applicable
- shared error code on failure

It should not log:

- raw signatures
- raw signed transaction blobs
- unlock prompts or password-bearing content

## 15. Inspector and Manual Testing

The adapter should be easy to test with MCP Inspector.

Recommended workflow:

1. run `starmaskd`
2. run one or more backend agents
3. run `starmask-mcp`
4. connect with MCP Inspector over stdio
5. call wallet tools
6. verify structured results across multiple backend kinds

## 16. Fake Daemon Strategy

For adapter integration tests, prefer a fake daemon server that speaks the daemon contract over
local transport.

This allows testing:

- input validation
- error mapping
- result mapping
- stdio server behavior
- backend-kind-specific response projection

without needing a real browser extension or local account vault.

## 17. Ready-to-Implement Checklist

This document is implementation-ready when:

1. tool DTO structs exist
2. the daemon client trait exists
3. one local JSON-RPC daemon client implementation exists
4. one `rmcp` server wiring module exists
5. adapter tests cover unlock, sign-transaction, sign-message, and error mapping paths

At that point, `starmask-mcp` can be implemented without reopening coordinator or backend
semantics.
