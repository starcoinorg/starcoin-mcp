# Starcoin MCP Deployment Model

## Purpose

This document defines the deployment and runtime model for the local wallet-facing Starcoin MCP stack.

The scope of this document is:

- `starmask-runtime`
- `starmaskd`
- `starmask-native-host`
- `Starmask` Chrome extension
- the local MCP host, such as Claude Code or Codex

## Design Goal

The deployment model must preserve the signing trust boundary:

- the MCP host can request signing
- the daemon can broker signing
- only the extension can approve and sign

## Runtime Topology

```mermaid
flowchart LR
    H["MCP Host"] --> M["starmask-runtime (stdio)"]
    M --> D["starmaskd (local daemon)"]
    D --> N["starmask-native-host"]
    N --> E["Starmask Chrome Extension"]
```

## Deployment Profiles

### Local Single-User Profile

This is the only supported profile for the first implementation.

Properties:

- all processes run under one OS user
- the daemon listens only on a user-scoped local transport
- the wallet extension runs in a browser profile controlled by the same OS user
- the MCP host is local to the same machine

### Unsupported Profiles

The following profiles are out of scope:

- shared multi-user daemon
- remote daemon access
- remote browser or remote extension access
- network-exposed signing broker

## Installed Artifacts

The local installation consists of:

1. `starmask-runtime`
2. `starmaskd`
3. `starmask-native-host`
4. `Starmask` Chrome extension
5. optional `starmaskctl`

## Process Responsibilities

### MCP Host

- launches `starmask-runtime`
- invokes MCP tools
- persists `request_id` when necessary
- never talks directly to the extension

### `starmask-runtime`

- speaks MCP over stdio
- validates request shape
- forwards RPC to the daemon
- has no long-lived signing state

### `starmaskd`

- persists request and wallet state
- owns the canonical request lifecycle
- enforces TTL and local policy
- survives MCP host restarts

### `starmask-native-host`

- is launched by Chrome through Native Messaging
- forwards extension messages to the daemon
- should be stateless beyond connection-scoped transport state

### `Starmask` Extension

- holds wallet state and signing authority
- renders approval UI
- performs transaction decoding and message rendering
- returns signed results or rejection

## Startup Model

The normal startup sequence is:

1. the user installs the local binaries and the browser extension
2. the user-level daemon is started
3. the native messaging manifest is registered
4. the browser extension connects to the native host
5. the native host connects to the daemon
6. the extension registers its `wallet_instance_id`
7. the MCP host starts `starmask-runtime` when needed
8. `starmask-runtime` connects to the daemon on demand

The design assumes that wallet connectivity may come before or after the MCP host starts.

## Steady-State Model

In steady state:

- `starmaskd` stays alive across many MCP tool calls
- `starmask-runtime` may be short-lived
- the extension may disconnect and reconnect without losing canonical request state
- the native host may be restarted by Chrome without changing logical wallet identity

## Canonical Runtime Paths

### Path 1: Wallet-First

1. browser is already open
2. extension is already connected
3. daemon already knows the wallet instance
4. MCP host later starts `starmask-runtime`
5. requests can be delivered immediately

### Path 2: Host-First

1. MCP host starts `starmask-runtime`
2. daemon is reachable but no extension is connected yet
3. host may still query wallet status
4. in the first release, signing request creation fails fast until a connected unlocked wallet instance is available
5. once the extension connects, the host may retry request creation with the same `client_request_id`

### Path 3: Browser Restart

1. daemon remains alive
2. browser and extension disconnect
3. non-terminal requests remain persisted in the daemon
4. when the extension reconnects, the daemon re-evaluates which requests are re-deliverable

## Shutdown Model

### MCP Host Exit

- `starmask-runtime` may exit immediately
- the daemon keeps all non-terminal requests
- the host can later resume by polling the same `request_id`

### Browser Exit

- the extension disconnects
- native host exits
- the daemon marks the wallet instance disconnected
- non-terminal requests remain subject to recovery policy

### Daemon Exit

- all active transports are dropped
- persisted requests must survive
- after restart, the daemon reloads non-terminal state before accepting new claims

## Recovery Rules

The deployment model requires the following recovery behavior:

1. daemon restart does not lose persisted requests
2. extension reconnect does not create a new logical wallet identity unless local storage was reset
3. host restart does not require re-creating an existing request
4. transport loss alone does not imply rejection

## Local Transport Requirements

The first implementation should use:

- Unix domain socket on macOS and Linux
- named pipe on Windows

The transport must:

- be scoped to the current OS user
- reject non-local access
- support request-response RPC and event delivery

## Identity and Routing

The deployment model distinguishes:

- process identity
- wallet identity
- account identity

They must not be collapsed into one concept.

Rules:

1. `wallet_instance_id` identifies one extension-local wallet instance
2. browser reconnect must preserve `wallet_instance_id` where possible
3. account addresses may appear in more than one wallet instance
4. request routing is performed by `starmaskd`, not by the MCP host transport layer

## Installation-Time Constraints

The deployment model depends on:

1. a valid Native Messaging host manifest
2. an exact allowlist of extension IDs per release channel
3. compatible protocol versions across:
   - `starmask-runtime`
   - `starmaskd`
   - `starmask-native-host`
   - `Starmask` extension

## Observability Requirements

The deployment model requires:

- daemon logs
- native host bridge logs
- extension connection status
- diagnostic command output from `starmaskctl doctor`

The first implementation should prefer simple local diagnostics over remote telemetry.

## Implementation Gates

The deployment model is considered ready only when the following follow-up documents exist:

1. `starmask-runtime/docs/daemon-protocol.md`
2. `starmask-runtime/docs/native-messaging-contract.md`
3. `starmask-runtime/docs/persistence-and-recovery.md`
4. `starmask-runtime/docs/configuration.md`

This document defines where components live and how they relate. It does not define exact wire messages or database layout.
