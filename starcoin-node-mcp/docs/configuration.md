# Starcoin Node MCP Configuration

## Purpose

This document defines the configuration surface for `starcoin-node-mcp`.

## Configuration Principles

1. explicit endpoint selection
2. fail closed on chain mismatch in transaction mode
3. least capability by default
4. bounded timeouts and watch intervals
5. secrets come from configuration, not tool input

## Configuration Sources

Precedence:

1. CLI flags
2. environment variables
3. config file
4. built-in defaults

## Rust Configuration Binding

Recommended Rust approach:

- `clap` for CLI parsing
- `serde` for config deserialization
- one normalized runtime config struct after merge and validation

Validation must happen before:

- opening the MCP server
- probing the RPC endpoint
- enabling transaction tools

## Default Paths

### macOS

- config file:
  - `$HOME/Library/Application Support/StarcoinMCP/node-mcp.toml`
- log file:
  - `$HOME/Library/Logs/StarcoinMCP/starcoin-node-mcp.log`
- cache directory:
  - `$HOME/Library/Caches/StarcoinMCP/node-mcp/`

### Linux

- config file:
  - `$XDG_CONFIG_HOME/starcoin-mcp/node-mcp.toml`
  - fallback: `$HOME/.config/starcoin-mcp/node-mcp.toml`
- log file:
  - `$XDG_STATE_HOME/starcoin-mcp/starcoin-node-mcp.log`
  - fallback: `$HOME/.local/state/starcoin-mcp/starcoin-node-mcp.log`
- cache directory:
  - `$XDG_CACHE_HOME/starcoin-mcp/node-mcp/`
  - fallback: `$HOME/.cache/starcoin-mcp/node-mcp/`

### Windows

- config file:
  - `%APPDATA%\\StarcoinMCP\\node-mcp.toml`
- log file:
  - `%LOCALAPPDATA%\\StarcoinMCP\\logs\\starcoin-node-mcp.log`
- cache directory:
  - `%LOCALAPPDATA%\\StarcoinMCP\\cache\\node-mcp\\`

## Required Settings

### Endpoint Settings

- `rpc_endpoint_url`
- `mode`
  - `read_only`
  - `transaction`
- `vm_profile`
  - `auto`
  - `vm2_only`
  - `legacy_compatible`

### Chain Pin Settings

For `transaction` mode, the following settings are required:

- `expected_chain_id`
- `expected_network`

For `read_only` mode, they are recommended and may be omitted only when the caller explicitly accepts endpoint autodetection.

## Optional Endpoint Settings

- `connect_timeout_ms`
- `request_timeout_ms`
- `startup_probe_timeout_ms`
- `rpc_auth_token_env`
- `rpc_headers`
- `tls_server_name`
- `allow_insecure_remote_transport`

## Transaction Safety Settings

- `default_expiration_ttl_seconds`
- `max_expiration_ttl_seconds`
- `watch_poll_interval_seconds`
- `watch_timeout_seconds`
- `max_head_lag_seconds`
- `warn_head_lag_seconds`
- `allow_submit_without_prior_simulation`

Recommended defaults:

- `default_expiration_ttl_seconds = 600`
- `max_expiration_ttl_seconds = 3600`
- `watch_poll_interval_seconds = 3`
- `watch_timeout_seconds = 120`
- `warn_head_lag_seconds = 15`
- `max_head_lag_seconds = 60`
- `allow_submit_without_prior_simulation = true`

The first release allows submission without prior simulation because a signed transaction may arrive from an external wallet flow, but the result should make it clear whether simulation had been performed earlier.

## Caching Settings

- `chain_status_cache_ttl_seconds`
- `abi_cache_ttl_seconds`
- `module_cache_max_entries`
- `disable_disk_cache`

Recommended defaults:

- `chain_status_cache_ttl_seconds = 3`
- `abi_cache_ttl_seconds = 300`
- `module_cache_max_entries = 1024`
- `disable_disk_cache = true`

The first release should prefer in-memory caches only.

## Policy Defaults

The first implementation should use these defaults:

- `mode = read_only`
- `vm_profile = auto`
- `allow_insecure_remote_transport = false`
- `allow_submit_without_prior_simulation = true`
- `disable_disk_cache = true`

These defaults keep the server conservative while still supporting the canonical wallet-orchestrated flow.

## Environment Variable Mapping

Suggested environment variable names:

- `STARCOIN_NODE_MCP_RPC_ENDPOINT_URL`
- `STARCOIN_NODE_MCP_MODE`
- `STARCOIN_NODE_MCP_VM_PROFILE`
- `STARCOIN_NODE_MCP_EXPECTED_CHAIN_ID`
- `STARCOIN_NODE_MCP_EXPECTED_NETWORK`
- `STARCOIN_NODE_MCP_RPC_AUTH_TOKEN`
- `STARCOIN_NODE_MCP_REQUEST_TIMEOUT_MS`
- `STARCOIN_NODE_MCP_ALLOW_INSECURE_REMOTE_TRANSPORT`
- `STARCOIN_NODE_MCP_LOG_LEVEL`

## Safe Bounds

The implementation should clamp unsafe timing values:

1. `watch_poll_interval_seconds` below `1` is raised to `1`
2. `default_expiration_ttl_seconds` below `30` is raised to `30`
3. `default_expiration_ttl_seconds` above `max_expiration_ttl_seconds` is lowered to the configured maximum
4. `warn_head_lag_seconds` above `max_head_lag_seconds` is lowered to `max_head_lag_seconds`

## Configuration Errors

Misconfiguration should surface with actionable errors.

Typical cases:

- missing `expected_chain_id` in transaction mode
- missing `expected_network` in transaction mode
- invalid or unsupported `vm_profile`
- insecure remote endpoint without explicit override
- malformed RPC header configuration
- negative or zero timeouts after normalization

## Non-Goals

This document does not define:

- package-manager-specific install commands
- wallet configuration
- node binary configuration
