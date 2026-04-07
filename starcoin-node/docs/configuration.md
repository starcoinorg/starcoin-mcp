# Starcoin Node Configuration

## Purpose

This document defines the configuration surface for `starcoin-node`.

## Configuration Principles

1. explicit endpoint selection
2. fail closed on chain mismatch in transaction mode
3. least capability by default
4. bounded timeouts and watch intervals
5. secrets come from configuration, not tool input
6. bounded query size, payload size, and local concurrency budgets

## Configuration Sources

Precedence:

1. CLI flags
2. environment variables
3. config file
4. built-in defaults

## Required Rust Configuration Binding

The first conforming implementation is Rust, so runtime configuration should be represented with Rust-native typed configuration objects.

Required Rust approach:

- `clap` for CLI parsing
- `serde` for config deserialization
- one normalized runtime config struct after merge and validation
- raw config structs separated from validated runtime config structs
- time and retry settings normalized into `std::time::Duration`
- endpoint URLs parsed into typed URL values before runtime startup
- secret-bearing fields stored in redaction-aware wrappers rather than plain strings

Validation must happen before:

- starting CLI-driven tool execution or any future host adapter
- probing the RPC endpoint
- enabling transaction tools

Recommended Rust-native normalized types:

- `rpc_endpoint_url`
  - `url::Url`
- file and cache paths
  - `std::path::PathBuf`
- timeout and polling fields
  - `std::time::Duration`
- bounded numeric settings
  - non-zero integer wrappers where appropriate
- auth tokens and sensitive headers
  - secret wrappers that do not expose values through `Debug`
- chain pin configuration
  - one typed `ChainPin` struct rather than scattered optional strings

## Default Paths

### macOS

- config file:
  - preferred: `$HOME/Library/Application Support/StarcoinNode/node-cli.toml`
- log file:
  - `$HOME/Library/Logs/StarcoinNode/starcoin-node.log`
- cache directory:
  - `$HOME/Library/Caches/StarcoinNode/node/`

### Linux

- config file:
  - preferred: `$XDG_CONFIG_HOME/starcoin-node/node-cli.toml`
  - fallback: `$HOME/.config/starcoin-node/node-cli.toml`
- log file:
  - `$XDG_STATE_HOME/starcoin-node/starcoin-node.log`
  - fallback: `$HOME/.local/state/starcoin-node/starcoin-node.log`
- cache directory:
  - `$XDG_CACHE_HOME/starcoin-node/node/`
  - fallback: `$HOME/.cache/starcoin-node/node/`

### Windows

- config file:
  - preferred: `%APPDATA%\\StarcoinNode\\node-cli.toml`
- log file:
  - `%LOCALAPPDATA%\\StarcoinNode\\logs\\starcoin-node.log`
- cache directory:
  - `%LOCALAPPDATA%\\StarcoinNode\\cache\\node\\`

## Required Settings

### Endpoint Settings

- `rpc_endpoint_url`
- `mode`
  - `read_only`
  - `transaction`
- `vm_profile`
  - `auto`
    - shared RPC stays shared; dual-surface methods prefer the VM2 RPC surface
  - `vm1_only`
    - dual-surface methods require the VM1 RPC surface
  - `vm2_only`
    - dual-surface methods require the VM2 RPC surface

`vm_profile` is an RPC routing choice across three RPC categories: shared RPC, VM1 RPC
surface, and VM2 RPC surface. It does not declare semantic compatibility between VM1 and VM2
transactions, token codes, or resources.

### Chain Pin Settings

For `transaction` mode, the following settings are required:

- `expected_chain_id`
- `expected_network`
- `require_genesis_hash_match`

For remote `transaction` mode, the following setting should also be treated as required:

- `expected_genesis_hash`

For local `transaction` mode, `expected_genesis_hash` is still strongly recommended.

For `read_only` mode, chain pin settings are still strongly recommended.

They may be omitted only when:

- `allow_read_only_chain_autodetect = true`
- the operator explicitly accepts endpoint autodetection for that deployment

If `read_only` starts without `expected_chain_id` or `expected_network` under this override, startup should emit a high-severity warning that includes:

- the detected `chain_id`
- the detected network name
- the detected `genesis_hash` when available
- the fact that read-only queries are running without configured chain pins

## Optional Endpoint Settings

