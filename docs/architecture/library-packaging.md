# Library Packaging Model

## Purpose

This document defines how Starcoin MCP servers should be packaged when a Rust host wants to:

- launch them as standalone binaries
- embed them in-process as libraries

The goal is to support both forms without duplicating MCP handler wiring or collapsing trust boundaries.

## Design Goal

Packaging must not change responsibility boundaries.

Repository-level rule:

- `starcoin-node-mcp` remains chain-facing only
- `starmask-mcp` remains wallet-facing only
- embedding both into one host binary must not create a combined signer-and-node authority

## Common Packaging Pattern

Each MCP project should follow the same high-level shape:

1. domain and adapter crates stay independent of `rmcp`
2. one MCP-facing server crate owns `rmcp` integration
3. the server crate exposes a small library facade
4. a thin binary entrypoint handles CLI parsing, config loading, and default tracing setup

Recommended public surface for the MCP-facing server crate:

- one server constructor such as `*McpServer::new(deps)`
- one stdio serving helper such as `serve_stdio(deps)`
- optionally one config bootstrap helper such as `serve_stdio_with_config(config)` when bootstrap belongs naturally in that crate

The library facade should not:

- initialize tracing implicitly
- spawn an extra Tokio runtime when the caller already owns one
- bypass existing typed boundaries in core or daemon crates

## Host Ownership Rules

When a host embeds an MCP server as a library:

- the host owns Tokio runtime setup
- the host owns tracing initialization
- the host chooses whether to bootstrap from config or construct dependencies directly
- the embedded MCP server still speaks MCP over the selected transport boundary

This means the host reuses the same MCP adapter logic as the standalone binary rather than re-implementing tool registration and dispatch.

## Wallet MCP Packaging

`starmask-mcp` already has a separate daemon boundary.

Recommended packaging model:

- `starmask-core` and `starmaskd` remain independent of `rmcp`
- the `starmask-mcp` crate exposes the MCP adapter as a library as well as a binary
- the adapter depends on a `DaemonClient` trait rather than only one concrete local client
- `LocalDaemonClient` remains the default implementation for stdio hosts that talk to `starmaskd` over local IPC

Recommended public facade:

- `DaemonClient`
- `LocalDaemonClient`
- `StarmaskMcpServer<C>`
- `serve_stdio(client)`
- `default_socket_path()`

This keeps wallet lifecycle and persistence in `starmaskd` while allowing another Rust binary to reuse the same MCP adapter in-process.

## Node MCP Packaging

`starcoin-node-mcp` does not need a daemon in the first release.

Recommended packaging model:

- `starcoin-node-mcp-core` owns typed app bootstrap and orchestration
- `starcoin-node-mcp-rpc` owns endpoint probing and RPC normalization
- `starcoin-node-mcp-server` owns `rmcp` integration and exports a library facade
- the standalone `starcoin-node-mcp` binary becomes a thin wrapper around that facade

Recommended public facade:

- `AppContext::bootstrap(config)`
- `StarcoinNodeMcpServer::new(app)`
- `serve_stdio(app)`
- optionally `serve_stdio_with_config(config)`

## Non-Goal

The repository should not introduce one monolithic combined library that merges wallet and node MCP behavior into a single trust domain.

If one host binary wants both capabilities, it should link both MCP server libraries separately and preserve their distinct dependency graphs and failure handling.

## Migration Strategy

Recommended order:

1. add `lib.rs` to the MCP-facing crate without changing tool semantics
2. move stdio serving logic into library helpers
3. reduce `main.rs` to CLI and tracing glue
4. introduce dependency traits where a server is still bound to one concrete transport client
5. update docs and acceptance checks so both standalone and embedded entrypoints stay aligned
