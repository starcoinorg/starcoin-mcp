# Host Integration Model

## Purpose

This document defines the canonical orchestration model for local hosts that integrate both the
logical chain boundary `starcoin-node` and the logical wallet boundary `starmask-runtime`.

Status note:

- these names still define the host-facing logical boundaries
- the current repository realizes those boundaries through `starcoin-node-cli`, `starmaskd`,
  backend agents, and future or external adapter processes
- the repository no longer ships in-tree stdio adapters for either side

The target hosts are local tools such as Codex, Claude Code, and repository-specific workflow
scripts.

## Design Goal

The host should orchestrate chain access and wallet access without collapsing their trust
boundaries.

Repository-level rule:

- chain-facing tasks go to `starcoin-node`
- wallet-facing tasks go to `starmask-runtime`
- process supervision belongs to a TUI or operator tool, not to the transaction-orchestration host

## Current Repository Realization

The current workspace maps the logical boundaries as follows:

- logical `starcoin-node`
  - current executable: `starcoin-node-cli`
  - current transport shape: one-shot CLI command per request
- logical `starmask-runtime`
  - current runtime core: `starmaskd`
  - current signing backends:
    - Starmask extension through `starmask-native-host`
    - `local_account_dir` through `local-account-agent`
  - current host integrations:
    - future or external adapter processes
    - direct daemon clients used by repository-local scripts

This document therefore defines the logical tool flow and the required data handoff, not a claim
that those logical surfaces are all compiled into the current repository as stdio servers.

## Trust Boundary

The host may coordinate both boundaries, but it must not assume that:

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

Approval surfaces are backend-specific:

- browser UI for extension-backed wallets
- local `tty_prompt` UI for the current `local_account_dir` backend

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

1. Call `wallet_list_instances`
2. Call `wallet_list_accounts`
3. If multiple wallet instances can satisfy the request, explicitly select `wallet_instance_id`
4. If simulation is desired before signing and no public key is known yet, call
   `wallet_get_public_key`

#### Phase B: Unsigned transaction preparation

1. Call one of:
   - `prepare_transfer`
   - `prepare_contract_call`
   - `prepare_publish_package`
2. Pass `sender_public_key` when available
3. Retain the returned `raw_txn_bcs_hex`, `chain_context`, `simulation_status`, and any freshness
   metadata

#### Phase C: Simulation completion

If preparation returned `simulation_status = skipped_missing_public_key`:

1. obtain the sender public key from the wallet side
2. rerun `prepare_transfer` or call `simulate_raw_transaction`
3. continue only with the fresh preparation result

#### Phase D: Wallet approval

1. Call `wallet_request_sign_transaction`
2. Include:
   - `client_request_id`
   - `wallet_instance_id` when selection is explicit
   - `account_address`
   - `chain_id`
   - `raw_txn_bcs_hex`
3. Poll `wallet_get_request_status`
4. Continue until a terminal lifecycle state is reached

The host must treat the approval surface as backend-owned:

- extension backends approve in browser UI
- `local_account_dir` approves in a local TTY prompt

#### Phase E: Submission

If the wallet request is approved:

1. read `signed_txn_bcs_hex`
2. retain the exact `chain_context` returned by the preparation result that produced the signed
   bytes
3. call `submit_signed_transaction`
4. if `submission_state = accepted`, optionally follow with `watch_transaction`
5. if `submission_state = unknown`, reconcile by `txn_hash` before any retry
6. if the chain-side error is `transaction_expired` or `sequence_number_stale`, restart from
   preparation and request a fresh signature

Recommended transfer confirmation policy:

- default to `min_confirmed_blocks = 2`
- interpret that as the inclusion block plus at least 1 additional observed block
- treat `status_summary.confirmed = true` with top-level `confirmed = false` as an intermediate
  "included but not yet deep enough" state

### 3. Message Signing Flow

Use only `starmask-runtime`.

Typical sequence:

1. `wallet_list_instances`
2. `wallet_list_accounts`
3. select `wallet_instance_id` if needed
4. `wallet_sign_message`
5. poll `wallet_get_request_status`
6. retrieve `signature` after approval

### 4. Recovery Flow

The host should treat wallet approval as asynchronous and failure-prone.

If the host is interrupted:

- persist `request_id`
- resume by calling `wallet_get_request_status`

If the wallet helper process restarts:

- continue polling the same `request_id`
- do not create a duplicate request unless the original request reaches a terminal state

If `wallet_selection_required` is returned:

- re-run wallet discovery
- select a concrete `wallet_instance_id`
- retry the wallet-facing request

## Host Responsibilities

The host should:

- preserve `request_id` values across retries where possible
- preserve `client_request_id` when retrying create calls after uncertain failures
- preserve `wallet_instance_id` selection once it has been chosen
- preserve chain-side `chain_context` and `txn_hash` metadata across retries and reconciliation
- keep chain-side and wallet-side errors separate in its reasoning
- avoid automatic re-submission of rejected or uncertain wallet requests

The host should not:

- assume a pending request was lost because one poll attempt failed
- create duplicate sign requests without checking the original request status
- blindly re-submit a signed transaction when the previous submission result is
  `submission_unknown`
- treat process supervision as part of the signing or submission boundary

## TUI and Supervisor Boundary

An operator-facing TUI or runtime supervisor may:

- start and stop `starmaskd`
- start and stop local backend agents
- optionally start one node-side service

It must not:

- sign transactions
- submit transactions in place of `starcoin-node`
- replace the host's responsibility for cross-boundary request orchestration

## Shared Contracts

This orchestration model depends on:

- `shared/protocol/error-codes.md`
- `shared/protocol/request-lifecycle.md`
- `shared/schemas/unsigned-transaction-envelope.schema.json`
- `shared/schemas/wallet-sign-request.schema.json`
- `shared/schemas/wallet-sign-result.schema.json`
- `starcoin-node/docs/architecture/host-integration.md`
- `starmask-runtime/docs/starmask-interface-design.md`
