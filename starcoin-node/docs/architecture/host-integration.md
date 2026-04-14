# Host Integration

## Purpose

This document defines the repository-level host orchestration for one user-in-the-loop
transaction flow.

The canonical first flow is:

1. prepare an unsigned transfer with `starcoin-node`
2. request wallet approval and signing with `starmask-runtime`
3. submit the signed transaction with `starcoin-node`
4. optionally watch the transaction to terminal status

This document is host-facing. It describes what an MCP host such as Codex should do, in what
order, and which outputs from one server become inputs to another.

## Actors

- `MCP host`
  - orchestrates tool calls and retry behavior
- `starcoin-node`
  - prepares, simulates, submits, and watches transactions
- `starmask-runtime`
  - routes signing requests to the selected wallet instance
- `wallet approval surface`
  - browser UI for extension-backed wallets
  - CLI `tty_prompt` card for `local_account_dir`
- `Starcoin RPC endpoint`
  - receives the submitted signed transaction

## Trust Boundaries

1. `starcoin-node` may build unsigned transactions and describe them, but it must not sign.
2. `starmask-runtime` may request signing, but it must not submit signed bytes to the chain.
3. The wallet approval surface is the final user consent point for signing.
4. The host may use chain-side summaries for UX, but the wallet must still render canonical payload
   bytes before approval.

## Flow Summary

### Phase A: Select Chain And Wallet Context

The host should gather:

- sender account address
- receiver address
- amount
- token code if not default STC
- target wallet instance when routing is ambiguous
- sender public key when available

Recommended tool calls:

- `starcoin-node.chain_status`
- `starmask-runtime.wallet_list_instances`
- `starmask-runtime.wallet_list_accounts`
- `starmask-runtime.wallet_get_public_key`

If any field is missing or ambiguous, the host should ask a narrow follow-up question or list the
available wallet candidates rather than starting preparation from partial intent.

If `wallet_get_public_key` is unavailable for the selected account, the host may continue, but it
must expect `prepare_transfer` to return `simulation_status = skipped_missing_public_key`.

### Phase B: Prepare Unsigned Transfer

The host prepares the unsigned transaction through `starcoin-node.prepare_transfer`.

Example input:

```json
{
  "sender": "0x1...",
  "sender_public_key": "0x02...",
  "receiver": "0xabcd...",
  "amount": "1000000",
  "token_code": "0x1::STC::STC"
}
```

The host must retain at least these fields from the preparation result:

- `transaction_kind`
- `raw_txn_bcs_hex`
- `chain_context`
- `simulation_status`
- `simulation`
- `execution_facts`
- `next_action`

If preparation fails with `invalid_address`, `invalid_asset`, `invalid_amount`,
`invalid_chain_context`, `simulation_failed`, or `rpc_unavailable`, the host must not create a
wallet signing request from stale or partial data.

### Phase B.5: Preflight, Risk, And Preview

Before requesting wallet signing, the host should run a read-only preflight pass around the
prepared transaction.

Recommended tool calls:

- `starcoin-node.chain_status`
- `starcoin-node.node_health`
- `starcoin-node.get_account_overview` for the sender
- `starcoin-node.get_account_overview` for the receiver

The host should derive or check:

- sender balance for the selected token
- gas-token balance when it differs from the transferred token
- prepared sequence number versus `next_sequence_number_hint`
- fee estimate from `prepare_transfer.execution_facts` and `prepare_transfer.simulation.gas_used`
- latest chain identity versus `prepare_transfer.chain_context`

The host should then:

- generate structured risk labels
- show a transaction preview with network, sender, receiver, token, amount, raw amount, nonce, fee estimate, balance, and simulation outcome
- stop before wallet signing if any blocking risk is present

### Phase C: Request Wallet Signing

The host sends the prepared raw transaction to `starmask-runtime.wallet_request_sign_transaction`.

Example input:

```json
{
  "client_request_id": "codex-transfer-001",
  "account_address": "0x1...",
  "wallet_instance_id": "local-default",
  "chain_id": 251,
  "raw_txn_bcs_hex": "0x...",
  "tx_kind": "transfer",
  "display_hint": "Transfer 1 STC to 0xabcd...",
  "client_context": "codex",
  "ttl_seconds": 300
}
```

The host should derive these fields directly from Phase B when available:

- `raw_txn_bcs_hex` from `prepare_transfer.raw_txn_bcs_hex`
- `tx_kind` from `prepare_transfer.transaction_kind`

The host may derive `display_hint` from the chain-side summary, but that hint is supportive
context only. It is not the source of truth for wallet approval.

### Phase D: Wait For User Approval

The host polls `starmask-runtime.wallet_get_request_status` until the request becomes terminal.

Terminal outcomes:

- `approved`
- `rejected`
- `cancelled`
- `expired`
- `failed`

For the `local_account_dir` backend, the wallet approval surface is a CLI card with explicit
actions:

- `approve`
- `reject`
- `view raw canonical payload`

The wallet approval surface must render canonical payload-derived fields before approval. The host
must not infer approval from transport loss or from the mere existence of a pending request.

When the request reaches `approved`, the host extracts:

- `result.signed_txn_bcs_hex`

If the request reaches `rejected`, `cancelled`, or `expired`, the host must stop the current flow.
It may start over only by reusing Phase B or preparing a fresh transaction, depending on the
failure reason.

### Phase E: Submit Signed Transaction

After approval, the host submits the signed bytes through
`starcoin-node.submit_signed_transaction`.

Example input:

```json
{
  "signed_txn_bcs_hex": "0x...",
  "prepared_chain_context": {
    "...": "use the chain_context from prepare_transfer"
  },
  "blocking": false,
  "min_confirmed_blocks": 2
}
```

The host must pass the `chain_context` returned by the same preparation result that produced the
signed transaction. It must not mix a signed transaction from one preparation result with the chain
context of another.

Expected output fields:

- `txn_hash`
- `submission_state`
- `submitted`
- `prepared_simulation_status`
- `error_code`
- `next_action`

If `next_action = watch_transaction`, the host should immediately follow with
`starcoin-node.watch_transaction`.

Recommended transfer confirmation policy:

- default to `min_confirmed_blocks = 2`
- interpret that as the inclusion block plus at least 1 additional observed block
- treat `status_summary.confirmed = true` with top-level `confirmed = false` as "included but not yet deep enough"

## Local Audit Guidance

The host should write a minimal local audit trail for transfer execution.

Recommended fields:

- request id
- payload hash
- backend or wallet instance id
- timestamps for preview, request creation, request terminal status, and submission
- terminal decision or final submit state

The audit trail must not log plaintext passwords, private keys, raw signed payloads, or full signed
transaction bytes.

## Retry And Recovery Rules

### Preparation returned `simulation_status = skipped_missing_public_key`

The host should:

1. call `starmask-runtime.wallet_get_public_key`
2. rerun `starcoin-node.prepare_transfer` with `sender_public_key`
3. continue only with the fresh preparation result

### Wallet request expired before approval

The host should:

1. discard the old `request_id`
2. check whether the prepared transaction is still fresh enough to sign
3. if freshness is uncertain, rerun `prepare_transfer`
4. create a new wallet signing request

### Submission failed with `transaction_expired` or `sequence_number_stale`

The host should:

1. discard the old signed bytes
2. rerun `prepare_transfer`
3. request a fresh wallet signature
4. submit again only with the newly signed transaction

### Submission returned `submission_unknown`

The host must not blindly resubmit. It should:

1. reconcile by `txn_hash`
2. watch or query transaction status
3. resubmit only if reconciliation proves the prior submission did not land

## Data Handoff Contract

The host must preserve these cross-server bindings:

- `prepare_transfer.raw_txn_bcs_hex` -> `wallet_request_sign_transaction.raw_txn_bcs_hex`
- `prepare_transfer.transaction_kind` -> `wallet_request_sign_transaction.tx_kind`
- `wallet_get_request_status.result.signed_txn_bcs_hex` -> `submit_signed_transaction.signed_txn_bcs_hex`
- `prepare_transfer.chain_context` -> `submit_signed_transaction.prepared_chain_context`

If any of these bindings break, the host must restart from Phase B instead of trying to repair the
flow in place.

## Minimal Happy Path

1. `wallet_list_accounts`
2. optional `wallet_get_public_key`
3. `prepare_transfer`
4. `wallet_request_sign_transaction`
5. poll `wallet_get_request_status`
6. extract `signed_txn_bcs_hex`
7. `submit_signed_transaction`
8. optional `watch_transaction`

This is the recommended first end-to-end transfer flow for Codex and other MCP hosts.
