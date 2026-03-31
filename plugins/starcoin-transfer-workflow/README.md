# Starcoin Transfer Workflow

This is a repo-local Codex plugin example that turns Codex into the transfer host client.

The plugin does not add a third chain daemon. It packages the host-side orchestration into:

- `.mcp.json`
  - registers `starcoin-node-mcp` and `starmask-mcp` as local stdio servers
- `hooks/hooks.json`
  - runs a startup health check when Codex opens the workspace
- `skills/starcoin-transfer/SKILL.md`
  - tells Codex how to run `prepare -> confirm -> sign -> submit -> watch`
- `examples/node-mcp.example.toml`
  - copyable transaction-mode template for `starcoin-node-mcp`
- `examples/starmaskd-local-account.example.toml`
  - copyable local-account template for `starmaskd`
- `scripts/doctor.py`
  - checks whether the local environment is ready for the workflow
- `scripts/wallet_runtime.py`
  - runs a foreground wallet-side supervisor for `starmaskd + local-account-agent`
- `scripts/run_transfer_test.py`
  - generates isolated runtime configs and runs one CLI-based user-in-the-loop transfer test

## What "Host Client" Means Here

In this design, Codex itself is the host client.

The plugin makes that happen by:

1. adding both MCP servers to the host
2. packaging one transfer skill with strict sequencing and safety rules
3. keeping wallet approval inside the wallet surface

That means:

- `starcoin-node-mcp` prepares and submits transactions
- `starmask-mcp` creates signing requests
- `starmaskd` plus the wallet backend remain outside Codex
- the final approval still happens in the wallet UI or CLI approval card

## Files

- `.codex-plugin/plugin.json`
  - plugin manifest and UI metadata
- `.mcp.json`
  - stdio MCP server registration for Codex
- `hooks/hooks.json`
  - startup runtime guardrail for Codex sessions
- `skills/starcoin-transfer/SKILL.md`
  - transfer workflow instructions for Codex
- `examples/node-mcp.example.toml`
  - starter node config for one transfer workflow
- `examples/starmaskd-local-account.example.toml`
  - starter wallet-daemon config for one local-account backend
- `scripts/doctor.py`
  - local environment diagnostics
- `scripts/wallet_runtime.py`
  - foreground wallet-side supervisor with `up / status / down`
- `scripts/run_transfer_test.py`
  - one-shot Python host client for a local transfer test

## Runtime Prerequisites

The plugin expects:

1. `starcoin-node-mcp` to have a valid config file
2. `starmaskd` to be running
3. a wallet backend to be registered with `starmaskd`
4. `starmask-mcp` to be able to reach the daemon socket

Default config locations:

- macOS node config:
  - `~/Library/Application Support/StarcoinMCP/node-mcp.toml`
- macOS wallet config:
  - `~/Library/Application Support/StarcoinMCP/config.toml`
- macOS daemon socket:
  - `~/Library/Application Support/StarcoinMCP/run/starmaskd.sock`

Repo-local example templates:

- `plugins/starcoin-transfer-workflow/examples/node-mcp.example.toml`
- `plugins/starcoin-transfer-workflow/examples/starmaskd-local-account.example.toml`

## Isolated Dev Runtime

If you want one safe local test flow, keep the chain node data and the signing wallet data in
different directories.

Recommended layout:

- dev node data dir:
  - `<repo-root>/.runtime/devstack`
- standalone signer wallet dir:
  - `<repo-root>/.runtime/devwallet`

Why this split matters:

- the Starcoin node keeps a lock on its own `account_vaults`
- `local-account-agent` must open a wallet directory independently
- reusing the node-owned wallet directory causes `LOCK: Resource temporarily unavailable`

Example standalone wallet creation against a running dev node:

```bash
chmod 700 <repo-root>/.runtime/devwallet
starcoin --connect ws://127.0.0.1:9870 --local-account-dir <repo-root>/.runtime/devwallet account create -p test123
starcoin --connect ws://127.0.0.1:9870 --local-account-dir <repo-root>/.runtime/devwallet account create -p test123
```

Then fund the sender from the dev node side:

```bash
starcoin -n dev -d <repo-root>/.runtime/devstack dev get-coin <sender-address>
```

## Optional Environment Overrides

The plugin prefers source-tree launches through `cargo run`, but you can override that.

Installed binaries on PATH take precedence automatically. The override variables below are only
needed when the executable name or location differs from the default PATH lookup.

Node MCP overrides:

- `STARCOIN_NODE_MCP_BIN`
  - use an already installed `starcoin-node-mcp` binary
- `STARCOIN_MCP_WORKSPACE_ROOT`
  - point repo-relative manifest defaults at a checked-out `starcoin-mcp` workspace
- `STARCOIN_NODE_MCP_MANIFEST`
  - override the Cargo manifest path for source-tree launch
- `STARCOIN_NODE_MCP_CONFIG`
  - pass a non-default config file to the node server

