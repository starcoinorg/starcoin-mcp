# Starmask Wallet Backend Configuration

## Status

This document is the phase-2 configuration contract for the planned multi-backend implementation.

It is not part of the current `v1` configuration contract. The current extension-backed `v1`
configuration remains defined by:

- `docs/configuration.md`

## 1. Purpose

This document defines the configuration model needed to implement:

- generic backend registration
- `local_account_dir` integration

The goal is to make multi-backend runtime wiring deterministic before coding starts.

## 2. Configuration Model

Phase-2 configuration has two layers:

1. global daemon settings
2. per-backend entries in `[[wallet_backends]]`

The same config file may be shared by:

- `starmaskd`
- a local backend agent started with `--config <path> --backend-id <id>`

This keeps daemon policy and backend identity in one authoritative place.

## 3. Source Precedence

Phase-2 keeps the current precedence order:

1. CLI flags
2. environment variables
3. config file
4. built-in defaults

Phase-2 design choice:

- per-backend fields are config-file-only in the initial rollout
- environment variables remain limited to global path, log, and timing overrides

This avoids inventing an unbounded environment-variable matrix for backend-specific options.

## 4. Global Daemon Settings

Global settings remain close to the current `v1` model:

- `channel`
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

Phase-2 adds:

- `wallet_backends`

Phase-2 removes extension-specific trust fields from the global scope:

- `allowed_extension_ids`
- `native_host_name`

Those move into backend entries for `starmask_extension`.

## 5. Common Backend Entry Fields

Every `[[wallet_backends]]` entry must define:

- `backend_id`
- `backend_kind`
- `enabled`
- `instance_label`
- `approval_surface`

Rules:

1. `backend_id` must be unique
2. `backend_id` must be stable across restarts
3. `enabled = false` entries are ignored at runtime but still validated for syntax
4. `approval_surface` must be valid for the selected backend kind

Supported `backend_kind` values in phase 2:

- `starmask_extension`
- `local_account_dir`

Supported `approval_surface` values in phase 2:

- `browser_ui`
- `tty_prompt`
- `desktop_prompt`

## 6. `starmask_extension` Backend Entry

Required fields:

- `backend_id`
- `backend_kind = "starmask_extension"`
- `enabled`
- `instance_label`
- `approval_surface = "browser_ui"`
- `allowed_extension_ids`
- `native_host_name`

Optional fields:

- `profile_hint`

Rules:

1. `allowed_extension_ids` must be non-empty
2. `native_host_name` must match the Native Messaging manifest name
3. production channel must reject development extension IDs

## 7. `local_account_dir` Backend Entry

Required fields:

- `backend_id`
- `backend_kind = "local_account_dir"`
- `enabled`
- `instance_label`
- `approval_surface`
- `account_dir`
- `prompt_mode`
- `unlock_cache_ttl_seconds`

Optional fields:

- `allow_read_only_accounts`, default `true`
- `require_strict_permissions`, default `true`

Rules:

1. `approval_surface` must be `tty_prompt` or `desktop_prompt`
2. `prompt_mode` must match `approval_surface`
3. `account_dir` must resolve to one canonical local directory
4. `unlock_cache_ttl_seconds` must be positive and bounded
5. if `require_strict_permissions = true`, startup fails on insecure filesystem ownership or mode

## 8. Reserved Future Backend Kind

`private_key_dev` remains phase-4 work.

Phase-2 design choice:

1. phase-2 config loaders must reject `backend_kind = "private_key_dev"`
2. future development-only backend config should be added only when the phase-4 rollout begins

## 9. Recommended Config Example

```toml
channel = "development"
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

[[wallet_backends]]
backend_id = "browser-default"
backend_kind = "starmask_extension"
enabled = true
instance_label = "Browser Default"
approval_surface = "browser_ui"
allowed_extension_ids = ["kmheclfnfmpacglnpegeohempmedhiaf"]
native_host_name = "com.starcoin.starmask.development"

[[wallet_backends]]
backend_id = "local-main"
backend_kind = "local_account_dir"
enabled = true
instance_label = "Local Main"
approval_surface = "tty_prompt"
prompt_mode = "tty_prompt"
account_dir = "/Users/alice/.starcoin/account"
unlock_cache_ttl_seconds = 300
allow_read_only_accounts = true
require_strict_permissions = true
```

## 10. Validation Rules

Phase-2 configuration loading must fail fast when:

1. no enabled backend entries exist
2. `backend_id` values are duplicated
3. `backend_kind` is unknown
4. a backend uses an invalid `approval_surface`
5. `local_account_dir` points to a missing or insecure directory
6. `starmask_extension` omits extension allowlist or host name
7. a reserved future backend kind such as `private_key_dev` is configured during phase 2
8. `wallet_offline_after_seconds <= heartbeat_interval_seconds`

## 11. Compatibility Mode

Phase-2 should provide one migration bridge from the current `v1` config:

1. if `wallet_backends` is absent, the loader may translate legacy top-level extension settings into
   one implicit `starmask_extension` backend with `backend_id = "browser-default"`
2. if `wallet_backends` is present, legacy top-level extension fields must be rejected

This avoids ambiguous precedence between old and new config shapes.

## 12. Backend Agent Startup Contract

The initial local-account agent should start with:

```text
local-account-agent --config <path> --backend-id <backend_id>
```

Runtime rules:

1. the agent reads exactly one backend entry by `backend_id`
2. the agent derives `wallet_instance_id` from that `backend_id`
3. the agent connects to the daemon socket from the global config
4. the agent must refuse to start if the selected backend entry is disabled or has the wrong
   `backend_kind`

## 13. Performance and Operations Notes

Configuration should help keep the system bounded.

Required properties:

1. unlock cache TTL must be finite
2. result retention must be finite
3. polling and heartbeat timings must remain configurable but bounded
4. backend entries must be explicit rather than discovered from arbitrary local directories

## 14. Relationship to Other Documents

This document should be read together with:

- `docs/unified-wallet-coordinator-evolution.md`
- `docs/wallet-backend-agent-contract.md`
- `docs/wallet-backend-local-socket-binding.md`
- `docs/wallet-backend-security-model.md`
