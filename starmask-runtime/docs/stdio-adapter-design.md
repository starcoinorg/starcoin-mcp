# Starmask Runtime RMCP Adapter Design

## Status

This document is the authoritative design reference for the `v1` adapter that the repository used
to ship in-tree.

The `crates/starmask-runtime` adapter has been removed from the workspace. The material here now tracks
the historical tool set and daemon-client contract, plus possible future external adapter work.
Planned multi-backend expansion is tracked in `docs/unified-wallet-coordinator-evolution.md`.

## 1. Purpose

This document defines how `starmask-runtime` should use the Rust MCP SDK `rmcp` without leaking SDK
concerns into core wallet logic.

## 2. Design Goal

The MCP adapter should do only four things:

1. expose tools to the MCP host
2. validate and deserialize tool input
3. call the daemon through the local daemon client
4. map daemon results into MCP tool responses

It should not own:

- lifecycle state transitions
- wallet policy
- persistence
- Native Messaging logic

## 3. Layering

```text
main
  -> rmcp server setup
  -> tool handler methods
  -> daemon client
  -> starmask-core typed views and error mapping
```

## 4. Crate Shape

Recommended module layout:

```text
starmask-runtime/src/
  lib.rs
  main.rs
  server.rs
  dto.rs
  daemon_client.rs
  error_mapping.rs
```

The current implementation may keep the tool surface inside `server.rs` as long as the adapter
remains thin.

## 5. Current Tool Registration

The current adapter exposes:

- `wallet_status`
- `wallet_list_instances`
- `wallet_list_accounts`
- `wallet_get_public_key`
- `wallet_request_sign_transaction`
- `wallet_get_request_status`
- `wallet_cancel_request`
- `wallet_sign_message`

These tools must mirror the current daemon contract exactly.

## 6. Input Mapping

Recommended flow for each tool:

1. `rmcp` deserializes tool params into a tool DTO
2. the tool DTO validates obvious shape errors
3. the tool DTO converts into one daemon-client request struct
4. the daemon client performs one local RPC call
5. the daemon response converts into one MCP-visible result DTO

Rules:

- keep one conversion step in each direction
- avoid building JSON by hand in tool handlers

## 7. Daemon Client Boundary

The adapter should depend on one daemon client abstraction.

Recommended methods:

- `system_ping`
- `system_get_info`
- `wallet_status`
- `wallet_list_instances`
- `wallet_list_accounts`
- `wallet_get_public_key`
- `create_sign_transaction_request`
- `create_sign_message_request`
- `get_request_status`
- `cancel_request`

The current adapter does not expose `wallet_request_unlock`, because the daemon and tool surface do
not yet implement it.

## 8. Output Shape

Tool outputs should preserve structured daemon fields, including:

- `request_id`
- `client_request_id`
- `status`
- `result_kind`
- `result_available`
- `result_expires_at`
- `error_code`

Do not compress these into plain English summaries.

## 9. Error Mapping

The adapter should centralize error translation.

Recommended layers:

1. daemon transport error
2. daemon protocol error
3. MCP tool error or result mapping

Mapping rules:

1. shared error codes should remain visible in structured outputs where possible
2. transport failures should become clear tool failures, not fake domain statuses
3. input validation errors should map to invalid-request style tool errors

## 10. Library Packaging

The `starmask-runtime` crate may be packaged as both:

- a standalone binary entrypoint
- an embeddable Rust library used by another local host binary

Recommended public facade:

- `DaemonClient`
- `LocalDaemonClient`
- a future `StarmaskMcpServer<C>`-style adapter type
- a future `serve_stdio(client)` helper
- `default_socket_path()`

## 11. Logging

The MCP adapter should log:

- tool name
- request ID where applicable
- shared error code on failure

It should not log:

- raw signatures
- raw signed transaction blobs

## 12. Inspector and Manual Testing

The adapter should be easy to test with an MCP inspector.

Recommended workflow:

1. run `starmaskd`
2. run `starmask-runtime`
3. connect with MCP Inspector over stdio
4. call wallet tools
5. verify structured results

## 13. Deliberate `v1` Omissions

The current adapter design does not specify:

- explicit unlock tools
- backend-kind-aware tool responses
- local-account backends

Those additions are tracked in `docs/unified-wallet-coordinator-evolution.md`.
