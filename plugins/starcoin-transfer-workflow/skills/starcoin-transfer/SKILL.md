---
name: starcoin-transfer
description: Use when the user wants a user-in-the-loop Starcoin transfer through Codex. This skill orchestrates local wallet and chain scripts while keeping prepare, confirm, sign, submit, and watch steps bound together.
---

# Starcoin Transfer

This skill turns Codex into the host-side transfer controller without depending on plugin-managed
stdio adapters.

The canonical execution path is:

- wallet-side calls through `scripts/starmaskd_client.py`
- chain-side calls through `scripts/node_cli_client.py`
- host-side sequencing and state retention in Codex

Use this skill when the user wants to transfer tokens, request a signature for a prepared transfer,
or track the final transaction state.

## CLI Quick Reference

Use these forms directly. Do not call `--help` first unless one of these commands fails or the
script path itself has changed.

- Wallet status and discovery:
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/starmaskd_client.py call wallet_list_instances`
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/starmaskd_client.py call wallet_list_accounts`
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/starmaskd_client.py call wallet_get_public_key`
- Chain status and reads:
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/node_cli_client.py call chain_status`
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/node_cli_client.py call node_health`
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/node_cli_client.py call get_account_overview`
- Runtime checks:
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/doctor.py`
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/wallet_runtime.py status`
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/wallet_runtime.py up --replace`
- End-to-end test:
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/run_transfer_test.py --rpc-url http://127.0.0.1:9850 --wallet-runtime-dir $HOME/.runtime/wallet-runtime --sender <sender> --receiver <receiver> --amount 1 --amount-unit stc --vm-profile vm2_only --min-confirmed-blocks 3`

Default runtime locations for this workflow:

- wallet runtime: `$HOME/.runtime/wallet-runtime`
- wallet dir: `$HOME/.runtime/devwallet`
- node config: `$HOME/.runtime/node-cli.toml`
- wallet config: `$HOME/.runtime/wallet-runtime/starmaskd.toml`

Known important parameters:

- `starmaskd_client.py`
  - `--wallet-runtime-dir <dir>`
  - `--socket-path <sock>`
- `node_cli_client.py`
  - `--config <node-cli.toml>`
  - `--vm-profile <auto|vm1_only|vm2_only>`
  - `--workspace-root <starcoin-mcp-root>`
- `wallet_runtime.py`
  - `--runtime-dir <dir>`
  - `up --wallet-dir <dir> --chain-id <id> --backend-id <id> --replace`
- `run_transfer_test.py`
  - `--rpc-url <http-rpc>`
  - `--wallet-runtime-dir <dir>` or `--wallet-dir <dir>`
  - `--sender <address> --receiver <address>`
  - `--amount <value> [--amount-unit raw|stc]`
  - `--vm-profile <auto|vm1_only|vm2_only>`
  - `--min-confirmed-blocks <n>`
  - `--token-code <vm-profile-matched-stc-or-explicit-token>`
  - `--audit-log-path <transfer-audit.jsonl>`

## Workflow

The current script path does not parse free-form transfer language by itself. Codex should resolve
the transfer intent first, then use the scripts for deterministic execution.

### 1. Capture The Transfer Intent

- Extract or confirm: network, sender account, receiver address, token code, amount, and wallet instance.
- If a field is missing, ask one precise follow-up question instead of a broad prompt.
- If routing is ambiguous, list the concrete candidates from `wallet_list_instances` and `wallet_list_accounts`.
- Do not start chain-side preparation until the transfer intent is unambiguous.

### 2. Validate The Resolved Inputs

- Check that sender and receiver look like valid Starcoin addresses before preparation.
- Check that the selected token code is explicit and consistent with the chosen `vm_profile`.
- `vm_profile` is RPC routing, not per-account VM detection.
- Do not automatically switch between `0x1::STC::STC` and `0x1::starcoin_coin::STC`.
- If the user gives a human-readable non-STC amount and decimals are not already known from trusted metadata, ask instead of guessing.

### 3. Gather Chain And Wallet Context

- If the runtime might not be ready, stop early and ask the user to run `python3 ./plugins/starcoin-transfer-workflow/scripts/doctor.py`.
- Use `wallet_list_instances` and `wallet_list_accounts` to discover wallet candidates.
- Use `chain_status` to inspect chain identity.
- Use `node_health` to inspect RPC availability, peer count, and lag warnings.
- If the sender public key is not already known, call `wallet_get_public_key`.

### 4. Prepare The Transaction

- `prepare_transfer.amount` expects the raw on-chain integer amount.
- If the user gives a human-readable STC amount and `token_code` is omitted or equals `0x1::STC::STC` or `0x1::starcoin_coin::STC`, normalize it with 9 decimals before preparation. `1 STC = 1_000_000_000` raw units.
- Shared RPC such as `chain.info`, `node.info`, and `txpool.gas_price` is profile-neutral, while transfer-oriented dual-surface tools such as `prepare_transfer` and `submit_signed_transaction` follow the selected profile.
- If `token_code` is omitted, the workflow default STC token code follows `vm_profile`:
  - `vm1_only` -> `0x1::STC::STC`
  - `auto` -> `0x1::starcoin_coin::STC`
  - `vm2_only` -> `0x1::starcoin_coin::STC`