- `connect_timeout_ms`
- `request_timeout_ms`
- `startup_probe_timeout_ms`
- `rpc_auth_token_env`
- `rpc_headers`
- `tls_server_name`
- `allowed_rpc_hosts`
- `tls_pinned_spki_sha256`
- `allow_insecure_remote_transport`
- `allow_read_only_chain_autodetect`

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

The first release allows submission without prior simulation because a signed transaction may arrive from an external wallet flow.

If `allow_submit_without_prior_simulation = false`, the Rust implementation should fail closed unless the same node-side process already recorded a local preparation or `simulate_raw_transaction` attestation for the raw transaction with `simulation_status = performed`.

`submit_signed_transaction` should surface `prepared_simulation_status` when such a local record
exists so the host can tell whether the chain runtime observed a prior simulation.

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

## Resource Governance Settings

- `max_submit_blocking_timeout_seconds`
- `max_watch_timeout_seconds`
- `min_watch_poll_interval_seconds`
- `max_list_blocks_count`
- `max_events_limit`
- `max_account_resource_limit`
- `max_account_module_limit`
- `max_list_resources_size`
- `max_list_modules_size`
- `max_publish_package_bytes`
- `max_concurrent_watch_requests`
- `max_inflight_expensive_requests`

Recommended defaults:

- `max_submit_blocking_timeout_seconds = 60`
- `max_watch_timeout_seconds = 300`
- `min_watch_poll_interval_seconds = 2`
- `max_list_blocks_count = 100`
- `max_events_limit = 200`
- `max_account_resource_limit = 100`
- `max_account_module_limit = 50`
- `max_list_resources_size = 100`
- `max_list_modules_size = 100`
- `max_publish_package_bytes = 524288`
- `max_concurrent_watch_requests = 8`
- `max_inflight_expensive_requests = 16`

Rules:

- caller-supplied list and watch parameters should be clamped to these bounds when truncation preserves the tool's semantics
- oversized publish-package payloads should be rejected with `payload_too_large` rather than silently truncated
- local concurrency exhaustion should return `rate_limited` before outbound RPC side effects occur

## Policy Defaults

The first implementation should use these defaults:

- `mode = read_only`
- `vm_profile = auto`
- `require_genesis_hash_match = true`
- `allow_insecure_remote_transport = false`
- `allow_read_only_chain_autodetect = false`
- `allow_submit_without_prior_simulation = true`
- `disable_disk_cache = true`

These defaults keep the server conservative while still supporting the canonical wallet-orchestrated flow.

## Environment Variable Mapping

Suggested environment variable names:

- `STARCOIN_NODE_RPC_ENDPOINT_URL`
- `STARCOIN_NODE_CONNECT_TIMEOUT_MS`
- `STARCOIN_NODE_MODE`
- `STARCOIN_NODE_VM_PROFILE`
- `STARCOIN_NODE_STARTUP_PROBE_TIMEOUT_MS`
- `STARCOIN_NODE_EXPECTED_CHAIN_ID`
- `STARCOIN_NODE_EXPECTED_NETWORK`
- `STARCOIN_NODE_EXPECTED_GENESIS_HASH`
- `STARCOIN_NODE_REQUIRE_GENESIS_HASH_MATCH`
- `STARCOIN_NODE_RPC_AUTH_TOKEN`
- `STARCOIN_NODE_RPC_HEADERS`
- `STARCOIN_NODE_ALLOWED_RPC_HOSTS`
- `STARCOIN_NODE_TLS_SERVER_NAME`
- `STARCOIN_NODE_TLS_PINNED_SPKI_SHA256`
- `STARCOIN_NODE_REQUEST_TIMEOUT_MS`
- `STARCOIN_NODE_ALLOW_INSECURE_REMOTE_TRANSPORT`
- `STARCOIN_NODE_ALLOW_READ_ONLY_CHAIN_AUTODETECT`
- `STARCOIN_NODE_MAX_SUBMIT_BLOCKING_TIMEOUT_SECONDS`
- `STARCOIN_NODE_MAX_WATCH_TIMEOUT_SECONDS`
- `STARCOIN_NODE_MIN_WATCH_POLL_INTERVAL_SECONDS`
- `STARCOIN_NODE_MAX_LIST_BLOCKS_COUNT`
- `STARCOIN_NODE_MAX_EVENTS_LIMIT`
- `STARCOIN_NODE_MAX_ACCOUNT_RESOURCE_LIMIT`
- `STARCOIN_NODE_MAX_ACCOUNT_MODULE_LIMIT`
- `STARCOIN_NODE_MAX_LIST_RESOURCES_SIZE`
- `STARCOIN_NODE_MAX_LIST_MODULES_SIZE`
- `STARCOIN_NODE_MAX_PUBLISH_PACKAGE_BYTES`
- `STARCOIN_NODE_MAX_CONCURRENT_WATCH_REQUESTS`
- `STARCOIN_NODE_MAX_INFLIGHT_EXPENSIVE_REQUESTS`
- `STARCOIN_NODE_LOG_LEVEL`

