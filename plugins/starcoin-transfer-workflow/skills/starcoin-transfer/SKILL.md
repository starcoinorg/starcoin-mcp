---
name: starcoin-transfer
description: Use when the user wants a user-in-the-loop Starcoin wallet workflow through an agentic host. This skill covers address creation, transfer preparation and confirmation, and local workflow audit trails.
---

# Starcoin Transfer

This skill turns an agentic host into the host-side transfer controller without depending on plugin-managed
stdio adapters.

The canonical execution path is:

- wallet-side calls through `scripts/starmaskd_client.py`
- chain-side calls through `scripts/node_cli_client.py`
- host-side sequencing and state retention in the agentic host

Use this skill when the user wants to transfer tokens, request a signature for a prepared transfer,
create a fresh wallet address before transfer, or inspect the local audit trail for those flows.

## CLI Quick Reference

Use these forms directly. Do not call `--help` first unless one of these commands fails or the
script path itself has changed.

- Client wrappers accept tool arguments either as a final inline JSON object or on stdin. Prefer
  inline JSON for one-off reads so the command is self-contained:
  `python3 ./plugins/starcoin-transfer-workflow/scripts/node_cli_client.py call get_account_overview '{"address":"<address>"}'`
- Wallet status and discovery:
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/starmaskd_client.py call wallet_list_instances`
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/starmaskd_client.py call wallet_list_accounts '{"wallet_instance_id":"<wallet-instance-id>","include_public_key":true}'`
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/starmaskd_client.py call wallet_get_public_key '{"wallet_instance_id":"<wallet-instance-id>","address":"<address>"}'`
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/starmaskd_client.py call wallet_set_account_label '{"wallet_instance_id":"<wallet-instance-id>","address":"<address>","label":"<account-name>"}'`
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/run_create_account.py --wallet-runtime-dir $HOME/.starcoin-agents/wallet-runtime --wallet-instance-id <wallet-instance-id> --account-name <account-name>`
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/starmaskd_client.py --wallet-runtime-dir $HOME/.starcoin-agents/wallet-runtime call wallet_create_account '{"client_request_id":"create-<unique-id>","wallet_instance_id":"<wallet-instance-id>"}'`
- Chain status and reads:
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/node_cli_client.py call chain_status`
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/node_cli_client.py call node_health`
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/node_cli_client.py call get_account_overview '{"address":"<address>"}'`
- Runtime checks:
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/doctor.py`
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/wallet_runtime.py status`
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/wallet_runtime.py export-account --address <account-address> --output-file <output-file>`
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/workflow_audit.py summary --path $HOME/.starcoin-agents/wallet-runtime/audit/transfer-audit.jsonl`
- End-to-end test:
  - `python3 ./plugins/starcoin-transfer-workflow/scripts/run_transfer_test.py --rpc-url http://127.0.0.1:9850 --wallet-runtime-dir $HOME/.starcoin-agents/wallet-runtime --sender <sender> --receiver <receiver> --amount 1 --amount-unit stc --vm-profile vm2_only --min-confirmed-blocks 3`

Default runtime locations for this workflow:

- wallet runtime: `$HOME/.starcoin-agents/wallet-runtime`
- wallet dir: `$HOME/.starcoin-agents/local-accounts/default`
- node config: `$HOME/.starcoin-agents/node-cli.toml`
- wallet config: `$HOME/.starcoin-agents/wallet-runtime/starmaskd.toml`

Use `wallet_runtime.py export-account --address <account-address>` only when the private key for one
specific local address must be exported. It does not copy the local account vault. Stop the wallet
runtime before exporting, and use `--password-stdin` for non-interactive runs.

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
- `run_create_account.py`
  - `--wallet-runtime-dir <dir>`
  - `--wallet-instance-id <wallet-instance-id>`
  - `--account-name <account-name>`
  - `--client-request-id <idempotency-key>`
  - `--display-hint <hint>`
  - `--ttl-seconds <seconds>`
  - `--audit-log-path <create-account-audit.jsonl>`
- `run_transfer_test.py`
  - `--rpc-url <http-rpc>`
  - `--wallet-runtime-dir <dir>` or `--wallet-dir <dir>`
  - `--sender <address> --receiver <address>`
  - `--amount <value> [--amount-unit raw|stc]`
  - `--vm-profile <auto|vm1_only|vm2_only>`
  - `--min-confirmed-blocks <n>`
  - `--token-code <vm-profile-matched-stc-or-explicit-token>`
  - `--audit-log-path <transfer-audit.jsonl>`
  - `--state-path <transfer-state.json>`

## Workflow

