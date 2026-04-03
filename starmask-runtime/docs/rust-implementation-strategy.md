# Starmask Runtime Rust Implementation Strategy

## Status

This document translates the current `v1` extension-backed design into Rust implementation
constraints.

Repository status note: the in-tree `crates/starmask-runtime` adapter has been removed. Sections that
describe the adapter crate now serve as design guidance for any future external MCP transport.

It does not yet define the generic multi-backend rollout. That architecture work is tracked in
`docs/unified-wallet-coordinator-evolution.md` and should only graduate into this document when the
underlying contracts and phases are accepted.

## Purpose

This document defines how the first `starmask-runtime` implementation should be structured when Rust is the primary implementation language.

It does not replace the protocol, recovery, or security documents. It translates those documents into Rust-specific implementation constraints.

## Language Allocation

The recommended language split is:

- `starmaskd`: Rust
- `starmask-runtime`: Rust
- `starmask-native-host`: Rust
- `starmaskctl`: Rust
- `Starmask` browser extension and approval UI: TypeScript

This keeps all local binaries in one language while leaving the browser-facing wallet in the language that best fits Chrome extension APIs.

## MCP SDK Selection

The first Rust implementation should prefer the official Rust MCP SDK from `modelcontextprotocol/rust-sdk`.

Recommended crate choice:

- `rmcp`

Rationale:

1. it is the official Rust SDK listed by the MCP project
2. it already targets a Tokio async runtime
3. it reduces the amount of MCP transport and protocol boilerplate that `starmask-runtime` itself must own

Scope rule:

- use `rmcp` only for the MCP-facing `starmask-runtime` binary
- do not make `starmaskd`, `starmask-core`, or `starmask-native-host` depend on `rmcp`

This preserves a clean boundary:

- MCP protocol handling stays in the shim
- wallet lifecycle, policy, persistence, and native bridge logic stay in project-owned crates

Versioning rule:

1. prefer a released `rmcp` crate version when it supports the target MCP protocol revision and required server features
2. if a released crate is temporarily behind the required protocol revision, pin a reviewed git commit from the official repository
3. isolate the SDK behind a small adapter layer so future SDK upgrades are localized

## Rust Design Goals

The Rust implementation should optimize for:

1. strong type boundaries between protocol strings and internal state
2. explicit lifecycle transitions
3. deterministic local recovery
4. small, distributable binaries
5. minimal unsafe surface

## Workspace Shape

The first implementation should use one Cargo workspace with a small number of crates.

Recommended shape:

```text
starmask-runtime/
  Cargo.toml
  crates/
    starmask-types/
    starmask-core/
    starmaskd/
    starmask-runtime/
    starmask-native-host/
    starmaskctl/
```

Recommended responsibilities:

- `starmask-types`
  - shared Rust data types
  - protocol DTOs
  - ids, enums, error code mapping
- `starmask-core`
  - request lifecycle state machine
  - policy checks
  - repository traits
  - domain services
- `starmaskd`
  - daemon binary
  - IPC listeners
  - scheduler, timers, recovery bootstrap
- `starmask-runtime`
  - MCP stdio entrypoint
  - `rmcp` integration adapter
  - tool-to-daemon adapter
  - optional embeddable library entrypoints for host binaries that want to reuse the same MCP adapter in-process
- `starmask-native-host`
  - Chrome Native Messaging bridge binary
- `starmaskctl`
  - diagnostics and operator tooling

The first implementation should avoid splitting crates further unless a real boundary appears.

## Runtime Model

### `starmaskd`

`starmaskd` should use a Tokio runtime because it needs:

- local socket or pipe IO
- timers for TTL and lease expiry
- background garbage collection
- concurrent client connections

The daemon should still serialize lifecycle mutations through one coordinator task.

Recommended model:

1. transport tasks decode inbound JSON
2. transport tasks convert boundary DTOs into typed commands
3. typed commands go through an `mpsc` channel to one coordinator
4. the coordinator applies policy, state transitions, and persistence
5. results are returned via `oneshot` reply channels

This keeps concurrency at the transport layer while preserving a single owner for mutable lifecycle state.

### `starmask-runtime`

`starmask-runtime` is a thin local adapter.

Recommendation:

- keep it small
- do not embed lifecycle policy
- use a simple runtime model
- current-thread Tokio runtime is sufficient unless profiling shows otherwise
- let `rmcp` own MCP protocol and stdio transport details as much as practical
- expose a small library facade so another Rust binary can reuse the same adapter wiring without copying `main.rs`

Recommended library-facing surface:

- `DaemonClient`
- `LocalDaemonClient`
- a future `StarmaskMcpServer<C>`-style adapter type
- a future `serve_stdio(client)` helper
- `default_socket_path()`

### `starmask-native-host`

The native host is also a thin adapter.

Recommendation:

- one reader task
- one writer path
- one bridge client to `starmaskd`
- no local request state beyond the current Chrome connection

## Type System Strategy

Protocol strings should exist only at boundaries.