- If the chosen STC token code fails during dry-run, stop and ask for the correct `token_code` instead of retrying on another STC alias.
- For non-STC assets, only normalize a human-readable amount when decimals are already known from trusted chain metadata or prior explicit context. Otherwise ask instead of guessing.
- Call `prepare_transfer` through `node_cli_client.py`.
- Retain these fields from the result:
  - `transaction_kind`
  - `raw_txn_bcs_hex`
  - `chain_context`
  - `transaction_summary`
  - `simulation_status`
  - `simulation`
  - `execution_facts`
  - `next_action`
- If preparation fails with `invalid_address`, `invalid_asset`, `invalid_amount`, `simulation_failed`, `invalid_chain_context`, or `rpc_unavailable`, stop and explain the failure instead of creating a signing request.

### 5. Run Host Preflight

- Query `get_account_overview` for the sender before signing so the host can see balance and `next_sequence_number_hint`.
- Query `get_account_overview` for the receiver so the host can see whether the account already exists on-chain.
- Derive nonce, gas, and fee estimates from `prepare_transfer.execution_facts`.
- Compare the latest `chain_status` with `prepare_transfer.chain_context`.
- Generate risk labels for at least:
  - RPC unavailable or degraded
  - sender balance below transfer amount
  - sender gas balance below estimated fee
  - sequence / nonce moving ahead after preparation
  - receiver account missing on-chain

### 6. Show The Transaction Preview

- Summarize the prepared transaction in Codex before creating a signing request.
- Include network, sender, receiver, token, amount, raw amount, nonce, fee estimate, balance, and simulation outcome.
- Show the generated risk labels separately from the happy-path preview.
- If any blocking risk is present, stop before wallet signing.

### 7. Require Host Confirmation

- Ask for explicit confirmation after the preview and risk labels are shown.
- Do not ask the wallet to sign until the user has clearly confirmed the prepared transfer.

### 8. Create The Signing Request

- Call `wallet_request_sign_transaction` through `starmaskd_client.py` only after host confirmation.
- Copy `raw_txn_bcs_hex` directly from `prepare_transfer.raw_txn_bcs_hex`.
- Copy `tx_kind` directly from `prepare_transfer.transaction_kind`.
- `display_hint` may be derived from the chain-side summary, but it is only supportive context.
- Tell the user where approval will happen.
  - For `local_account_dir`, approval appears in the CLI approval card.

### 9. Wait For Wallet Approval

- Poll `wallet_get_request_status` through `starmaskd_client.py`.
- Stop on terminal failure states:
  - `rejected`
  - `cancelled`
  - `expired`
  - `failed`
- When the request becomes `approved`, extract `result.signed_txn_bcs_hex`.

### 10. Submit And Report Immediate Status

- Call `submit_signed_transaction` through `node_cli_client.py`.
- Use one confirmation-depth target for the whole transfer. The default is `min_confirmed_blocks = 2`, which means the inclusion block plus at least 1 additional observed block.
- Pass the `chain_context` from the same preparation result that produced the signed transaction.
- Pass the same `min_confirmed_blocks` value to both `submit_signed_transaction` and any direct `watch_transaction` follow-up.
- Report `txn_hash`, `submission_state`, `next_action`, and whether immediate confirmation data is already present.

### 11. Track Confirmation

- Inspect `submit_signed_transaction.next_action`.
- If `next_action = watch_transaction`, immediately call `watch_transaction`.
- If `next_action = reconcile_by_txn_hash`, reconcile by `txn_hash` through `watch_transaction` instead of blindly resubmitting.
- If `next_action = reprepare_then_resign`, discard the old signed bytes and restart from `prepare_transfer`.
- If `status_summary.confirmed = true` but top-level `confirmed = false`, report that the transaction is included but has not yet reached the requested confirmation depth.
- If submission is accepted but confirmation is still unavailable, report that the transaction is submitted but not yet confirmed. Do not present that state as final success.

### 12. Write The Audit Record

- Write a local JSONL audit record for the preflight preview, host decision, signing request lifecycle, and submit result.
- The audit trail should include request id, payload hash, backend id, timestamps, and terminal decision.
- Do not log plaintext passwords, private keys, raw signed payloads, or full signed transaction bytes.

## Safety Rules

- Do not mix `chain_context` across preparation results.
- Do not replace `raw_txn_bcs_hex` with any host-derived bytes.
- Do not infer approval from a pending request. Approval is only real when `wallet_get_request_status` returns `approved`.
- If the prepared transaction expires or the sequence number becomes stale, restart from `prepare_transfer`.
- If the user provides a human-readable non-STC token amount and decimal precision is not already known, ask for clarification instead of guessing.
- Do not treat `submission_unknown` or a missing post-submit watch result as permission to resubmit blindly.
- Do not proceed to wallet signing when the preview shows a blocking risk such as RPC unavailability, insufficient balance, or chain-context mismatch.
- If the local runtime is unavailable, stop on the runtime problem and send the user to `doctor.py` instead of switching over to the `starcoin` CLI transfer path.

## When The Environment Is Not Ready

If the chain config, daemon socket, or wallet runtime is unavailable:

1. tell the user the workflow is blocked on local runtime setup
2. ask them to run:

```bash
plugin_root="${STARCOIN_TRANSFER_PLUGIN_ROOT:-$HOME/plugins/starcoin-transfer-workflow}"
if [ ! -f "$plugin_root/scripts/doctor.py" ]; then
  plugin_root="./plugins/starcoin-transfer-workflow"
fi
python3 "$plugin_root/scripts/doctor.py"
```

3. continue only after the missing config or daemon issue is fixed
