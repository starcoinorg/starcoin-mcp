# Starcoin Transfer Workflow

This plugin example turns Codex into the transfer host client for a user-in-the-loop Starcoin
transfer flow.

The current direction is Plan B:

- keep the host sequencing in skills and scripts
- remove direct runtime dependence on in-tree stdio adapters for the converged transfer path
- keep wallet approval outside Codex
- keep chain-side transaction logic in Rust instead of reimplementing it in Python

The design document for that direction lives at:

- `docs/plan-b-script-skill-architecture.md`

## Current Runtime Model

The converged transfer path now looks like this:

```mermaid
flowchart LR
    H["Codex Skill + Scripts"] --> W["scripts/starmaskd_client.py"]
    H --> N["scripts/node_cli_client.py"]
    W --> D["starmaskd"]
    D --> A["local-account-agent or extension"]
    N --> C["starcoin-node-cli"]
    C --> R["starcoin-node-core"]
    R --> X["Starcoin RPC endpoint"]
```

The repository still contains:

- `starcoin-node`
- `starmask-runtime`

Those source trees remain because the chain CLI reuses `starcoin-node-core` and the wallet
runtime still uses `starmaskd` plus `local-account-agent`. The plugin bundle itself no longer
ships a plugin-managed adapter entrypoint.

## Main Files

- `.codex-plugin/plugin.json`
  - plugin manifest and UI metadata
- `docs/plan-b-script-skill-architecture.md`
  - phased design for the script + skill architecture
- `hooks/hooks.json`
  - startup runtime guardrail for Codex sessions
- `skills/starcoin-transfer/SKILL.md`
  - transfer workflow instructions for Codex
- `scripts/starmaskd_client.py`
  - direct JSON-RPC client for `starmaskd`
- `scripts/node_cli_client.py`
  - adapter that calls `starcoin-node-cli`
- `scripts/transfer_controller.py`
  - typed host-side transfer controller
- `scripts/transfer_host.py`
  - host-side preflight, risk labeling, preview, and audit helpers
- `scripts/wallet_runtime.py`
  - foreground wallet-side supervisor for `starmaskd + local-account-agent`
- `scripts/run_transfer_test.py`
  - one-shot transfer test through the direct daemon + CLI path

## Trust Boundary

The workflow still keeps the original trust split:

- `starmaskd` owns request lifecycle and wallet routing
- `local-account-agent` or the extension remains the signing authority
- `starcoin-node-cli` reuses `starcoin-node-core` for prepare, simulate, submit, and watch
- the host coordinates both sides, but does not merge them into one signer-aware runtime

## Runtime Prerequisites

The Plan B transfer path expects:

1. a valid chain-side config file for `starcoin-node-cli`
2. `starmaskd` to be running
3. a wallet backend to be registered with `starmaskd`
4. the daemon socket to be reachable

Default config locations now prefer `$HOME/.runtime`:

- node config:
  - `$HOME/.runtime/node-cli.toml`
- wallet config:
  - `$HOME/.runtime/config.toml`
- daemon socket:
  - `$HOME/.runtime/run/starmaskd.sock`

Repo-local example templates:

- `examples/node-cli.example.toml`
- `examples/starmaskd-local-account.example.toml`

`vm_profile` only affects RPC routes that have both VM1 and VM2 method names. Shared RPC such as
`chain.info`, `node.info`, and `txpool.gas_price` is profile-neutral.
`auto` is RPC routing, not per-account VM detection.
Transfer-oriented dual-surface tools such as `prepare_transfer` and `submit_signed_transaction`
follow the selected profile, while some account/resource reads may still begin on a VM1 read path
in `auto` and only use VM2 for repair or retry.
For fixed transfer semantics, prefer an explicit `vm1_only` or `vm2_only` choice plus a matching
`token_code`.
You can override the profile per host-side call with `scripts/node_cli_client.py --vm-profile ...`
or per end-to-end test with `scripts/run_transfer_test.py --vm-profile ...`.

## Isolated Dev Runtime

Keep the chain node data and signing wallet data in different directories.

Recommended layout:

