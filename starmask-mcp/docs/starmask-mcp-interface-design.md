# Starmask MCP Interface Design Draft

## 1. Purpose

This document defines the interface design for `starmask-mcp` when deployed with the following runtime topology:

- MCP host: Claude Code, Codex, or similar local MCP-capable host
- MCP entrypoint: `starmask-mcp`
- Local daemon: `starmaskd`
- Browser integration: Starmask Chrome extension
- Chrome bridge: `starmask-native-host`

The design goal is to provide a safe local signing entrypoint to MCP hosts without exposing private key material outside the Starmask extension.

## 2. Design Principles

1. `starmask-mcp` never holds private keys.
2. `starmaskd` never signs transactions.
3. The Chrome extension is the only signing authority.
4. Every transaction signing request requires explicit wallet approval unless a future policy layer says otherwise.
5. MCP hosts interact only with `starmask-mcp`.
6. All non-MCP transports are local-only.
7. Requests are asynchronous by default and identified by `request_id`.
8. Request creation is idempotent through `client_request_id`.
9. The first release fails fast when no connected unlocked wallet instance can satisfy a signing request.
10. The Rust implementation should keep protocol strings at the boundary and typed enums and newtypes in the core domain.

## 3. Runtime Topology

```mermaid
flowchart LR
    H["MCP Host"] --> M["starmask-mcp"]
    M --> D["starmaskd"]
    D --> N["starmask-native-host"]
    N --> E["Starmask Chrome Extension"]
```

### 3.1 Component Responsibilities

#### `starmask-mcp`

- Exposes MCP tools to the host.
- Validates tool inputs at the MCP boundary.
- Converts tool calls into daemon RPC requests.
- Returns structured results to the host.
- Should prefer the official Rust MCP SDK `rmcp` for MCP transport and tool wiring in the first Rust implementation.

#### `starmaskd`

- Maintains local request store and state machine.
- Tracks wallet availability and extension registrations.
- Routes requests between MCP clients and extension sessions.
- Enforces local policy, TTL, and rate limiting.
- Persists state required for retries and polling.

#### `starmask-native-host`

- Native Messaging shim launched by Chrome.
- Bridges Chrome extension messages to `starmaskd`.
- Does not implement wallet logic.

#### `Starmask Chrome Extension`

- Holds encrypted wallet state and unlock state.
- Parses Starcoin transactions and messages.
- Displays approval UI.
- Produces signatures and signed transactions.

## 4. Process Model

### 4.1 `starmask-mcp`

- Launch mode: on demand by MCP host
- Transport: MCP over stdio
- Lifetime: tied to MCP host session or tool use pattern

### 4.2 `starmaskd`

- Launch mode: long-lived user daemon
- Transport to clients: Unix domain socket on macOS/Linux, named pipe on Windows
- Lifetime: user session scoped

### 4.3 `starmask-native-host`

- Launch mode: on demand by Chrome via Native Messaging
- Transport to extension: Native Messaging stdin/stdout
- Transport to daemon: local socket/pipe
- Lifetime: tied to Chrome extension connection

### 4.4 Extension service worker

- Opens a persistent Native Messaging connection to keep the worker active while the wallet is online.
- Registers the current wallet instance with the daemon.

## 5. MCP Tool Surface

The initial MCP tool surface should be intentionally narrow.

### 5.1 `wallet_status`

#### Purpose

Return current wallet availability and extension connectivity.

#### Input

- no required parameters

#### Output

- `wallet_available`: boolean
- `wallet_online`: boolean
- `default_wallet_instance_id`: string or null
- `wallet_instances`: array of
  - `wallet_instance_id`
  - `extension_connected`
  - `active_accounts_count`
  - `lock_state`
- `message`: string

### 5.2 `wallet_list_accounts`

#### Purpose

List accounts currently exposed by Starmask to the local daemon.

#### Input

