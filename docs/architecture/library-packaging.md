# Library Packaging Model

## Purpose

This document defines how Starcoin host adapters should be packaged when a Rust host wants to:

- launch them as standalone binaries
- embed them in-process as libraries

The goal is to support both forms without duplicating host-adapter wiring or collapsing trust
boundaries.

## Design Goal

Packaging must not change responsibility boundaries.

Repository-level rule:

- `starcoin-node` remains chain-facing only
- `starmask-runtime` remains wallet-facing only
- the repository must not ship a merged library that blurs wallet-facing and chain-facing responsibilities

If an external host links both libraries into one process, that host is explicitly assuming a combined trust domain and must treat it as such.

## Common Packaging Pattern

Each adapter-facing subproject should follow the same high-level shape:

1. domain and adapter crates stay independent of `rmcp`
2. one host-adapter crate owns `rmcp` integration
3. the adapter crate exposes a small library facade
4. a thin binary entrypoint handles CLI parsing, config loading, and default tracing setup

Recommended public surface for the host-adapter crate:

- one server constructor such as `*McpServer::new(deps)`
- one stdio serving helper such as `serve_stdio(deps)`
- optionally one config bootstrap helper such as `serve_stdio_with_config(config)` when bootstrap belongs naturally in that crate

The library facade should not:

- initialize tracing implicitly
- spawn an extra Tokio runtime when the caller already owns one
- bypass existing typed boundaries in core or daemon crates

## Host Ownership Rules

When a host embeds an adapter as a library:

- the host owns Tokio runtime setup
- the host owns tracing initialization
- the host chooses whether to bootstrap from config or construct dependencies directly
- the embedded adapter still speaks MCP over the selected transport boundary

This means the host reuses the same adapter logic as the standalone binary rather than
re-implementing tool registration and dispatch.

## Wallet Adapter Packaging

`starmask-runtime` already has a separate daemon boundary.

The repository no longer ships an in-tree `starmask-runtime` adapter crate. If a host adapter is added
back later, it should remain a thin layer over the daemon client boundary instead of taking over
wallet lifecycle ownership.

Recommended packaging model:

- `starmask-core` and `starmaskd` remain independent of `rmcp`
- any future `starmask-runtime` crate should expose the host adapter as a library as well as a binary
- the adapter depends on a `DaemonClient` trait rather than only one concrete local client
- `LocalDaemonClient` remains the default implementation for stdio hosts that talk to `starmaskd` over local IPC

Recommended public facade:

- `DaemonClient`
- `LocalDaemonClient`
- a future `StarmaskMcpServer<C>`-style adapter type
- a future `serve_stdio(client)` helper
- `default_socket_path()`

This keeps wallet lifecycle and persistence in `starmaskd` while allowing another Rust binary to
reuse the same adapter in-process if one is reintroduced.

Keep the architecture boundary explicit between any future `starmask-runtime` transport adapter,
`starmaskd` (lifecycle owner and persistence owner), `starmask-native-host` (Chrome Native
Messaging bridge), and the Starmask extension (approval UI and signing authority).

## Node Adapter Packaging

`starcoin-node` does not need a daemon in the first release.

The repository no longer ships an in-tree `starcoin-node-server` crate. If a host adapter is
added back later, it should remain a thin wrapper over the existing libraries and CLI-facing app
bootstrap.

Recommended packaging model:

- `starcoin-node-core` owns typed app bootstrap and orchestration
- `starcoin-node-rpc` owns endpoint probing and RPC normalization
- `starcoin-node-cli` is the current thin executable wrapper around the shared app bootstrap
- any future host adapter should own `rmcp` integration as a separate thin crate

Recommended public facade:

- `AppContext::bootstrap(config)`
- CLI entrypoints should keep config loading and tracing at the binary boundary
- any future host adapter should expose its own `serve_stdio(app)`-style helper without changing
  core or RPC crates

## Operator TUI Packaging

The planned runtime supervision TUI is not a host adapter and should not be packaged as one.

First-pass packaging rule:

- implement the TUI as a separate operator-facing binary, ideally in its own top-level subproject
- supervise existing binaries such as `starmaskd`, `local-account-agent`, and an optional
  node-side service as child processes
- keep `starcoin-node-cli` short-lived and on-demand rather than turning it into a background
  daemon

Why this is the preferred first implementation:

1. it preserves the current wallet restart and recovery semantics owned by `starmaskd`
2. it avoids inventing a combined chain-plus-wallet in-process trust domain
3. it reuses already implemented CLIs and process boundaries
4. it aligns with the existing repository-local supervisor behavior in
   `plugins/starcoin-transfer-workflow/scripts/wallet_runtime.py`

If a later TUI iteration embeds libraries directly, that choice must be documented as a separate
design decision because it changes operational boundaries even if the signing trust boundary
remains intact.

## Non-Goal

The repository should not ship one monolithic combined library that merges wallet and node
adapter behavior into a single trust domain.

If one host binary wants both capabilities, it should link both adapter libraries separately and
preserve distinct wiring and failure-handling boundaries.

## Migration Strategy

Recommended order:

1. add `lib.rs` to the adapter-facing crate without changing tool semantics
2. move stdio serving logic into library helpers
3. reduce `main.rs` to CLI and tracing glue
4. introduce dependency traits where a server is still bound to one concrete transport client
5. update docs and acceptance checks so both standalone and embedded entrypoints stay aligned
