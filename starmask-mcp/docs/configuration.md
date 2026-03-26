# Starmask MCP Unified Configuration

## 1. Purpose

This document defines the configuration surface for:

- `starmaskd`
- `starmask-mcp`
- `starmask-native-host`
- local wallet backend agents

The configuration model is intentionally backend-generic. The old extension-only configuration is
no longer sufficient once `local_account_dir` and other signer backends participate in the same
coordinator.

## 2. Configuration Principles

1. secure defaults first
2. local-only by default
3. production and development channels remain separable
4. backend definitions live in one normalized runtime config
5. timing-sensitive behavior is configurable but bounded

## 3. Configuration Sources

Precedence:

1. CLI flags
2. environment variables
3. config file
4. built-in defaults

Validation must complete before:

- opening the daemon listener
- opening the database
- starting recovery
- enabling any signer backend

## 4. Default Paths

### 4.1 macOS

- daemon socket:
  - `$HOME/Library/Application Support/StarcoinMCP/run/starmaskd.sock`
- database:
  - `$HOME/Library/Application Support/StarcoinMCP/starmaskd.sqlite3`
- logs:
  - `$HOME/Library/Logs/StarcoinMCP/starmaskd.log`
- config file:
  - `$HOME/Library/Application Support/StarcoinMCP/config.toml`

### 4.2 Linux

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

### 4.3 Windows

- daemon pipe:
  - `\\\\.\\pipe\\starcoin-mcp-starmaskd`
- database:
  - `%LOCALAPPDATA%\\StarcoinMCP\\starmaskd.sqlite3`
- logs:
  - `%LOCALAPPDATA%\\StarcoinMCP\\logs\\starmaskd.log`
- config file:
  - `%APPDATA%\\StarcoinMCP\\config.toml`

## 5. Normalized Runtime Config Shape

The runtime config should normalize into:

- one global daemon config
- one policy and timing config
- zero or more `wallet_backends`
- one thin MCP adapter config

Recommended top-level TOML shape:

```toml
channel = "development"
socket_path = "/Users/alice/Library/Application Support/StarcoinMCP/run/starmaskd.sock"
database_path = "/Users/alice/Library/Application Support/StarcoinMCP/starmaskd.sqlite3"
log_path = "/Users/alice/Library/Logs/StarcoinMCP/starmaskd.log"
log_level = "info"

[timing]
default_request_ttl_seconds = 300
min_request_ttl_seconds = 30
max_request_ttl_seconds = 3600
delivery_lease_seconds = 30
presentation_lease_seconds = 45
result_retention_seconds = 600
terminal_record_retention_seconds = 86400

[policy]
require_explicit_wallet_selection_when_ambiguous = true
allow_auto_route_when_exactly_one_match = true
allow_account_listing_without_approval = true
allow_public_key_lookup_without_approval = true
fail_fast_when_wallet_unavailable = true
fail_fast_when_wallet_locked = true
allow_blind_signing = false
allow_dev_backends = false

[[wallet_backends]]
id = "browser-default"
backend_kind = "starmask_extension"
enabled = true
label = "StarMask Browser"
transport_kind = "native_messaging"
allowed_extension_ids = ["kmheclfnfmpacglnpegeohempmedhiaf"]
native_host_name = "com.starcoin.starmask.development"
heartbeat_interval_seconds = 10
wallet_offline_after_seconds = 25

[[wallet_backends]]
id = "local-main"
backend_kind = "local_account_dir"
enabled = true
label = "Local Account Vault"
transport_kind = "local_socket"
agent_socket_path = "/Users/alice/Library/Application Support/StarcoinMCP/run/local-main.sock"
account_provider = "local"
local_account_dir = "/Users/alice/.starcoin/account-vault"
prompt_mode = "tty"
default_unlock_ttl_seconds = 300
max_unlock_ttl_seconds = 1800
allow_sign_transaction = true
allow_sign_message = true
allow_get_public_key = true
```

## 6. Global Settings

Required global settings:

- `channel`
  - one of `development`, `staging`, `production`
- `socket_path` or `pipe_name`
- `database_path`
- `log_path`

Optional but recommended global settings:

- `log_level`
- `migration_mode`
- `maintenance_interval_seconds`

## 7. Timing Settings

Recommended defaults:

- `default_request_ttl_seconds = 300`
- `min_request_ttl_seconds = 30`
- `max_request_ttl_seconds = 3600`
- `delivery_lease_seconds = 30`
- `presentation_lease_seconds = 45`
- `result_retention_seconds = 600`
- `terminal_record_retention_seconds = 86400`

Optional unlock-related timing:

- `default_unlock_ttl_seconds = 300`
- `max_unlock_ttl_seconds = 1800`