- `wallet_instance_id`: optional filter
- `include_public_key`: boolean, default `false`

Policy:

- the first release does not require an interactive approval gate for account listing
- account listing is treated as local metadata access, not signing authority

#### Output

- `wallet_instances`: array of
  - `wallet_instance_id`
  - `extension_connected`
  - `lock_state`
  - `accounts`
    - `address`
    - `label`
    - `public_key`: optional
    - `is_default`
    - `is_locked`

### 5.3 `wallet_get_public_key`

#### Purpose

Return the public key for a known account.

#### Input

- `address`
- `wallet_instance_id`: optional

If `wallet_instance_id` is omitted:

- the daemon may auto-select when exactly one wallet instance exposes the account
- otherwise the request fails with `wallet_selection_required`

Lookup rules:

- cached public keys may be returned without interactive approval
- if the key is unknown and the selected wallet is locked, the request fails with `wallet_locked`

#### Output

- `address`
- `public_key`
- `curve`: default `ed25519`

### 5.4 `wallet_request_sign_transaction`

#### Purpose

Create an asynchronous signing request for a Starcoin raw transaction.

#### Input

- `client_request_id`
- `account_address`
- `wallet_instance_id`: optional explicit route target
- `chain_id`
- `raw_txn_bcs_hex`
- `tx_kind`
  - `transfer`
  - `contract_call`
  - `publish_package`
  - `unknown`
- `display_hint`
  - optional human-readable description
- `client_context`
  - optional string, for example `codex` or `claude-code`
- `ttl_seconds`

If `wallet_instance_id` is omitted:

- the daemon may auto-route the request when exactly one known wallet instance exposes the requested account
- otherwise the request fails with `wallet_selection_required`

Creation policy:

- the selected wallet instance must be connected and unlocked
- the first release fails fast instead of queueing requests for a later wallet reconnect

#### Output

- `request_id`
- `client_request_id`
- `status`
  - initial value is typically `created`, and may advance to `dispatched` or `pending_user_approval` before the host polls again
- `wallet_instance_id`
- `created_at`
- `expires_at`
- `message`

### 5.5 `wallet_get_request_status`

#### Purpose

Poll signing request state.

#### Input

- `request_id`

#### Output

- `request_id`
- `client_request_id`
- `kind`
- `status`
  - one of the shared lifecycle states defined in `shared/protocol/request-lifecycle.md`
- `updated_at`
- `result_kind`
  - `signed_transaction`
  - `signed_message`
  - `none`
- `result_available`
- `result_expires_at`
- `error_code`: optional shared error code
- `reason`: optional
- `signed_txn_bcs_hex`: only when `approved`
- `signature`: only when `kind = sign_message` and `approved`

### 5.6 `wallet_cancel_request`

#### Purpose

Cancel a pending request if not yet approved.

#### Input

- `request_id`

#### Output

- `request_id`
- `status`
- `cancelled`: boolean

### 5.7 `wallet_sign_message`

#### Purpose

Support message signing for login, challenge-response, or off-chain authorization.

#### Input

- `client_request_id`
- `account_address`
- `wallet_instance_id`: optional explicit route target
- `message`
- `format`
  - `utf8`
  - `hex`
- `display_hint`
- `client_context`
- `ttl_seconds`

#### Output

- `request_id`
- `client_request_id`
- `kind`
- `status`
  - initial value is typically `created`, and may advance asynchronously
- `message`

## 6. MCP Result Semantics

MCP tool results should remain structured and deterministic.

### 6.1 Success

- Always include machine-readable status fields.
- Do not rely on free-form text for status transitions.

### 6.2 Pending

When the wallet action is waiting on the user:

- return `status = pending_user_approval`
- include `request_id`
- include a user-facing prompt such as `Please confirm in Starmask`

### 6.3 Errors

Use shared error codes from `shared/protocol/error-codes.md` wherever possible.

The most relevant shared errors for this project are:

- `wallet_unavailable`
- `wallet_locked`
- `wallet_selection_required`
- `wallet_instance_not_found`
- `extension_not_connected`
- `invalid_account`
- `request_expired`
- `request_not_found`
- `request_rejected`
- `idempotency_key_conflict`
- `invalid_transaction_payload`
- `unsupported_chain`
- `internal_bridge_error`

## 7. Daemon RPC Surface

The daemon-facing protocol may use JSON-RPC over local socket/pipe. The protocol should be versioned independently from MCP.

### 7.1 Methods exposed by `starmaskd` to `starmask-mcp`

- `wallet.status`
- `wallet.listInstances`
- `wallet.listAccounts`
- `wallet.getPublicKey`
- `request.createSignTransaction`
- `request.createSignMessage`
- `request.getStatus`
- `request.cancel`

### 7.2 Methods exposed by `starmaskd` to `starmask-native-host`

- `extension.register`
- `extension.heartbeat`
- `extension.updateAccounts`
- `request.pullNext`
- `request.presented`
- `request.resolve`
- `request.reject`

### 7.3 Internal Event Types

- `wallet.instance.connected`
- `wallet.instance.disconnected`
- `request.created`
- `request.available`
- `request.dispatched`
- `request.approved`
- `request.rejected`
- `request.expired`

## 8. Request Object

Each signing request should be persisted by the daemon.

The canonical envelope should align with `shared/schemas/wallet-sign-request.schema.json` and `shared/schemas/wallet-sign-result.schema.json`.

### 8.1 Common Required Fields

- `request_id`
- `client_request_id`
- `kind`
  - `sign_transaction`
  - `sign_message`
- `status`
- `account_address`
- `payload_hash`
- `created_at`
- `expires_at`

### 8.2 Common Optional Fields

- `wallet_instance_id`
- `client_context`
- `display_hint`
- `failure_reason`
- `error_code`
- `approved_at`
- `rejected_at`
- `updated_at`
- `result_expires_at`

### 8.3 Transaction-Signing Fields

Required when `kind = sign_transaction`:

- `chain_id`
- `raw_txn_bcs_hex`

Optional result fields:

- `signed_txn_bcs_hex`

### 8.4 Message-Signing Fields

Required when `kind = sign_message`:

- `message_bytes`
- `message_format`

Optional result fields:

- `signature`

## 9. Extension Registration

The daemon must distinguish multiple extension instances.

### 9.1 `wallet_instance_id`

The extension generates and persists a stable `wallet_instance_id` in extension-local storage on first run.

### 9.2 Registration Payload

When the extension connects through Native Messaging, it sends:

- `wallet_instance_id`
- `extension_id`
- `extension_version`
- `protocol_version`
- `profile_hint`
- `accounts_summary`
- `lock_state`

### 9.3 Version Compatibility

The daemon must reject incompatible protocol versions with an actionable error.

### 9.4 Wallet Instance Selection

The daemon must support multiple wallet instances.

Routing rules:

1. If the caller provides `wallet_instance_id`, the daemon routes only to that instance.
2. If the caller omits `wallet_instance_id` and exactly one wallet instance exposes the account, the daemon may auto-route.
3. If the caller omits `wallet_instance_id` and multiple wallet instances expose the account, the daemon must fail with `wallet_selection_required`.
4. If the caller references an unknown instance, the daemon must fail with `wallet_instance_not_found`.

## 10. Signing Flow

### 10.1 Transaction Signing

1. MCP host prepares unsigned transaction via a separate chain-facing MCP server.
2. MCP host calls `wallet_request_sign_transaction`.
3. `starmask-mcp` forwards the request to `starmaskd`.
4. `starmaskd` persists the request in `created`.
5. If a matching wallet instance is connected, the daemon emits `request.available`.
6. The extension claims the request through `request.pullNext`.
7. `starmaskd` moves the request to `dispatched`.
8. Once approval UI is shown, the extension calls `request.presented`.
9. `starmaskd` moves the request to `pending_user_approval`.
10. The user approves or rejects.
11. The extension signs locally and returns the result to `starmaskd`.
12. The host polls via `wallet_get_request_status`.
13. On approval, the host retrieves `signed_txn_bcs_hex`.

