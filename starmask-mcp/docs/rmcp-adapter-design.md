# Starmask MCP RMCP Adapter Design

## Purpose

This document defines how `starmask-mcp` should use the Rust MCP SDK `rmcp` without leaking SDK concerns into core wallet logic.

## Design Goal

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

## Layering

Recommended layering inside the `starmask-mcp` crate:

```text
main
  -> rmcp server setup
  -> tool handler methods
  -> daemon client
  -> starmask-core typed views and error mapping
```

## Crate Shape

Recommended module layout:

```text
starmask-mcp/src/
  main.rs
  server.rs
  tools.rs
  dto.rs
  daemon_client.rs
  error_mapping.rs
```

The first implementation may keep a small tool set inside `server.rs` instead of splitting out
`tools.rs`, as long as:

1. the MCP adapter remains thin
2. tool-specific business policy still lives in `starmaskd`
3. the file is split once tool count or complexity grows materially

## RMCP Responsibilities

`rmcp` should own:

- stdio server transport
- MCP tool registration
- MCP request dispatch

Project code should own:

- tool argument structs
- tool-to-daemon mapping
- daemon client protocol
- result projection

## Tool Registration

Recommended tools:

- `wallet_status`
- `wallet_list_accounts`
- `wallet_get_public_key`
- `wallet_request_sign_transaction`
- `wallet_get_request_status`
- `wallet_cancel_request`
- `wallet_sign_message`

These should mirror the design contract exactly and should not introduce MCP-only semantics that the daemon does not understand.

## Input Mapping

Recommended flow for each tool:

1. `rmcp` deserializes tool params into a tool DTO
2. tool DTO validates obvious shape errors
3. tool DTO converts into daemon-client request struct
4. daemon client performs local RPC call
5. daemon response converts into MCP-visible result DTO

Rule:

- keep one conversion step in each direction
- avoid building JSON by hand in tool handlers

## Daemon Client Boundary

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
- `create_sign_transaction_request`
- `create_sign_message_request`
- `get_request_status`
- `cancel_request`

The first implementation may ship with one concrete local JSON-RPC client.

## Error Mapping

The adapter should centralize error translation.

Recommended layers:

1. daemon transport error
2. daemon protocol error
3. MCP tool error/result mapping

Mapping rules:

1. shared error codes should remain visible in structured outputs where possible
2. transport failures should become a clear tool failure, not a fake domain status
3. retryable hints from the daemon should be preserved

## Output Shape

Tool outputs should preserve:

- `request_id`
- `client_request_id`
- `status`
- `result_kind`
- `result_available`
- `result_expires_at`
- `error_code`

Do not compress these into plain English strings.

## Logging

The MCP adapter should log:

- tool name
- request id where applicable
- shared error code on failure

It should not log:

- raw signatures
- raw signed transaction blobs

## Runtime Model

The first implementation should use a small Tokio runtime.

Recommended model:

- one async main
- one `rmcp` server
- daemon client calls over local IPC

Because the adapter does not own mutable lifecycle state, it does not need a coordinator.

## Inspector and Manual Testing

The adapter should be easy to test with an MCP inspector.

Recommended workflow:

1. run `starmaskd`
2. run `starmask-mcp`
3. connect with MCP Inspector over stdio
4. call wallet tools
5. verify structured results

## Fake Daemon Strategy

For adapter integration tests, prefer a fake daemon server that speaks the daemon JSON-RPC contract over local transport.

This allows testing:

- input validation
- error mapping
- result mapping
- stdio server behavior

without needing a real extension.

## Ready-to-Implement Checklist

This document is implementation-ready when:

1. tool DTO structs exist
2. daemon client trait exists
3. one local JSON-RPC daemon client implementation exists
4. one `rmcp` server wiring module exists

At that point, `starmask-mcp` can be implemented without reopening daemon or wallet semantics.