Inside Rust code, use typed wrappers and enums.

Recommended internal types:

- `RequestId`
- `ClientRequestId`
- `WalletInstanceId`
- `DeliveryLeaseId`
- `PresentationId`
- `RequestKind`
- `RequestStatus`
- `ResultKind`
- `LockState`
- `Channel`

Recommended rule:

- deserialize into boundary DTOs with `serde`
- convert DTOs into domain types with `TryFrom`
- perform lifecycle logic only on domain types

The first implementation should not pass raw stringly-typed status values through core services.

## Error Model

Use a layered error model.

Recommended approach:

1. typed domain and protocol errors in libraries
2. map errors to shared error codes at protocol boundaries
3. allow opaque application errors only at binary entrypoints and test harnesses

`thiserror` is a good fit for library error types because it derives standard `Error` implementations without forcing a public dependency in the external API surface.

## Serialization and Boundary Types

Recommended crates:

- `serde`
- `serde_json`

Use them for:

- daemon JSON-RPC DTOs
- native messaging payloads
- persisted payload envelopes where JSON storage is acceptable
- MCP tool input and output types

Rust recommendation:

- keep transport DTOs separate from domain structs
- derive `Serialize` and `Deserialize` on DTOs
- avoid deriving transport serialization on stateful coordinator internals unless needed

For the MCP-facing crate specifically:

- map `rmcp` tool inputs and outputs into project DTOs
- convert those DTOs into `starmask-core` domain types before applying policy

## Persistence Strategy

The first implementation should use SQLite with `rusqlite`.

Why:

- the daemon is local and low-throughput
- lifecycle correctness matters more than high write concurrency
- SQLite transactions and pragma configuration are easy to control directly

Recommended rules:

1. isolate SQLite behind a repository boundary
2. do not scatter SQL across transport handlers
3. use one transaction per lifecycle mutation
4. enable WAL mode
5. enable foreign key enforcement
6. configure a busy timeout

Concurrency rule:

- database writes should happen on a dedicated blocking path owned by the coordinator or a repository worker, not ad hoc inside arbitrary async tasks

## IPC Strategy

Recommended crates and primitives:

- Tokio Unix sockets on macOS and Linux
- Tokio Windows named pipes on Windows

The platform-specific transport should sit behind one Rust abstraction so the daemon protocol does not fork into separate code paths above the transport layer.

Recommended split:

- `ipc::listener`
- `ipc::client`
- `ipc::frame`
- `ipc::platform`

## Native Messaging Strategy

The native host must implement Chrome Native Messaging framing exactly:

- JSON messages
- UTF-8 bytes
- 32-bit native-endian length prefix

It must also respect Chrome process behavior:

- `connectNative()` keeps the host process alive for the port lifetime
- the first CLI argument identifies the caller origin
- stdout is the protocol channel
- stderr is for diagnostics only

Rust implementation rule:

- keep framing in one dedicated module
- do not mix log output with stdout writes
- if Windows needs a binary-mode stdio shim, isolate it in one tiny platform module

## Configuration Strategy

Recommended crates:

- `clap` for CLI parsing
- `serde` for config deserialization
- a TOML parser for config files

Recommended flow:

1. parse CLI args
2. load config file
3. merge environment overrides
4. normalize into one validated runtime config struct
5. only then start listeners or recovery

The normalized runtime config should be the only config object visible to most of the program.

## Observability Strategy

Recommended crates:

- `tracing`
- `tracing-subscriber`

Use structured fields such as:

- `request_id`
- `client_request_id`
- `wallet_instance_id`
- `status`
- `error_code`

Logging rules:

1. do not log private keys
2. do not log full signed payloads at normal log levels
3. prefer stable field names over ad hoc message strings

## Unsafe Code Policy

The Rust workspace should default to:

- `#![forbid(unsafe_code)]`

If one small platform shim requires unsafe code:

- isolate it in the narrowest possible module
- document the safety invariant
- keep it out of the core lifecycle and persistence crates

## Test Strategy

The Rust implementation should use multiple layers of tests.

### Core tests

- unit tests for lifecycle transitions
- policy tests for routing and approval requirements
- recovery tests for replayed events and restart semantics

### Integration tests

- daemon JSON-RPC round trips
- native host framing
- temp-directory SQLite recovery
- socket and pipe transport tests where supported

### Boundary tests

- schema-to-Rust serialization compatibility
- MCP tool result mapping
- error code mapping
- `rmcp` integration tests for stdio server behavior

The lifecycle core should be testable without launching Chrome or a real MCP host.

## First Implementation Sequence

Recommended order:

1. `starmask-types`
2. `starmask-core`
3. SQLite repository and recovery bootstrap
4. daemon IPC layer
5. native host framing and bridge
6. MCP shim with `rmcp`
7. diagnostics command

This order keeps correctness-critical code ahead of adapters.

## Non-Goals

This document does not lock the project to a single MCP Rust library or framework.

It defines the internal Rust architecture that should remain stable even if the chosen MCP SDK changes later.
