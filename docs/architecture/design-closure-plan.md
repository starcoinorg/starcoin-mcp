# Starmask MCP Design Closure Plan

## Purpose

This document defines how `starmask-mcp` design work should be completed before implementation starts.

The goal is to prevent a partial implementation from hard-coding assumptions that are still unsettled at the protocol, recovery, or security layer.

## Design Completion Standard

`starmask-mcp` design is considered ready for implementation only when all of the following are true:

1. Every runtime component has a documented responsibility and lifecycle.
2. Every cross-process boundary has a versioned protocol contract.
3. Every asynchronous request has a closed lifecycle from creation to terminal cleanup.
4. Every restart and disconnect path has a documented recovery rule.
5. Every security-sensitive action has a clear approval and trust-boundary rule.
6. Every required local artifact, path, and configuration item is documented.
7. The first implementation scope is explicitly frozen.

## Required Document Set

The following documents should exist before implementation starts:

1. `docs/architecture/host-integration.md`
2. `docs/architecture/deployment-model.md`
3. `starmask-mcp/docs/starmask-mcp-interface-design.md`
4. `starmask-mcp/docs/security-model.md`
5. `starmask-mcp/docs/daemon-protocol.md`
6. `starmask-mcp/docs/native-messaging-contract.md`
7. `starmask-mcp/docs/persistence-and-recovery.md`
8. `starmask-mcp/docs/configuration.md`
9. `starmask-mcp/docs/approval-ui-spec.md`
10. `starmask-mcp/docs/testing-and-acceptance.md`

## Design Order

The documents should be completed in this order:

1. repository-level orchestration and deployment
2. security model
3. daemon protocol
4. native messaging and extension contract
5. persistence and recovery
6. configuration and installation detail
7. approval UI specification
8. testing and acceptance criteria

This order keeps later documents constrained by earlier ones instead of re-opening top-level assumptions.

## Closed-Loop Flow Requirement

Each design flow must be closed from start to finish.

At minimum, the repository must define complete flows for:

1. wallet online discovery
2. account discovery
3. public key retrieval
4. transaction signing
5. message signing
6. request cancellation
7. request expiry
8. wallet disconnect before presentation
9. wallet disconnect after presentation
10. daemon restart with non-terminal requests
11. host restart with persisted `request_id`
12. extension version mismatch
13. native host manifest missing or invalid

A flow is incomplete if it documents only the happy path and does not explain:

- who initiates the action
- which process owns the state transition
- what persistent state changes
- what error code is returned on failure
- how the caller recovers

## First-Release Security Decisions

The first-release design is now closed on the following decisions:

1. `wallet_list_accounts` does not require an interactive approval gate.
2. `wallet_instance_id` is required whenever routing is ambiguous.
3. signed results use bounded multi-read retention.
4. message-signing and transaction-signing requests share one canonical request table.
5. after `request.presented`, only the same `wallet_instance_id` may resume the request.
6. the host may receive only structured status and result metadata; the extension remains the source of truth for rendered approval content.
7. blind signing is blocked by policy and unsupported payloads must be rejected.

## First Implementation Scope Freeze

The first implementation should remain intentionally narrow.

In scope:

- one local user session
- one Chrome-based wallet family
- one daemon instance per OS user
- explicit approval for every signing request
- transaction signing
- message signing
- polling-based host interaction

Out of scope for the first implementation:

- remote wallet access
- background auto-signing
- cross-device approval
- multiple browser families
- push callbacks into MCP hosts
- policy exceptions for low-risk signing

## Review Checklist

Before implementation begins, review the design with the following checklist:

- Are all process boundaries documented?
- Are all request states shared and consistent across docs?
- Are all result objects reflected in shared schemas?
- Are all terminal states explicit?
- Are all retry rules idempotent?
- Are all security decisions owned by the extension instead of the host?
- Are all local-only assumptions written down?
- Are all install-time artifacts discoverable and diagnosable?

## Closure Status

The required first-round document set now exists.

Before implementation begins, the remaining work is review and consistency checking, not additional top-level design expansion.
