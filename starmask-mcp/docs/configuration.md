# Starmask MCP Configuration

## Status

This document is the authoritative configuration contract for the current runtime.

Repository status note: the in-tree `crates/starmask-mcp` adapter has been removed. Configuration
references to `starmask-mcp` should be read as historical or external-adapter guidance.

The current Rust code in `crates/starmaskd` supports both:

- legacy extension-backed `v1` top-level settings for compatibility
- phase-2 `wallet_backends` entries for generic backends

Detailed multi-backend entry rules remain defined separately in:

- `docs/unified-wallet-coordinator-evolution.md`
- `docs/wallet-backend-configuration.md`

## 1. Purpose

This document defines the configuration surface for:

- `starmaskd`
- `starmask-mcp`
- `starmask-native-host`

## 2. Configuration Principles

1. secure defaults first
2. local-only by default
3. production and development channels remain separable
4. timing-sensitive behavior is configurable but bounded

## 3. Configuration Sources

Precedence:

1. CLI flags
2. environment variables
3. config file
4. built-in defaults

Validation should happen before:

- opening the daemon listener
- opening the SQLite database
- starting recovery

## 4. Current Runtime Config Shape

The current daemon runtime config contains:

- `channel`
- `allowed_extension_ids`
- `native_host_name`
- `socket_path`
- `database_path`
- `log_level`
- `maintenance_interval_seconds`
- `default_request_ttl_seconds`
- `min_request_ttl_seconds`
- `max_request_ttl_seconds`
- `delivery_lease_seconds`
- `presentation_lease_seconds`
- `heartbeat_interval_seconds`
- `wallet_offline_after_seconds`
- `result_retention_seconds`
- `wallet_backends`

If `wallet_backends` is absent, legacy top-level extension fields are translated into one implicit
extension backend.

## 5. Default Paths

### 5.1 macOS

- daemon socket:
  - `$HOME/Library/Application Support/StarcoinMCP/run/starmaskd.sock`
- database:
  - `$HOME/Library/Application Support/StarcoinMCP/starmaskd.sqlite3`
- logs:
  - `$HOME/Library/Logs/StarcoinMCP/starmaskd.log`
- config file:
  - `$HOME/Library/Application Support/StarcoinMCP/config.toml`

### 5.2 Linux

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

### 5.3 Windows

- daemon pipe:
  - `\\\\.\\pipe\\starcoin-mcp-starmaskd`
- database:
  - `%LOCALAPPDATA%\\StarcoinMCP\\starmaskd.sqlite3`
- logs:
  - `%LOCALAPPDATA%\\StarcoinMCP\\logs\\starmaskd.log`
- config file:
  - `%APPDATA%\\StarcoinMCP\\config.toml`

## 6. Current Config File Example

```toml
channel = "development"
allowed_extension_ids = ["kmheclfnfmpacglnpegeohempmedhiaf"]
native_host_name = "com.starcoin.starmask.development"
socket_path = "/Users/alice/Library/Application Support/StarcoinMCP/run/starmaskd.sock"
database_path = "/Users/alice/Library/Application Support/StarcoinMCP/starmaskd.sqlite3"
log_level = "info"
maintenance_interval_seconds = 1
default_request_ttl_seconds = 300
min_request_ttl_seconds = 30
max_request_ttl_seconds = 3600
delivery_lease_seconds = 30
presentation_lease_seconds = 45
heartbeat_interval_seconds = 10
wallet_offline_after_seconds = 25
result_retention_seconds = 600
```

## 7. Required Settings

### 7.1 Channel and extension trust

- `channel`
  - one of `development`, `staging`, `production`
- `allowed_extension_ids`
- `native_host_name`

### 7.2 Transport and storage

- `socket_path` or `pipe_name`
- `database_path`

### 7.3 Operational settings

- `log_level`
- timing settings listed above

## 8. Timing Defaults

Current defaults:

- `default_request_ttl_seconds = 300`
- `min_request_ttl_seconds = 30`
- `max_request_ttl_seconds = 3600`
- `delivery_lease_seconds = 30`
- `presentation_lease_seconds = 45`
- `heartbeat_interval_seconds = 10`
- `wallet_offline_after_seconds = 25`
- `result_retention_seconds = 600`

The current code does not expose terminal-record retention as a user-configurable setting.

## 9. Policy Defaults

The current implementation closes the remaining policy questions in favor of a narrow and
deterministic design:

- explicit wallet selection is required when routing is ambiguous
- auto-route is allowed only when exactly one wallet instance matches
- account listing does not require an interactive approval step
- public-key lookup may use cached metadata
- requests fail fast when the target wallet is unavailable
- requests fail fast when the target wallet is locked unless the selected backend advertises
  backend-local unlock support
- blind signing is not supported

## 10. Environment Variable Mapping

Suggested environment variable names currently supported by `starmaskd`:

- `STARMASKD_SOCKET_PATH`
- `STARMASKD_DB_PATH`
- `STARMASKD_LOG_LEVEL`
- `STARMASKD_CHANNEL`
- `STARMASKD_ALLOWED_EXTENSION_IDS`
- `STARMASKD_NATIVE_HOST_NAME`
- `STARMASKD_MAINTENANCE_INTERVAL_SECONDS`
- `STARMASKD_DEFAULT_REQUEST_TTL_SECONDS`
- `STARMASKD_MIN_REQUEST_TTL_SECONDS`
- `STARMASKD_MAX_REQUEST_TTL_SECONDS`
- `STARMASKD_DELIVERY_LEASE_SECONDS`
- `STARMASKD_PRESENTATION_LEASE_SECONDS`
- `STARMASKD_HEARTBEAT_INTERVAL_SECONDS`
- `STARMASKD_WALLET_OFFLINE_AFTER_SECONDS`
- `STARMASKD_RESULT_RETENTION_SECONDS`

`starmask-mcp` should support:

- daemon socket or pipe override
- RPC timeout override
- log level override

`starmask-native-host` should support:

- daemon socket or pipe override
- expected channel name
- log level override

## 11. Safe Bounds

The implementation should clamp unsafe timing values:

1. `request_ttl_seconds` below minimum is raised to minimum
2. `request_ttl_seconds` above maximum is lowered to maximum
3. `result_retention_seconds` may not be zero
4. `wallet_offline_after_seconds` must be greater than `heartbeat_interval_seconds`

## 12. Configuration Errors

Misconfiguration should surface with actionable errors.

Typical cases:

- invalid extension ID allowlist
- empty extension ID allowlist
- missing Native Messaging manifest
- unsupported channel value
- unwritable database path
- socket path without a parent directory
- insecure or unusable runtime directories

## 13. Non-Goals

This document does not define package-manager-specific install commands. Per-backend phase-2 entry
rules are defined in `docs/wallet-backend-configuration.md`.
