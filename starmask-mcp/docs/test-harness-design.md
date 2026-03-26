# Starmask MCP Test Harness Design

## Status

This document describes the recommended harness structure for the current `v1` extension-backed
stack.

If phase 2 adds generic backend-agent testing or `local_account_dir` flows, those additions should
extend this document explicitly after the corresponding protocol and acceptance documents are
approved.

## Purpose

This document defines how the first Rust implementation should be tested end to end without requiring every test to launch a real browser extension.

It turns the acceptance matrix into concrete test harness shapes.

## Design Goals

The harness should make it easy to test:

1. lifecycle correctness
2. restart recovery
3. daemon JSON-RPC behavior
4. Native Messaging framing
5. MCP shim result mapping

## Test Layers

## Layer 1: Pure Core Tests

Target:

- `starmask-core`

Use:

- in-memory repositories or deterministic fake repositories
- fake clock
- fake id generator

Cover:

- lifecycle transitions
- policy routing
- idempotency
- expiry rules
- result retention rules

## Layer 2: SQLite Repository Tests

Target:

- SQLite repository implementation

Use:

- temporary directories
- real SQLite file
- migration bootstrap

Cover:

- schema creation
- unique constraints
- claim transaction correctness
- startup recovery queries
- result eviction

## Layer 3: Daemon RPC Integration Tests

Target:

- `starmaskd`

Use:

- real daemon process or in-process daemon runtime
- local socket or pipe
- test client

Cover:

- JSON-RPC request parsing
- error mapping
- command dispatch
- state persistence across requests

## Layer 4: Native Messaging Bridge Tests

Target:

- `starmask-native-host`

Use:

- framed stdin/stdout harness
- fake extension message source
- fake daemon backend

Cover:

- frame parsing
- maximum length rejection
- stderr/stdout separation
- reconnect and resume behavior
- message correlation through `message_id` and `reply_to`

## Layer 5: MCP Shim Tests

Target:

- `starmask-mcp`

Use:

- `rmcp` stdio integration tests
- fake daemon backend

Cover:

- tool registration
- tool input validation
- tool output structure
- daemon error to MCP result mapping

## Layer 6: End-to-End Local Integration Tests

Target:

- full local stack except the real browser extension UI

Use:

- `starmaskd`
- `starmask-native-host`
- fake extension runtime
- `starmask-mcp`

Cover:

- request create -> claim -> present -> resolve
- request create -> claim -> reject
- cancel while UI open
- restart before presentation
- restart after presentation and same-instance resume

## Fake Components

## Fake Clock

Needed for:

- TTL expiry
- lease expiry
- result retention eviction

## Fake Id Generator

Needed for:

- deterministic `request_id`
- deterministic lease ids
- deterministic `presentation_id`

## Fake Extension Runtime

Should simulate:

- register
- heartbeat
- account updates
- `request.pullNext`
- `request.presented`
- `request.resolve`
- `request.reject`

It should not do browser UI work. It only exercises the contract.

## Fake Daemon Backend

Used in MCP shim and native host tests when full daemon startup is unnecessary.

Should simulate:

- successful responses
- shared error responses
- transport drops
- delayed responses

## Restart Simulation

The harness must support:

1. daemon stop and restart using the same SQLite file
2. native host reconnect
3. fake extension reconnect with the same `wallet_instance_id`
4. MCP host restart by preserving and reusing `request_id`

## Fixture Strategy

Use repository-stable fixtures for:

- daemon JSON-RPC payloads
- Native Messaging payloads
- MCP tool outputs

Preferred location:

```text
starmask-mcp/tests/fixtures/
```

The canonical Native Messaging examples from:

- `docs/native-messaging-examples.md`

should seed those fixtures.

## Acceptance Traceability

Each acceptance item from:

- `docs/testing-and-acceptance.md`

should map to at least one test id or harness scenario.

Recommended approach:

- maintain a small matrix file in tests or docs linking:
  - acceptance item
  - test module
  - scenario id

## Inspector and Manual Validation

Some checks should remain manual in early phases:

- MCP Inspector tool behavior
- approval UI visual behavior
- real Chrome Native Messaging registration

Those should still have scripted preparation steps so they are reproducible.

## Recommended First Test Sequence

Implement tests in this order:

1. pure lifecycle unit tests
2. SQLite schema and repository tests
3. daemon JSON-RPC tests
4. Native Messaging framing tests
5. MCP shim integration tests
6. restart and resume end-to-end tests

## Ready-to-Implement Checklist

This document is implementation-ready when:

1. fake clock interface exists
2. fake id generator exists
3. fake extension runtime contract exists
4. temp-directory SQLite test bootstrap exists
5. one end-to-end local integration harness exists

At that point, implementation can move directly toward integration and acceptance instead of inventing test strategy ad hoc.