Compatibility notes:

- `STARCOIN_NODE_CLI_CONFIG` is also accepted when selecting a config file path for `starcoin-node-cli`
- `STARCOIN_NODE_CLI_LOG_LEVEL` is accepted as a newer alias for `STARCOIN_NODE_LOG_LEVEL`
- transfer-workflow helper scripts still recognize `STARCOIN_NODE_MCP_CONFIG` while migrating older `starcoin-node-mcp` setups

Deprecated migration mapping from `STARCOIN_NODE_MCP_*` names:

- `STARCOIN_NODE_MCP_RPC_ENDPOINT_URL` -> `STARCOIN_NODE_RPC_ENDPOINT_URL`
- `STARCOIN_NODE_MCP_CONNECT_TIMEOUT_MS` -> `STARCOIN_NODE_CONNECT_TIMEOUT_MS`
- `STARCOIN_NODE_MCP_MODE` -> `STARCOIN_NODE_MODE`
- `STARCOIN_NODE_MCP_VM_PROFILE` -> `STARCOIN_NODE_VM_PROFILE`
- `STARCOIN_NODE_MCP_STARTUP_PROBE_TIMEOUT_MS` -> `STARCOIN_NODE_STARTUP_PROBE_TIMEOUT_MS`
- `STARCOIN_NODE_MCP_EXPECTED_CHAIN_ID` -> `STARCOIN_NODE_EXPECTED_CHAIN_ID`
- `STARCOIN_NODE_MCP_EXPECTED_NETWORK` -> `STARCOIN_NODE_EXPECTED_NETWORK`
- `STARCOIN_NODE_MCP_EXPECTED_GENESIS_HASH` -> `STARCOIN_NODE_EXPECTED_GENESIS_HASH`
- `STARCOIN_NODE_MCP_REQUIRE_GENESIS_HASH_MATCH` -> `STARCOIN_NODE_REQUIRE_GENESIS_HASH_MATCH`
- `STARCOIN_NODE_MCP_RPC_AUTH_TOKEN` -> `STARCOIN_NODE_RPC_AUTH_TOKEN`
- `STARCOIN_NODE_MCP_RPC_HEADERS` -> `STARCOIN_NODE_RPC_HEADERS`
- `STARCOIN_NODE_MCP_ALLOWED_RPC_HOSTS` -> `STARCOIN_NODE_ALLOWED_RPC_HOSTS`
- `STARCOIN_NODE_MCP_TLS_SERVER_NAME` -> `STARCOIN_NODE_TLS_SERVER_NAME`
- `STARCOIN_NODE_MCP_TLS_PINNED_SPKI_SHA256` -> `STARCOIN_NODE_TLS_PINNED_SPKI_SHA256`
- `STARCOIN_NODE_MCP_REQUEST_TIMEOUT_MS` -> `STARCOIN_NODE_REQUEST_TIMEOUT_MS`
- `STARCOIN_NODE_MCP_ALLOW_INSECURE_REMOTE_TRANSPORT` -> `STARCOIN_NODE_ALLOW_INSECURE_REMOTE_TRANSPORT`
- `STARCOIN_NODE_MCP_ALLOW_READ_ONLY_CHAIN_AUTODETECT` -> `STARCOIN_NODE_ALLOW_READ_ONLY_CHAIN_AUTODETECT`
- `STARCOIN_NODE_MCP_MAX_SUBMIT_BLOCKING_TIMEOUT_SECONDS` -> `STARCOIN_NODE_MAX_SUBMIT_BLOCKING_TIMEOUT_SECONDS`
- `STARCOIN_NODE_MCP_MAX_WATCH_TIMEOUT_SECONDS` -> `STARCOIN_NODE_MAX_WATCH_TIMEOUT_SECONDS`
- `STARCOIN_NODE_MCP_MIN_WATCH_POLL_INTERVAL_SECONDS` -> `STARCOIN_NODE_MIN_WATCH_POLL_INTERVAL_SECONDS`
- `STARCOIN_NODE_MCP_MAX_LIST_BLOCKS_COUNT` -> `STARCOIN_NODE_MAX_LIST_BLOCKS_COUNT`
- `STARCOIN_NODE_MCP_MAX_EVENTS_LIMIT` -> `STARCOIN_NODE_MAX_EVENTS_LIMIT`
- `STARCOIN_NODE_MCP_MAX_ACCOUNT_RESOURCE_LIMIT` -> `STARCOIN_NODE_MAX_ACCOUNT_RESOURCE_LIMIT`
- `STARCOIN_NODE_MCP_MAX_ACCOUNT_MODULE_LIMIT` -> `STARCOIN_NODE_MAX_ACCOUNT_MODULE_LIMIT`
- `STARCOIN_NODE_MCP_MAX_LIST_RESOURCES_SIZE` -> `STARCOIN_NODE_MAX_LIST_RESOURCES_SIZE`
- `STARCOIN_NODE_MCP_MAX_LIST_MODULES_SIZE` -> `STARCOIN_NODE_MAX_LIST_MODULES_SIZE`
- `STARCOIN_NODE_MCP_MAX_PUBLISH_PACKAGE_BYTES` -> `STARCOIN_NODE_MAX_PUBLISH_PACKAGE_BYTES`
- `STARCOIN_NODE_MCP_MAX_CONCURRENT_WATCH_REQUESTS` -> `STARCOIN_NODE_MAX_CONCURRENT_WATCH_REQUESTS`
- `STARCOIN_NODE_MCP_MAX_INFLIGHT_EXPENSIVE_REQUESTS` -> `STARCOIN_NODE_MAX_INFLIGHT_EXPENSIVE_REQUESTS`
- `STARCOIN_NODE_MCP_LOG_LEVEL` -> `STARCOIN_NODE_LOG_LEVEL`

