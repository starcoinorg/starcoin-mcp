# Runtime Supervision TUI

## Purpose

This document defines the first coding target for an operator-facing TUI that starts and monitors
the local Starcoin wallet runtime and, optionally, one node-side service.

Status note:

- the current repository already contains the startable binaries the TUI must supervise
- the repository does not currently ship in-tree stdio adapters for `starcoin-node` or
  `starmask-runtime`
- the TUI is therefore an operator tool, not a replacement for the logical host-adapter boundary

## Design Goal

The TUI should make local runtime startup predictable without collapsing the existing trust
boundaries.

Non-negotiable rules:

1. the TUI never signs
2. the TUI never submits transactions in place of `starcoin-node`
3. the TUI never owns browser approval UI
4. the TUI never turns `starcoin-node-cli` into a daemon
5. the TUI keeps wallet-side and node-side process ownership explicit

## Current Startable Units

The current repository exposes these relevant process types:

- `starmaskd`
  - long-lived wallet coordinator
- `local-account-agent`
  - one process per enabled `local_account_dir` backend
- `starmask-native-host`
  - launched by Chrome on demand
- `starcoin-node-cli`
  - one-shot chain command runner
- optional local node-side service
  - external RPC-producing process chosen by the operator

Important implication:

- wallet-side supervision is concrete and in-repo
- node-side supervision is optional and should target the underlying RPC service, not
  `starcoin-node-cli`

## Proposed Implementation Shape

The first implementation should be a new top-level Rust subproject, for example:

```text
starcoin-runtime-tui/
  Cargo.toml
  src/
    main.rs
    app.rs
    config.rs
    process_model.rs
    wallet.rs
    node.rs
    diagnostics.rs
    ui/
```

Packaging choice:

- use a separate binary
- supervise child processes with `std::process::Command`
- keep the first pass out-of-process instead of embedding `starmaskd` or `starcoin-node-cli`

Suggested Rust stack:

- `ratatui`
- `crossterm`
- `tokio`
- `serde` / `toml` for TUI-specific config and state

## Runtime Profiles

The TUI should manage explicit runtime profiles.

Minimum profile fields:

- `wallet_config_path`
  - path to the `starmaskd` config file to use
- `node_config_path`
  - path to the `node-cli.toml` file to validate or display
- `manage_node_service`
  - whether the TUI should also start a node-side service
- `node_service`
  - optional command, working directory, and environment for the managed node-side service
- `runtime_state_dir`
  - directory for pid metadata, logs, and TUI-local state

Recommended profile kinds:

1. `system`
   - uses normal OS-default config locations
2. `workspace_dev`
   - uses repository-local `.runtime/` paths similar to the current workflow scripts

The first TUI pass should support selecting one profile at startup rather than hot-switching
profiles while processes are running.

## Supervisor Security Requirements

The TUI does not become a signer, but it can still weaken the deployment if it launches or adopts
processes unsafely.

### Child-process launch hygiene

Required rules:

1. resolve every managed executable to an absolute path before launch
2. avoid ambient `PATH` search in product packaging
3. use an explicit working directory for managed node-side services
4. pass secrets through protected config or narrowly scoped environment injection rather than argv
5. minimize inherited environment variables before spawning child processes

### Runtime state and artifact hardening

`runtime_state_dir` must be treated as sensitive operator state.

Required rules:

1. the state directory must be current-user only
2. pid files, launch metadata, logs, and copied diagnostics must not be written into shared
   writable directories
3. TUI-local logs must follow the same redaction rules as the managed processes
4. channel-specific profiles must not share the same pid metadata or socket override files

### Process adoption and stale-artifact safety

Required rules:

1. process adoption must verify more than the pid alone
2. adoption should confirm executable path, current OS user, and process start identity before the
   TUI treats a process as owned
3. stale socket cleanup must happen only after a failed connect and only inside an owned private
   runtime directory
4. the TUI must not delete arbitrary filesystem paths just because a pid file or launch record
   points there

### Managed node-side service hardening

If `manage_node_service = true`, the TUI must additionally enforce:

1. local bind by default
2. a visible warning before starting a node-side service that listens beyond loopback
3. no attempt to supervise admin or debug RPC surfaces as if they were the same endpoint used by
   `starcoin-node-cli`
