# Starmask MCP Configuration

## Purpose

This document defines the configuration surface for:

- `starmaskd`
- `starmask-mcp`
- `starmask-native-host`

## Configuration Principles

1. secure defaults first
2. local-only by default
3. production and development channels must be separable
4. timing-sensitive behavior must be configurable but bounded

## Configuration Sources

Precedence:

1. CLI flags
2. environment variables
3. config file
4. built-in defaults

## Default Paths

### macOS

- daemon socket:
  - `$HOME/Library/Application Support/StarcoinMCP/run/starmaskd.sock`
- database:
  - `$HOME/Library/Application Support/StarcoinMCP/starmaskd.sqlite3`
- logs:
  - `$HOME/Library/Logs/StarcoinMCP/starmaskd.log`
- config file:
  - `$HOME/Library/Application Support/StarcoinMCP/config.toml`

### Linux

- daemon socket:
  - `$XDG_RUNTIME_DIR/starcoin-mcp/starmaskd.sock`
  - fallback: `$HOME/.local/state/starcoin-mcp/starmaskd.sock`
- database:
  - `$XDG_STATE_HOME/starcoin-mcp/starmaskd.sqlite3`
  - fallback: `$HOME/.local/state/starcoin-mcp/starmaskd.sqlite3`
- logs:
  - `$XDG_STATE_HOME/starcoin-mcp/starmaskd.log`
  - fallback: `$HOME/.local/state/starcoin-mcp/starmaskd.log`
- config file:
  - `$XDG_CONFIG_HOME/starcoin-mcp/config.toml`
  - fallback: `$HOME/.config/starcoin-mcp/config.toml`

### Windows

- daemon pipe:
  - `\\\\.\\pipe\\starcoin-mcp-starmaskd`
- database:
  - `%LOCALAPPDATA%\\StarcoinMCP\\starmaskd.sqlite3`
- logs:
  - `%LOCALAPPDATA%\\StarcoinMCP\\logs\\starmaskd.log`
- config file:
  - `%APPDATA%\\StarcoinMCP\\config.toml`

## Required Settings

### Channel Settings

- `channel`
  - one of:
    - `development`
    - `staging`
    - `production`
- `allowed_extension_ids`
- `native_host_name`

### Transport Settings

- `socket_path` or `pipe_name`
- `socket_permissions`

### Storage Settings

- `database_path`
- `log_path`

## Timing Defaults

- `default_request_ttl_seconds = 300`
- `min_request_ttl_seconds = 30`
- `max_request_ttl_seconds = 3600`
- `delivery_lease_seconds = 30`
- `presentation_lease_seconds = 45`
- `heartbeat_interval_seconds = 10`
- `wallet_offline_after_seconds = 25`
- `result_retention_seconds = 600`
- `terminal_record_retention_seconds = 86400`

## Policy Defaults

The first implementation should use these defaults:

- `require_explicit_wallet_selection_when_ambiguous = true`
- `allow_auto_route_when_exactly_one_match = true`
- `allow_account_listing_without_approval = true`
- `allow_public_key_lookup_without_approval = true`
- `fail_fast_when_wallet_unavailable = true`
- `fail_fast_when_wallet_locked = true`
- `allow_blind_signing = false`
- `allow_message_sign_policy_exceptions = false`

These settings close the remaining first-release policy questions in favor of a narrow but deterministic implementation.

## Result Handling Settings

- `result_payload_multi_read = true`
- `result_payload_retention_seconds = 600`

The first release does not support:

- single-read signed results
- indefinite signed result retention

## Environment Variable Mapping

Suggested environment variable names:

- `STARMASKD_SOCKET_PATH`
- `STARMASKD_DB_PATH`
- `STARMASKD_LOG_PATH`
- `STARMASKD_CHANNEL`
- `STARMASKD_ALLOWED_EXTENSION_IDS`
- `STARMASKD_DEFAULT_REQUEST_TTL_SECONDS`
- `STARMASKD_RESULT_RETENTION_SECONDS`
- `STARMASKD_LOG_LEVEL`

## MCP Shim Settings

`starmask-mcp` should support:

- daemon socket or pipe override
- RPC timeout override
- log level override

The MCP shim should not carry policy that diverges from daemon policy.

## Native Host Settings

`starmask-native-host` should support:

- daemon socket or pipe override
- expected channel name
- log level override

It should not store wallet state or request state.

## Safe Bounds

The implementation should clamp unsafe timing values:

1. `request_ttl_seconds` below minimum is raised to minimum
2. `request_ttl_seconds` above maximum is lowered to maximum
3. `result_retention_seconds` may not be zero in the first release

## Configuration Errors

Misconfiguration should surface with actionable errors.

Typical cases:

- invalid extension ID allowlist
- missing native host manifest
- unsupported channel value
- unwritable database path
- insecure socket permissions

## Non-Goals

This document does not define package-manager-specific install commands.