Wrappers and launch scripts should translate these deprecated names during migration. A future release may drop the remaining compatibility shims entirely.

These names follow the precedence order defined earlier in this document:

- CLI flags override environment variables
- environment variables override config-file values
- config-file values override built-in defaults

In env-only deployments, unset optional endpoint variables fall back to config-file values when present, and otherwise to built-in defaults.

## Safe Bounds

The implementation should clamp unsafe timing values:

1. `watch_poll_interval_seconds` below `1` is raised to `1`
2. caller-supplied watch poll intervals below `min_watch_poll_interval_seconds` are raised to `min_watch_poll_interval_seconds`
3. caller-supplied watch timeouts above `max_watch_timeout_seconds` are lowered to `max_watch_timeout_seconds`
4. caller-supplied blocking submission timeouts above `max_submit_blocking_timeout_seconds` are lowered to `max_submit_blocking_timeout_seconds`
5. caller-supplied `count`, `limit`, `resource_limit`, `module_limit`, and `max_size` values above their configured maxima are lowered to those maxima
6. `default_expiration_ttl_seconds` below `30` is raised to `30`
7. `default_expiration_ttl_seconds` above `max_expiration_ttl_seconds` is lowered to the configured maximum
8. `warn_head_lag_seconds` above `max_head_lag_seconds` is lowered to `max_head_lag_seconds`

## Configuration Errors

Misconfiguration should surface with actionable errors.

Typical cases:

- missing `expected_chain_id` in transaction mode
- missing `expected_network` in transaction mode
- missing `expected_chain_id` or `expected_network` in `read_only` mode when `allow_read_only_chain_autodetect = false`
- missing `expected_genesis_hash` in remote transaction mode when `require_genesis_hash_match = true`
- invalid or unsupported `vm_profile`
- insecure remote endpoint without explicit override
- configured endpoint host not present in `allowed_rpc_hosts`
- malformed RPC header configuration
- negative or zero timeouts after normalization
- zero or negative resource-governance maxima after normalization
- zero-valued concurrency budgets for watches or expensive requests

## Non-Goals

This document does not define:

- package-manager-specific install commands
- wallet configuration
- node binary configuration