4. readiness checks against the exact endpoint URL that later chain commands use

## Wallet Supervision Model

### Process ownership

The TUI should:

1. start `starmaskd`
2. inspect the selected wallet config
3. launch one `local-account-agent` for each enabled `local_account_dir` backend
4. never directly launch `starmask-native-host`

Extension backend handling:

- treat extension backends as configured-but-external
- show manifest and connection status
- wait for Chrome to launch `starmask-native-host`

### Wallet readiness checks

`starmaskd` is ready only when:

1. its socket exists
2. the socket accepts a connection
3. daemon health calls succeed

A `local_account_dir` backend is ready only when:

1. its agent process is running
2. the daemon reports the expected `wallet_instance_id`

Recommended health sources:

- daemon socket reachability
- `system.ping` or `system.getInfo`
- `wallet_list_instances`
- `wallet_list_accounts`
- `starmaskctl doctor`

## Node Supervision Model

### Core rule

The TUI must not supervise `starcoin-node-cli` as a daemon.

Instead, the optional node section may supervise one external process that produces the RPC
endpoint consumed later by `starcoin-node-cli`.

Examples:

- a local Starcoin node process
- a docker-compose service wrapper
- a repository-specific devnet launcher

### Node readiness checks

A managed node-side service is ready only when:

1. the configured RPC endpoint answers health probes
2. the endpoint URL matches the `rpc_endpoint_url` later used by `starcoin-node-cli`
3. `starcoin-node-cli` config validation would pass for the selected profile

Recommended health probes:

- direct JSON-RPC `node.info`
- direct JSON-RPC `chain.info`
- optional `starcoin-node-cli call chain_status`

## Startup Order

Recommended first-pass startup sequence:

1. load the selected profile
2. validate the selected `starmaskd` config path exists and parses
3. start `starmaskd`
4. wait for daemon readiness
5. launch each enabled `local_account_dir` backend agent
6. wait for local backend registration
7. check extension-backend manifest and connection state
8. if `manage_node_service = true`, start the configured node-side service and wait for RPC
   readiness
9. show the combined runtime as `ready`, `degraded`, or `failed`

The node branch is intentionally last because wallet-side startup is the mandatory part of signing
flows.

## Shutdown and Recovery

Recommended stop order:

1. stop local-account agents
2. stop `starmaskd`
3. stop the managed node-side service only if the TUI started it

Recovery requirements:

1. the TUI should persist enough pid and launch metadata to rediscover a previous session
2. a TUI restart should offer either:
   - adopt running processes
   - stop them cleanly
   - start a fresh session
3. process adoption must use explicit metadata rather than guessing by process name alone

The current script precedent for this behavior is:

- `plugins/starcoin-transfer-workflow/scripts/wallet_runtime.py`

## UI Surface

The first TUI pass should stay narrow.

Minimum panes:

1. `Wallet`
   - daemon state
   - backend states
   - socket path
   - current wallet instances
2. `Node`
   - managed/unmanaged mode
   - RPC endpoint URL
   - health result
3. `Logs`
   - per-process log tail
4. `Diagnostics`
   - `starmaskctl doctor` output
   - node health probe summary

Minimum actions:

- `Start`
- `Stop`
- `Restart`
- `Run Diagnostics`
- `Open Logs`

## Lower-Level Integration Contracts

The TUI first pass depends on these lower-level contracts staying stable:

- `starmaskd` config loading and socket layout in `starmask-runtime`
- one-agent-per-backend startup contract in `starmask-runtime`
- `starcoin-node-cli` remaining a one-shot command surface in `starcoin-node`
- node-side health being derivable from the configured RPC endpoint

## Coding-Ready Checklist

This TUI design is ready for implementation because the following questions now have explicit
answers:

1. Which wallet processes are actually startable by the TUI?
   - `starmaskd` and enabled `local_account_dir` agents
2. Which wallet process is not TUI-owned?
   - `starmask-native-host`, because Chrome owns it
3. What is optional on the node side?
   - the underlying RPC-producing service
4. What is not a background service?
   - `starcoin-node-cli`
5. What is the first safe packaging choice?
   - a separate Rust TUI that supervises child processes
6. What security constraints must the implementation preserve?
   - private runtime directories, safe process adoption, no secrets on argv, and local-only wallet
     IPC defaults