The current script path does not parse free-form wallet language by itself. The agentic host should resolve
the user's intent first, then use the scripts for deterministic execution.

### Audit-First Transfer Rule

- For any real transfer that can reach wallet signing or chain submission, use the audited transfer workflow by default:
  - Prefer `run_transfer_test.py` with explicit `--audit-log-path` and `--state-path`, or
  - Use `TransferController` with `WorkflowAuditLogger` and `TransferStateStore` from the bundled scripts.
- Do not manually stitch together `prepare_transfer`, `wallet_request_sign_transaction`,
  `submit_signed_transaction`, and `watch_transaction` for a real transfer unless you also write equivalent
  structured audit records and persisted transfer state before signing/submission.
- The transfer audit record must cover: resolved intent, prepared transaction summary, simulation result,
  host preview and decision, signing request id and terminal status, submission result, confirmation result,
  payload hash, backend id, timestamps, and terminal outcome.
- The transfer state file must persist the prepared payload hash and any unresolved submission `txn_hash` so
  a retry can reconcile before resubmitting.
- Low-level direct tool calls are acceptable for read-only discovery, diagnostics, previews that stop before
  signing, or recovery queries, but not as the normal path for signed/submitted transfers.

### 0. Decide Which Wallet Flow Is Needed

- If the user wants a new local address, start with the create-account flow before any transfer preparation.
- If the user wants to inspect what happened in a prior create-account or transfer run, read the local JSONL audit file and summarize it instead of replaying the workflow.
- Only start chain-side transfer preparation after the sender, receiver, amount, token, and wallet instance are unambiguous.

### 1. Create A Fresh Address When Needed

- Discover wallet instances first with `wallet_list_instances`.
- If there is exactly one viable wallet instance, you may auto-select it. Otherwise ask one precise follow-up question with the concrete candidates.
- Prefer `run_create_account.py` for a user-facing guided flow. It creates the request, waits for approval, and writes a local audit record.
- Local account labels come from the daemon-side metadata layer instead of Starcoin account storage. If a local address has no custom name yet, `wallet_list_accounts` assigns and returns `account-1`, `account-2`, and so on in first-seen order.
- If you need lower-level control, call `wallet_create_account` through `starmaskd_client.py`, then poll `wallet_get_request_status`.
- Do not claim the address exists until the request reaches `approved` and `result.address` is present.
- Report the created address, whether it is default, and where the approval happened.

### 2. Capture The Transfer Intent

- Extract or confirm: network, sender account, receiver address, token code, amount, and wallet instance.
- If a field is missing, ask one precise follow-up question instead of a broad prompt.
- If routing is ambiguous, list the concrete candidates from `wallet_list_instances` and `wallet_list_accounts`.
- Do not start chain-side preparation until the transfer intent is unambiguous.

### 3. Validate The Resolved Inputs

- Check that sender and receiver look like valid Starcoin addresses before preparation.
- Check that the selected token code is explicit and consistent with the chosen `vm_profile`.
- `vm_profile` is RPC routing, not per-account VM detection.
- Do not automatically switch between `0x1::STC::STC` and `0x1::starcoin_coin::STC`.
- If the user gives a human-readable non-STC amount and decimals are not already known from trusted metadata, ask instead of guessing.

### 4. Gather Chain And Wallet Context

- If the runtime might not be ready, check it with `python3 ./plugins/starcoin-transfer-workflow/scripts/wallet_runtime.py status` or `python3 ./plugins/starcoin-transfer-workflow/scripts/doctor.py`.
- If the `starmaskd` socket is missing or the wallet daemon is not running, stop the transfer flow and ask the user to start the wallet runtime manually. Do not run `wallet_runtime.py up --replace` on the user's behalf.
- Give the user the manual startup command, usually `python3 ./plugins/starcoin-transfer-workflow/scripts/wallet_runtime.py up --replace`, and explain that the transfer can continue after the socket is available.
- Discover wallet candidates with `wallet_list_instances` and `wallet_list_accounts`.
- Inspect chain identity with `chain_status`.
- Check RPC availability, peer count, and lag warnings through `node_health`.
- If the sender public key is not already known, call `wallet_get_public_key`.

### 5. Prepare The Transaction

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

### 6. Run Host Preflight

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
- Treat sequence / nonce moving ahead after preparation as a blocking risk. Prepare again before signing.

### 7. Show The Transaction Preview

- Summarize the prepared transaction in the agentic host before creating a signing request.
- Include network, sender, receiver, token, amount, raw amount, nonce, fee estimate, balance, and simulation outcome.
- Show the generated risk labels separately from the happy-path preview.
- If any blocking risk is present, stop before wallet signing.

