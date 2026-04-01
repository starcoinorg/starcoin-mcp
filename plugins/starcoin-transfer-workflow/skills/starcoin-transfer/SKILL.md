---
name: starcoin-transfer
description: Use when the user wants a user-in-the-loop Starcoin transfer through Codex. This skill orchestrates starcoin-node-mcp and starmask-mcp while keeping prepare, confirm, sign, submit, and watch steps bound together.
---

# Starcoin Transfer

This skill turns Codex into the host-side transfer controller.

The plugin already registers these MCP servers:

- `starcoin-node-mcp`
- `starmask-mcp`

Use this skill when the user wants to transfer tokens, request a signature for a prepared transfer, or track the final transaction state.

## Workflow

### 1. Gather Context

- Call `starmask-mcp.wallet_list_instances`.
- Call `starmask-mcp.wallet_list_accounts`.
- Call `starcoin-node-mcp.chain_status`.
- If the sender public key is not already known, call `starmask-mcp.wallet_get_public_key`.
- If sender, receiver, amount, token, or wallet instance are ambiguous, ask before preparing a transaction.
- Treat `starcoin-node-mcp` and `starmask-mcp` as the only valid transfer execution path. Do not fall back to the `starcoin` CLI or ad hoc shell commands for balance lookup, transfer preparation, submission, or transaction watching when the MCP servers are available.

## 2. Prepare The Transaction

- `starcoin-node-mcp.prepare_transfer.amount` expects the raw on-chain integer amount.
- If the user gives a human-readable STC amount and `token_code` is omitted or equals `0x1::STC::STC`, normalize it with 9 decimals before preparation. `1 STC = 1_000_000_000` raw units.
- For non-STC assets, only normalize a human-readable amount when decimals are already known from trusted chain metadata or prior explicit context. Otherwise ask instead of guessing.
- Call `starcoin-node-mcp.prepare_transfer`.
- Retain these fields from the result:
  - `transaction_kind`
  - `raw_txn_bcs_hex`
  - `chain_context`
  - `transaction_summary`
  - `simulation_status`
  - `simulation`
  - `next_action`
- If preparation fails with `simulation_failed`, `invalid_chain_context`, or `rpc_unavailable`, stop and explain the failure instead of creating a signing request.

## 3. Require Host Confirmation

- Summarize the prepared transaction in Codex before creating a signing request.
- Include network, sender, receiver, token, amount, and simulation outcome.
- Ask for explicit confirmation before calling `wallet_request_sign_transaction`.
- Do not ask the wallet to sign until the user has clearly confirmed the prepared transfer.

## 4. Create The Signing Request

- Call `starmask-mcp.wallet_request_sign_transaction` only after host confirmation.
- Copy `raw_txn_bcs_hex` directly from `prepare_transfer.raw_txn_bcs_hex`.
- Copy `tx_kind` directly from `prepare_transfer.transaction_kind`.
- `display_hint` may be derived from the chain-side summary, but it is only supportive context.
- Tell the user where approval will happen.
  - For `local_account_dir`, approval appears in the CLI approval card.

## 5. Wait For Wallet Approval

- Poll `starmask-mcp.wallet_get_request_status`.
- Stop on terminal failure states:
  - `rejected`
  - `cancelled`
  - `expired`
  - `failed`
- When the request becomes `approved`, extract `result.signed_txn_bcs_hex`.

## 6. Submit And Watch

- Call `starcoin-node-mcp.submit_signed_transaction`.
- Pass the `chain_context` from the same preparation result that produced the signed transaction.
- Inspect `submit_signed_transaction.next_action`.
- If `submit_signed_transaction.next_action = watch_transaction`, immediately call `starcoin-node-mcp.watch_transaction`.
- If `submit_signed_transaction.next_action = reconcile_by_txn_hash`, reconcile by `txn_hash` through `starcoin-node-mcp.watch_transaction` instead of blindly resubmitting.
- If `submit_signed_transaction.next_action = reprepare_then_resign`, discard the old signed bytes and restart from `prepare_transfer`.
- If submission is accepted but confirmation is still unavailable, report that the transaction is submitted but not yet confirmed. Do not present that state as final success.

## Safety Rules

- Do not mix `chain_context` across preparation results.
- Do not replace `raw_txn_bcs_hex` with any host-derived bytes.
- Do not infer approval from a pending request. Approval is only real when `wallet_get_request_status` returns `approved`.
- If the prepared transaction expires or the sequence number becomes stale, restart from `prepare_transfer`.
- If the user provides a human-readable non-STC token amount and decimal precision is not already known, ask for clarification instead of guessing.
- Do not treat `submission_unknown` or a missing post-submit watch result as permission to resubmit blindly.
- If the MCP runtime is unavailable, stop on the runtime problem and send the user to `doctor.py` instead of switching over to the `starcoin` CLI transfer path.

## When The Environment Is Not Ready

If either MCP server is unavailable or the wallet daemon is not reachable:

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