### 10.2 Message Signing

Same lifecycle as transaction signing, but the final object is a message signature instead of a signed transaction.

When `kind = sign_message`, `wallet_get_request_status` must return:

- `result_kind = signed_message`
- `signature`

### 10.3 Delivery and Recovery

The canonical delivery model is durable and lease-based.

- Requests are first persisted in `created`.
- `request.pullNext` assigns a delivery lease to a wallet instance.
- If the wallet disconnects before calling `request.presented`, the daemon may return the request to `created`.
- If the wallet disconnects after `request.presented`, the request remains pinned to the same `wallet_instance_id` and may only be resumed by that same instance.
- Approved results are retained for bounded multi-read retrieval until `result_expires_at`.
- Hosts must treat requests as asynchronous and poll until a terminal state is reached.

### 10.4 Restart Scenarios

Expected recovery behavior:

- If Chrome closes before approval UI is shown, the request remains recoverable and may be re-delivered.
- If the extension restarts while a request is pending, the daemon may re-deliver it after reconnect.
- If `starmaskd` restarts, persisted non-terminal requests should be reloaded from storage and resumed.
- If the MCP host restarts, the host may continue polling existing `request_id` values.

## 11. Approval UI Requirements

The approval UI belongs to the extension, not the daemon.

Minimum UI fields:

- account address
- chain id
- transaction type
- receiver or target function
- amount if applicable
- gas parameters
- expiration
- workspace or client context if present

The extension must decode and render the transaction itself. It must not trust `display_hint` as the source of truth.

## 12. Security Requirements

1. Private key material must never leave the extension.
2. `starmaskd` must store only non-secret request metadata and signed outputs.
3. Socket/pipe access must be scoped to the current OS user.
4. All requests must have TTL.
5. Blind signing must be disabled by default.
6. The extension must require explicit user confirmation for every signing request in the initial release.
7. Native host manifest must whitelist the exact production extension ID.
8. Dev, staging, and production extension IDs must use separate host manifests.

## 13. Installation and Deployment

### 13.1 Installed Artifacts

- Chrome extension: `Starmask`
- user daemon: `starmaskd`
- MCP shim: `starmask-mcp`
- Chrome native host shim: `starmask-native-host`
- optional admin utility: `starmaskctl`

### 13.2 Installation Sequence

1. Install the Chrome extension.
2. Install local binaries.
3. Register Chrome Native Messaging host manifest.
4. Register `starmask-mcp` in the MCP host configuration.
5. Open Chrome and let the extension register with the daemon.
6. Run `starmaskctl doctor`.

### 13.3 Native Messaging Manifest Constraints

- `allowed_origins` must reference the exact extension origin.
- Each release channel should have a separate manifest and host name if extension IDs differ.

## 14. Diagnostics

`starmaskctl doctor` should validate:

- daemon reachable
- MCP shim reachable
- native messaging manifest present
- Chrome extension connected
- protocol versions compatible
- wallet accounts visible

## 15. Resolved First-Release Decisions

Resolved first-release decisions:

1. `wallet_list_accounts` does not require an interactive approval gate in the first release.
2. The daemon supports multiple extension instances and requires explicit selection whenever routing is ambiguous.
3. Approved results use bounded multi-read retention, not single-read delivery.
4. Message-signing and transaction-signing requests share one canonical request table with kind-specific payload fields.
5. Low-risk message-signing policy exceptions are out of scope for the first release.
6. After `request.presented`, a request may be resumed only by the same `wallet_instance_id`.
7. Request creation is idempotent through required `client_request_id`.