### 8. Require Host Confirmation

- Ask for explicit confirmation after the preview and risk labels are shown.
- Do not ask the wallet to sign until the user has clearly confirmed the prepared transfer.
- Record the host preview and the user's explicit decision in the transfer audit log before creating the signing request.

### 9. Create The Signing Request

- Call `wallet_request_sign_transaction` through `starmaskd_client.py` only after host confirmation.
- Copy `raw_txn_bcs_hex` directly from `prepare_transfer.raw_txn_bcs_hex`.
- Copy `tx_kind` directly from `prepare_transfer.transaction_kind`.
- `display_hint` may be derived from the chain-side summary, but it is only supportive context.
- Tell the user where approval will happen.
  - For `local_account_dir`, approval appears in the CLI approval card.
- Record the signing request id, wallet instance id, payload hash, and request status in the transfer audit log.

### 10. Wait For Wallet Approval

- Poll `wallet_get_request_status` through `starmaskd_client.py`.
- Stop on terminal failure states:
  - `rejected`
  - `cancelled`
  - `expired`
  - `failed`
- When the request becomes `approved`, extract `result.signed_txn_bcs_hex`.
- Record the terminal signing status in the transfer audit log. Do not log full signed transaction bytes.

### 11. Submit And Report Immediate Status

- Call `submit_signed_transaction` through `node_cli_client.py`.
- Use one confirmation-depth target for the whole transfer. The default is `min_confirmed_blocks = 2`, which means the inclusion block plus at least 1 additional observed block.
- Pass the `chain_context` from the same preparation result that produced the signed transaction.
- Pass the same `min_confirmed_blocks` value to both `submit_signed_transaction` and any direct `watch_transaction` follow-up.
- Use the persisted transfer state next to the audit log to verify the prepared payload hash before submission.
- Report `txn_hash`, `submission_state`, `next_action`, and whether immediate confirmation data is already present.
- Record the submit result and any unresolved submission state before returning control to the user.

### 12. Track Confirmation

- Inspect `submit_signed_transaction.next_action`.
- If `next_action = watch_transaction`, immediately call `watch_transaction`.
- If `next_action = reconcile_by_txn_hash`, reconcile by `txn_hash` through `watch_transaction` instead of blindly resubmitting.
- If the persisted transfer state already has an unresolved submission for the prepared payload, reconcile that `txn_hash` before any new submit attempt.
- If `next_action = reprepare_then_resign`, discard the old signed bytes and restart from `prepare_transfer`.
- If `status_summary.confirmed = true` but top-level `confirmed = false`, report that the transaction is included but has not yet reached the requested confirmation depth.
- If submission is accepted but confirmation is still unavailable, report that the transaction is submitted but not yet confirmed. Do not present that state as final success.
- Record the final watch or reconciliation outcome in the transfer audit log.

### 13. Write Or Inspect The Audit Record

- `run_create_account.py` writes create-account audit records under `$HOME/.starcoin-agents/wallet-runtime/audit/create-account-audit.jsonl` by default.
- `run_transfer_test.py` writes transfer audit records under the active runtime's `audit/transfer-audit.jsonl` by default.
- `run_transfer_test.py` writes transfer state under the active runtime's `audit/transfer-state.json` by default.
- Write a local JSONL audit record for the resolved intent, preflight preview, host decision, signing request lifecycle, submit result, and confirmation result.
- The audit trail should include request id, payload hash, backend id, timestamps, and terminal decision.
- Do not log plaintext passwords, private keys, raw signed payloads, or full signed transaction bytes.
- If a real transfer completed without these audit records, say so explicitly and reconstruct a minimal audit note from available facts rather than claiming full audit coverage.
- When reading an existing audit file for the user, prefer `workflow_audit.py summary`; summarize request id, payload hash, backend id, txn hash, terminal status, and timestamps. Do not dump the whole file unless the user explicitly asks.

## Safety Rules

- Do not mix `chain_context` across preparation results.
- Do not replace `raw_txn_bcs_hex` with any host-derived bytes.
- Do not infer approval from a pending request. Approval is only real when `wallet_get_request_status` returns `approved`.
- If the prepared transaction expires or the sequence number becomes stale, restart from `prepare_transfer`.
- If the user provides a human-readable non-STC token amount and decimal precision is not already known, ask for clarification instead of guessing.
- Do not treat `submission_unknown` or a missing post-submit watch result as permission to resubmit blindly.
- When a persisted unresolved submission exists for the prepared payload, reconcile by the persisted `txn_hash` before any new submit.
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