The implementation must clamp unsafe values rather than trusting config blindly.

## 8. Policy Settings

Recommended first-release policy defaults:

- `require_explicit_wallet_selection_when_ambiguous = true`
- `allow_auto_route_when_exactly_one_match = true`
- `allow_account_listing_without_approval = true`
- `allow_public_key_lookup_without_approval = true`
- `fail_fast_when_wallet_unavailable = true`
- `fail_fast_when_wallet_locked = true`
- `allow_blind_signing = false`
- `allow_dev_backends = false`

The first release does not support a policy that bypasses interactive approval for transaction
signing.

## 9. Backend List Model

`wallet_backends` is the critical configuration change in the unified design.

Each backend entry has:

- `id`
- `backend_kind`
- `enabled`
- `label`
- `transport_kind`

Optional common fields:

- `priority`
- `allow_sign_transaction`
- `allow_sign_message`
- `allow_get_public_key`
- `allow_unlock`
- `protocol_version`

### 9.1 `starmask_extension` backend settings

Required settings:

- `allowed_extension_ids`
- `native_host_name`

Optional settings:

- `heartbeat_interval_seconds`
- `wallet_offline_after_seconds`
- `channel_override`

Notes:

- production and development extension IDs must remain separate
- Native Messaging manifest names must match `native_host_name`

### 9.2 `local_account_dir` backend settings

Required settings:

- `agent_socket_path`
- `account_provider = "local"`
- `local_account_dir`
- `prompt_mode`

Optional settings:

- `default_unlock_ttl_seconds`
- `max_unlock_ttl_seconds`
- `spawn_command`
- `spawn_args`

Rules:

- `local_account_dir` must be an absolute path
- the backend must verify filesystem ownership and permission safety before serving
- `prompt_mode` must be one of `tty` or `desktop`

### 9.3 `private_key_dev` backend settings

Required settings:

- `agent_socket_path`
- `account_provider = "private_key"`

Exactly one secret source:

- `secret_file`
- `from_env`

Optional settings:

- `prompt_mode`
- `spawn_command`
- `spawn_args`
- `unsafe_unattended = false`

Rules:

- disabled unless `allow_dev_backends = true`
- rejected in `production` channel
- must be clearly marked unsafe in diagnostics

## 10. MCP Adapter Settings

`starmask-mcp` should support only thin adapter configuration:

- daemon socket or pipe override
- RPC timeout override
- log level override

The MCP adapter must not carry wallet policy that diverges from daemon policy.

## 11. Native Host Settings

`starmask-native-host` should support:

- daemon socket or pipe override
- expected channel
- expected backend ID or label when needed for diagnostics
- log level override

It must not store wallet state or request state.

## 12. Environment Variable Mapping

Environment variables are appropriate for top-level overrides and simple diagnostics. Complex
backend arrays should live in the config file.

Suggested environment variable names:

- `STARMASKD_SOCKET_PATH`
- `STARMASKD_DB_PATH`
- `STARMASKD_LOG_PATH`
- `STARMASKD_CHANNEL`
- `STARMASKD_LOG_LEVEL`
- `STARMASKD_DEFAULT_REQUEST_TTL_SECONDS`
- `STARMASKD_RESULT_RETENTION_SECONDS`
- `STARMASKD_ALLOW_DEV_BACKENDS`

Suggested MCP adapter environment variables:

- `STARMASK_MCP_DAEMON_SOCKET_PATH`
- `STARMASK_MCP_RPC_TIMEOUT_MS`

Suggested local backend environment variables for development only:

- `STARMASK_LOCAL_AGENT_SOCKET_PATH`
- `STARMASK_LOCAL_ACCOUNT_DIR`
- `STARMASK_PRIVATE_KEY_SECRET_FILE`
- `STARMASK_PRIVATE_KEY_FROM_ENV`

## 13. Safe Bounds

The implementation should clamp or reject unsafe values:

1. `request_ttl_seconds` below minimum is raised to minimum
2. `request_ttl_seconds` above maximum is lowered to maximum
3. `unlock_ttl_seconds` above maximum is lowered to maximum
4. `result_retention_seconds` may not be zero in the first release
5. `private_key_dev` is rejected when `channel = "production"`

## 14. Configuration Errors

Misconfiguration should surface with actionable errors.

Typical cases:

- invalid extension ID allowlist
- missing Native Messaging manifest
- unsupported `backend_kind`
- unsupported `account_provider`
- insecure local account directory permissions
- ambiguous secret source for `private_key_dev`
- unwritable database path
- insecure socket permissions

## 15. Non-Goals

This document does not define package-manager-specific install commands or operating-system keyring
integration details.