Wallet MCP overrides:

- `STARMASK_MCP_BIN`
  - use an already installed `starmask-mcp` binary
- `STARMASK_MCP_MANIFEST`
  - override the Cargo manifest path for source-tree launch
- `STARMASK_MCP_DAEMON_SOCKET_PATH`
  - pass a non-default daemon socket path to `starmask-mcp`

Wallet runtime overrides:

- `STARMASKD_BIN`
  - use an already installed `starmaskd` binary
- `LOCAL_ACCOUNT_AGENT_BIN`
  - use an already installed `local-account-agent` binary

## Wallet Stack

`starmask-mcp` is only the MCP adapter. The wallet runtime still has to exist first.

Preferred local-account flow:

1. start the wallet supervisor in one terminal
2. keep that terminal open for CLI approval cards
3. open Codex on this workspace so the plugin marketplace is visible
4. run `python3 ./plugins/starcoin-transfer-workflow/scripts/doctor.py`
5. run the host-side test or ask Codex to prepare a transfer from another terminal

Recommended wallet-side startup:

```bash
python3 ./plugins/starcoin-transfer-workflow/scripts/wallet_runtime.py up \
  --wallet-dir <repo-root>/.runtime/devwallet \
  --chain-id 254
```

The supervisor writes `wallet-runtime.json` under `.runtime/wallet-runtime/` and keeps
`local-account-agent` attached to the current terminal so `tty_prompt` approvals still work.

For a global plugin install, the same command works as long as `starmaskd` and
`local-account-agent` are on PATH. In that mode the script no longer needs
`STARCOIN_MCP_WORKSPACE_ROOT` just to launch the wallet side.

When the plugin is active, Codex also runs a session-start hook. If the transfer runtime is not
ready, the hook emits one concise warning and points back to the doctor script.

## Transfer Flow In Codex

Once the plugin is loaded, Codex can handle one transfer like this:

1. `wallet_list_instances`
2. `wallet_list_accounts`
3. optional `wallet_get_public_key`
4. `prepare_transfer`
5. host-side confirmation in chat
6. `wallet_request_sign_transaction`
7. CLI or wallet approval
8. `wallet_get_request_status`
9. `submit_signed_transaction`
10. optional `watch_transaction`

## CLI Transfer Test

There are now two supported test modes.

### One-shot Test

This mode is self-contained and starts wallet-side processes inside the test script:

```bash
python3 ./plugins/starcoin-transfer-workflow/scripts/run_transfer_test.py \
  --rpc-url http://127.0.0.1:9850 \
  --wallet-dir <repo-root>/.runtime/devwallet \
  --sender <sender-address> \
  --receiver <receiver-address>
```

### Reuse A Running Wallet Supervisor

This mode is the more converged flow. Start the wallet supervisor once, then point the transfer
test at its metadata directory:

```bash
python3 ./plugins/starcoin-transfer-workflow/scripts/run_transfer_test.py \
  --rpc-url http://127.0.0.1:9850 \
  --wallet-runtime-dir <repo-root>/.runtime/wallet-runtime \
  --sender <sender-address> \
  --receiver <receiver-address>
```

In one-shot mode, `run_transfer_test.py` does this:

1. probes the node and derives `chain_id`, `network`, and `genesis_hash`
2. creates a unique per-run runtime directory under `.runtime/` and writes isolated `node-mcp.toml` and `starmaskd.toml` files there
3. starts `starmaskd`
4. starts `local-account-agent`
5. starts `starcoin-node-mcp` and `starmask-mcp`
6. runs `wallet_get_public_key -> prepare_transfer -> wallet_request_sign_transaction -> wallet_get_request_status -> submit_signed_transaction`
7. shows a host-side confirmation card before wallet signing
8. waits for the local wallet CLI approval card in the same terminal

In supervisor-reuse mode, steps 3 and 4 are skipped. The script reads
`.runtime/wallet-runtime/wallet-runtime.json`, reuses the running daemon socket and wallet instance,
and only starts the MCP-side pieces needed for the host flow.

The wallet approval remains the final consent point. If the account is locked, the terminal will
also ask for the account password after you choose `approve`.

The script defaults to `--token-code 0x1::starcoin_coin::STC`, which matches the dev-chain STC
module path used by current Starcoin nodes.

The local wallet backend approval card is already implemented in:

- `starmask-mcp/crates/starmask-local-account-agent/src/tty_prompt.rs`

## Notes

- This plugin example is repo-local. It lives under the current workspace so you can inspect and modify it directly.
- If you want a global plugin instead, move the same files under `~/plugins/starcoin-transfer-workflow/` and mirror the marketplace entry into `~/.agents/plugins/marketplace.json`.
- In global mode, put `starcoin-node-mcp`, `starmask-mcp`, `starmaskd`, and `local-account-agent` somewhere on PATH. `~/bin` is fine, but any PATH directory works.
