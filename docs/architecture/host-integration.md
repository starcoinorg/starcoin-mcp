# Host Integration Model

## Purpose

This document defines the canonical orchestration model for local hosts that integrate both:

- `starcoin-node`
- `starmask-runtime`

The target hosts are local host-capable tools such as Claude Code and Codex.

## Design Goal

The host should orchestrate chain access and wallet access without collapsing their trust boundaries.

Repository-level rule:

- chain-facing tasks go to `starcoin-node`
- wallet-facing tasks go to `starmask-runtime`

## Trust Boundary

The host may coordinate both adapter boundaries, but it must not assume that:

- `starcoin-node` can sign
- `starmask-runtime` can query chain state

The intended boundary is:

- `starcoin-node`
  - query
  - prepare
  - simulate
  - submit signed transaction
- `starmask-runtime`
  - discover wallet instances
  - discover accounts
  - expose public keys
  - request user approval
  - return signatures or signed transactions

## Canonical Flows

### 1. Read-Only Query Flow

Use only `starcoin-node`.

Typical sequence:

1. `chain_status`
2. `get_account_overview`
3. `resolve_function_abi`
4. `call_view_function`
5. `get_transaction`

No wallet interaction is needed.

### 2. Signed Transaction Flow

This is the canonical cross-project transaction flow.

#### Phase A: Wallet discovery

1. Call `starmask-runtime.wallet_status`
2. Call `starmask-runtime.wallet_list_accounts`
3. If multiple wallet instances can satisfy the request, explicitly select `wallet_instance_id`
4. If simulation is desired before signing and no public key is known yet, call `starmask-runtime.wallet_get_public_key`

#### Phase B: Unsigned transaction preparation

1. Call one of:
   - `starcoin-node.prepare_transfer`
   - `starcoin-node.prepare_contract_call`
   - `starcoin-node.prepare_publish_package`
2. Pass `sender_public_key` when available
3. Inspect the returned unsigned transaction envelope, especially `chain_context`, `prepared_at`, and any freshness metadata

#### Phase C: Simulation completion

If preparation returned `simulation_status = skipped_missing_public_key`:

1. obtain the sender public key from `starmask-runtime`
2. call `starcoin-node.simulate_raw_transaction`

The host may require successful simulation before requesting wallet approval.

#### Phase D: Wallet approval

1. Call `starmask-runtime.wallet_request_sign_transaction`
2. Include:
   - `client_request_id`
   - `wallet_instance_id` when selection is explicit
   - `account_address`
   - `chain_id`
   - `raw_txn_bcs_hex`
3. The first release expects the selected wallet instance to be connected and unlocked before request creation succeeds
4. Poll `starmask-runtime.wallet_get_request_status`
5. Continue until a terminal lifecycle state is reached

#### Phase E: Submission

If the wallet request is approved:

1. read `signed_txn_bcs_hex` and retain the `chain_context` from the earlier preparation result
2. call `starcoin-node.submit_signed_transaction`
   Pass both `signed_txn_bcs_hex` and the previously prepared `chain_context` so the node-side server can reject chain drift before txpool contact.
   If node-side policy requires prior simulation, make sure the exact raw transaction had already been prepared or simulated through the same node-side server instance before submission.
3. if the chain-side call is rejected locally with `rate_limited`, back off and retry the same submission step without changing the signed bytes
4. if `submission_state = accepted`, call `starcoin-node.watch_transaction` or rely on the blocking submit convenience path with the same `min_confirmed_blocks` target
5. if `submission_state = unknown`, reconcile by `txn_hash` through `get_transaction` or `watch_transaction` before any retry
6. if the chain-side error is `transaction_expired` or `sequence_number_stale`, restart from Phase B with fresh preparation and then request fresh wallet approval
7. if reconciliation remains unresolved after timeout, persist the unresolved submission state and surface it to the user instead of blind re-submission

Recommended transfer confirmation policy:

- default to `min_confirmed_blocks = 2`
- interpret that as the inclusion block plus at least 1 additional observed block
- treat `status_summary.confirmed = true` with top-level `confirmed = false` as an intermediate "included but not yet deep enough" state

### 3. Message Signing Flow

Use only `starmask-runtime`.

Typical sequence:

1. `wallet_status`
2. `wallet_list_accounts`
3. select `wallet_instance_id` if needed
4. `wallet_sign_message`
   - include `client_request_id`
5. poll `wallet_get_request_status`
6. retrieve `signature` after approval

### 4. Recovery Flow

The host should treat wallet approval as asynchronous and failure-prone.

If the host is interrupted:

- persist `request_id`
- resume by calling `wallet_get_request_status`

If the wallet restarts:

- continue polling the same `request_id`
- do not create a duplicate request unless the original request reaches a terminal state

If `wallet_selection_required` is returned:

- re-run wallet discovery
- select a concrete `wallet_instance_id`
- retry the wallet-facing request

## Host Responsibilities

The MCP host should:

- preserve `request_id` values across retries where possible
- preserve `client_request_id` when retrying create calls after uncertain failures
- preserve `wallet_instance_id` selection once the user or host has chosen one
- preserve chain-side `chain_context` and `txn_hash` metadata across retries and reconciliation
- persist unresolved `submission_unknown` states across host interruptions where practical
- inspect `effective_*` or `applied_*` fields when the chain-side server clamps watch or query inputs to local policy bounds
- surface approval prompts clearly to the user
- avoid automatic re-submission of rejected wallet requests
- keep chain-side and wallet-side errors separate in its reasoning
- back off on chain-side `rate_limited` responses instead of tight-loop retrying

The MCP host should not:

- assume a pending request was lost just because a poll attempt failed
- create duplicate sign requests without checking the original request status
- blindly re-submit a signed transaction when the previous submission result is `submission_unknown`
- use transaction summaries as a security source of truth instead of wallet-rendered details

## Shared Contracts

This orchestration model depends on:

- `shared/protocol/error-codes.md`
- `shared/protocol/request-lifecycle.md`
- `shared/schemas/unsigned-transaction-envelope.schema.json`
- `shared/schemas/wallet-sign-request.schema.json`
- `shared/schemas/wallet-sign-result.schema.json`
