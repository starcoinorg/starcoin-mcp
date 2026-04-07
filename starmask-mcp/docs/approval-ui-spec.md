# Starmask Approval UI Specification

## Status

This document defines the minimum approval UI behavior for the current `v1` Starmask extension
flow.

Repository status note: the in-tree `crates/starmask-mcp` adapter has been removed. The approval
UI requirements remain relevant to the extension-backed flow itself.

It is intentionally extension-specific. If future phases add local unlock prompts or local-account
approval surfaces, they should be specified in separate documents or clearly versioned follow-on
sections rather than redefining this `v1` extension UI contract.

## Purpose

This document defines the minimum approval UI behavior required for secure signing.

The extension UI is the final authority for user consent.

## UI Security Principle

The UI must render from canonical request payload bytes, not from host-provided summaries.

`display_hint` may be shown only as supporting context.

## Supported Approval Types

- transaction signing
- message signing

## Screen States

### `loading`

Used while the extension decodes the request payload and prepares a renderable model.

Rules:

- approval actions must be disabled
- request ID may be shown

### `ready`

The request is fully decoded and safe to present.

Rules:

- all primary fields must be visible
- approve and reject actions must be enabled

### `approval_in_progress`

The user clicked approve and the extension is producing the final signature.

Rules:

- inputs are locked
- duplicate approval clicks are prevented

### `approved`

The UI shows a terminal success acknowledgement.

### `rejected`

The UI shows that the request was explicitly rejected.

### `cancelled`

The host or local policy cancelled the request while the UI was open.

Rules:

- approval must be disabled immediately
- the user should see that the request is no longer actionable

### `expired`

The request TTL elapsed while the UI was open.

Rules:

- approval must be disabled

### `unsupported`

The extension cannot safely render the payload.

Rules:

- approval must not be offered
- the UI must explain that blind signing is disabled

## Transaction Approval Fields

The transaction approval screen must display:

- account address
- wallet instance label or profile hint when available
- chain id
- transaction kind
- receiver or target function
- amount where applicable
- gas price
- max gas amount or gas budget
- expiration
- human-readable function/module identifiers where possible
- raw request ID in a secondary diagnostics area

If the transaction is a package publish, the UI must show:

- package identity if derivable
- module count or size summary
- a warning that code publication changes on-chain behavior

## Message Approval Fields

The message approval screen must display:

- account address
- message format
- canonical message preview
- byte length
- client context if present
- request ID in a diagnostics area

For non-UTF8 or long payloads:

- show a safe preview
- provide an explicit way to inspect the full payload

## Secondary Context

The UI may additionally show:

- `display_hint`
- `client_context`

Rules:

1. these fields must be visually secondary
2. they must not replace canonical payload rendering

## Action Rules

### Approve

Enabled only when:

- request is in `ready`
- payload is fully decoded
- request is not cancelled or expired
- wallet remains unlocked

### Reject

Available in:

- `ready`
- `unsupported`
- `expired`
- `cancelled`

Reject should always be available unless the request is already terminal.

## Cancellation Handling

If the daemon signals `request.cancelled` while the UI is open:

1. the screen transitions to `cancelled`
2. approve becomes disabled immediately
3. any in-flight approval completion must be discarded

## Resume Handling

If a previously presented request is recovered after reconnect:

1. the extension reopens the approval screen
2. the screen shows a small recovery banner such as:
   - `Recovered pending approval request`
3. no approval action is taken automatically
4. the resumed screen continues using the active `presentation_id`

## Unsupported Payload Handling

If the extension cannot safely render the payload:

1. transition to `unsupported`
2. explain that blind signing is disabled
3. allow only reject or dismiss

## Copy and Export Rules

The first release should avoid broad export features from the approval screen.

Allowed:

- copy request ID
- inspect canonical payload details

Avoid in the first release:

- copy signed result from the approval UI
- export raw payload to external apps from the approval UI

## Accessibility and Clarity

The UI must:

- distinguish primary action from secondary action clearly
- show chain and account before approve
- avoid truncating critical values without a reveal path

## Non-Goals

This document does not define visual styling or brand-specific design tokens.

It defines the minimum interaction and information rules required for safe approval.
