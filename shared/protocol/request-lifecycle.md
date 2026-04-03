# Shared Request Lifecycle

## Purpose

This document defines a shared lifecycle vocabulary for asynchronous approval and signing requests.

It is intended to be reused by wallet-facing MCP projects such as `starmask-runtime`.

## Canonical States

- `created`
  - The request has been accepted, persisted locally, and is waiting for wallet delivery.
- `dispatched`
  - The request has been claimed by a wallet-facing runtime under a delivery lease.
- `pending_user_approval`
  - The wallet has presented the request and is waiting for explicit user approval.
- `approved`
  - The request has been approved and a result is available.
- `rejected`
  - The request has been rejected by the user.
- `expired`
  - The request exceeded its TTL and is invalid.
- `cancelled`
  - The request was cancelled by the caller or local policy.
- `failed`
  - The request could not complete due to an internal failure.

## State Transition Rules

Typical valid transitions:

- `created -> dispatched`
- `dispatched -> pending_user_approval`
- `dispatched -> created`
- `pending_user_approval -> approved`
- `pending_user_approval -> rejected`
- `pending_user_approval -> expired`
- `pending_user_approval -> created`
- `created -> cancelled`
- `dispatched -> cancelled`
- `pending_user_approval -> cancelled`
- any non-terminal operational state -> failed

Terminal states:

- `approved`
- `rejected`
- `expired`
- `cancelled`
- `failed`

## Result Availability

- `approved`
  - A signing result or signed transaction is available.
- `rejected`
  - A structured rejection reason may be available.
- `expired`
  - No result is available.
- `cancelled`
  - No result is available.
- `failed`
  - A failure reason should be available where possible.

## Polling Guidance

When a tool initiates an asynchronous request, the initial result should:

- return a `request_id`
- return a lifecycle `status`
- indicate that the host should poll until a terminal state is reached

## Delivery and Recovery Guidance

The canonical delivery model is:

1. the caller creates a request
2. the local daemon persists it in `created`
3. a wallet runtime claims it and the daemon moves it to `dispatched`
4. once the wallet shows approval UI, the daemon moves it to `pending_user_approval`
5. the request eventually reaches a terminal state

If a wallet runtime disconnects or loses its lease before presenting approval UI, the request should return to `created`.

If a wallet runtime disconnects after presenting approval UI but before final resolution, the request remains `pending_user_approval` and may be resumed only by the same wallet instance according to local recovery policy.

## Result Retention Guidance

Approved results may remain readable for a bounded retention window.

After the retention window:

- the request remains terminal
- result payload bytes may be evicted
- polling should still return terminal metadata

## TTL Guidance

Every asynchronous approval request should include:

- `created_at`
- `expires_at`

Hosts should treat `expired` as terminal and not retry the same request id.