- dev node data dir:
  - `<repo-root>/.runtime/devstack`
- standalone signer wallet dir:
  - `<repo-root>/.runtime/devwallet`

Why:

- the Starcoin node keeps a lock on its own `account_vaults`
- `local-account-agent` must open a wallet directory independently
- reusing the node-owned wallet directory causes `LOCK: Resource temporarily unavailable`

Example standalone wallet bootstrap:

```bash
chmod 700 <repo-root>/.runtime/devwallet
starcoin --connect ws://127.0.0.1:9870 --local-account-dir <repo-root>/.runtime/devwallet account create -p test123
starcoin --connect ws://127.0.0.1:9870 --local-account-dir <repo-root>/.runtime/devwallet account create -p test123
```

Example funding from the dev node side:

```bash
starcoin -n dev -d <repo-root>/.runtime/devstack dev get-coin <sender-address>
```

Those `starcoin` CLI examples are only for wallet bootstrap and local funding. The transfer flow
itself should use the script-driven `starmaskd` + `starcoin-node-cli` path.

## Optional Environment Overrides

Installed binaries on PATH take precedence automatically. For source-tree runs, the test path
accepts these overrides:

- `STARCOIN_NODE_CLI_BIN`
  - use an installed `starcoin-node-cli` binary
- `STARCOIN_NODE_CLI_MANIFEST`
  - override the Cargo manifest for the source-tree CLI launch
- `STARCOIN_NODE_CLI_CONFIG`
  - override the default node CLI config path
- `STARCOIN_TRANSFER_WORKSPACE_ROOT`
  - repo-relative workspace override for source-tree development
- `STARMASKD_BIN`
  - use an installed `starmaskd` binary
- `LOCAL_ACCOUNT_AGENT_BIN`
  - use an installed `local-account-agent` binary

## Wallet Runtime

Preferred local-account flow:

1. start the wallet supervisor in one terminal
2. keep that terminal open for CLI approval cards
3. run `python3 ./scripts/doctor.py`
4. run the host-side transfer test or ask Codex to prepare a transfer

If the daemon socket path exists but the doctor reports `Connection refused`, rerun:

```bash
python3 ./scripts/doctor.py --cleanup-stale-socket
```

Recommended wallet-side startup:

```bash
python3 ./scripts/wallet_runtime.py up \
  --wallet-dir $HOME/.runtime/devwallet \
  --chain-id 254
```

The supervisor writes `wallet-runtime.json` under `$HOME/.runtime/wallet-runtime/` by default and keeps
`local-account-agent` attached to the current terminal so `tty_prompt` approvals still work.

## Transfer Flow

The converged Plan B flow is:

1. Codex resolves the user transfer intent into network, sender, receiver, token, amount, and wallet instance
2. `wallet.listInstances` through `scripts/starmaskd_client.py`
3. `wallet.listAccounts` through `scripts/starmaskd_client.py`
4. `chain_status` and `node_health` through `scripts/node_cli_client.py`
5. optional `wallet.getPublicKey`
6. `prepare_transfer` through `starcoin-node-cli`
7. `get_account_overview` for sender and receiver through `starcoin-node-cli`
8. host-side preflight preview and risk labels
9. host-side confirmation
10. `request.createSignTransaction` through `starmaskd`
11. CLI or wallet approval plus `request.getStatus`
12. `submit_signed_transaction` and `watch_transaction` through `starcoin-node-cli` until the requested confirmation depth is met
13. local JSONL audit record for preview, approval lifecycle, and submit result

If the local runtime is not ready, the right recovery is to stop and run `doctor.py`, not to fall
back to direct `starcoin` CLI transfer commands.

The scripts assume Codex has already resolved the intent. Natural-language extraction and precise
follow-up questions remain a skill-level responsibility, not a Python parser feature.

## CLI Transfer Test

Two modes are supported.

### One-shot Test

This mode starts wallet-side processes inside the test script:

```bash
python3 ./scripts/run_transfer_test.py \
  --rpc-url http://127.0.0.1:9850 \
  --wallet-dir <repo-root>/.runtime/devwallet \
  --sender <sender-address> \
  --receiver <receiver-address> \
  --amount 1 \
  --amount-unit stc \
  --vm-profile vm2_only \
  --min-confirmed-blocks 3 \
  --audit-log-path <repo-root>/.runtime/transfer-audit.jsonl
```

### Reuse A Running Wallet Supervisor

This mode reuses an already-running wallet runtime:

```bash
python3 ./scripts/run_transfer_test.py \
  --rpc-url http://127.0.0.1:9850 \
  --wallet-runtime-dir $HOME/.runtime/wallet-runtime \
  --sender <sender-address> \
  --receiver <receiver-address> \
  --amount 1 \
  --amount-unit stc \
  --vm-profile vm2_only \
  --min-confirmed-blocks 3
```

In one-shot mode, `run_transfer_test.py` does this:

1. probes the node and derives `chain_id`, `network`, and `genesis_hash`
2. writes isolated `node-cli.toml` and `starmaskd.toml` files under a unique `.runtime/` directory
3. starts `starmaskd`
4. starts `local-account-agent`
5. talks directly to `starmaskd` for wallet discovery, request creation, and status polling
6. calls `starcoin-node-cli` for `prepare_transfer`, `node_health`, `get_account_overview`, `submit_signed_transaction`, and follow-up `watch_transaction`
7. shows a host-side preflight preview card plus risk labels before wallet signing
8. blocks immediately if the preview finds a blocking risk such as RPC unavailability or insufficient balance
9. waits for the local wallet CLI approval card in the same terminal
10. appends JSONL audit records under the active runtime directory unless `--audit-log-path` overrides it

In supervisor-reuse mode, steps 3 and 4 are skipped. The script reads
`$HOME/.runtime/wallet-runtime/wallet-runtime.json`, reuses the daemon socket and wallet instance, and
runs the same direct daemon + CLI host flow.

`prepare_transfer.amount` is a raw on-chain integer. The test script accepts `--amount-unit stc`
for human-readable STC input and normalizes it to raw units before calling `prepare_transfer`.
`1 STC = 1_000_000_000` raw units.
If `--token-code` is omitted, the default STC token code now follows `--vm-profile`:

- `vm1_only` -> `0x1::STC::STC`
- `auto` -> `0x1::starcoin_coin::STC`
- `vm2_only` -> `0x1::starcoin_coin::STC`

The workflow does not automatically switch between `0x1::STC::STC` and
`0x1::starcoin_coin::STC`. If the connected chain expects one specific STC token code on one VM
surface, pass that token code explicitly.

Final success now also depends on confirmation depth:

- `--min-confirmed-blocks 2` is the default
- the count includes the inclusion block itself
- the default therefore means the inclusion block plus at least 1 additional observed block
- `--min-confirmed-blocks 1` means inclusion-only success

The transfer script maps that one higher-level setting onto both `submit_signed_transaction` and
`watch_transaction`, so the blocking submit path and any follow-up watch use the same semantics.

If submission is accepted but final confirmation is still missing, the script reports that as an
intermediate state and exits non-zero instead of treating it as a completed successful transfer.

`run_transfer_test.py` now also runs a host-side preflight step before wallet signing:

- `node_health` validates that the RPC path is currently usable
- `get_account_overview` provides sender balance, token visibility, and `next_sequence_number_hint`
- the preview compares the latest `chain_status` with the prepared `chain_context`
- fee estimates come from `prepare_transfer.raw_txn` plus `simulation.gas_used`
- blocking risks stop the flow before `request.createSignTransaction`

By default the audit file is written to:

- `<runtime-dir>/audit/transfer-audit.jsonl` for one-shot runs
- `<wallet-runtime-dir>/audit/transfer-audit.jsonl` for supervisor-reuse runs

## Notes

- This plugin example is repo-local. It lives under the current workspace so you can inspect and modify it directly.
- If you want a global plugin instead, move the same files under `~/plugins/starcoin-transfer-workflow/` and mirror the marketplace entry into `~/.agents/plugins/marketplace.json`.
- In global mode, put `starcoin-node-cli`, `starmaskd`, and `local-account-agent` somewhere on PATH.
