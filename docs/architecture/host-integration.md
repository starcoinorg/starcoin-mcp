# Host Integration Model

## Purpose

This document defines the canonical orchestration model for MCP hosts that integrate both:

- `starcoin-node-mcp`
- `starmask-mcp`

The target hosts are local MCP-capable tools such as Claude Code and Codex.

## Design Goal

The host should orchestrate chain access and wallet access without collapsing their trust boundaries.

Repository-level rule:

- chain-facing tasks go to `starcoin-node-mcp`
- wallet-facing tasks go to `starmask-mcp`

## Trust Boundary

The host may coordinate both MCP servers, but it must not assume that:

- `starcoin-node-mcp` can sign
- `starmask-mcp` can query chain state

The intended boundary is:

- `starcoin-node-mcp`
  - query
  - prepare
  - simulate
  - submit signed transaction
- `starmask-mcp`
  - discover wallet instances
  - discover accounts
  - expose public keys
  - request user approval
  - return signatures or signed transactions

## Canonical Flows

### 1. Read-Only Query Flow

Use only `starcoin-node-mcp`.

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

1. Call `starmask-mcp.wallet_status`
2. Call `starmask-mcp.wallet_list_accounts`
3. If multiple wallet instances can satisfy the request, explicitly select `wallet_instance_id`
4. If simulation is desired before signing and no public key is known yet, call `starmask-mcp.wallet_get_public_key`

#### Phase B: Unsigned transaction preparation

1. Call one of:
   - `starcoin-node-mcp.prepare_transfer`
   - `starcoin-node-mcp.prepare_contract_call`
   - `starcoin-node-mcp.prepare_publish_package`
2. Pass `sender_public_key` when available
3. Inspect the returned unsigned transaction envelope

#### Phase C: Simulation completion

If preparation returned `simulation_status = skipped_missing_public_key`:

1. obtain the sender public key from `starmask-mcp`
2. call `starcoin-node-mcp.simulate_raw_transaction`

The host may require successful simulation before requesting wallet approval.

#### Phase D: Wallet approval

1. Call `starmask-mcp.wallet_request_sign_transaction`
2. Include:
   - `wallet_instance_id` when selection is explicit
   - `account_address`
   - `chain_id`
   - `raw_txn_bcs_hex`
3. Poll `starmask-mcp.wallet_get_request_status`
4. Continue until a terminal lifecycle state is reached

#### Phase E: Submission

If the wallet request is approved:

1. read `signed_txn_bcs_hex`
2. call `starcoin-node-mcp.submit_signed_transaction`
3. optionally call `starcoin-node-mcp.watch_transaction`

### 3. Message Signing Flow

Use only `starmask-mcp`.

Typical sequence:

1. `wallet_status`
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
- preserve `wallet_instance_id` selection once the user or host has chosen one
- surface approval prompts clearly to the user
- avoid automatic re-submission of rejected wallet requests
- keep chain-side and wallet-side errors separate in its reasoning

The MCP host should not:

- assume a pending request was lost just because a poll attempt failed
- create duplicate sign requests without checking the original request status
- use transaction summaries as a security source of truth instead of wallet-rendered details

## Shared Contracts

This orchestration model depends on:

- `shared/protocol/error-codes.md`
- `shared/protocol/request-lifecycle.md`
- `shared/schemas/unsigned-transaction-envelope.schema.json`
- `shared/schemas/wallet-sign-request.schema.json`
- `shared/schemas/wallet-sign-result.schema.json`
