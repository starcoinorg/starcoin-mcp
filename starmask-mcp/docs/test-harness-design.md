# Starmask MCP Test Harness Design

## Status

This document describes the recommended harness structure for the current repository state.

Repository status note: the in-tree `crates/starmask-mcp` adapter has been removed. Any harness
sections that mention MCP shim coverage now describe historical tests or future external adapter
coverage.

It covers both:

- the current `v1` extension-backed path
- the current phase-2 generic-backend and `local_account_dir` path

Current evidence status is tracked separately in:

- `../../docs/testing-coverage-assessment.md`

## Purpose

This document turns the acceptance contracts into concrete test-layer ownership.

Use it together with:

- `docs/testing-and-acceptance.md`
- `docs/wallet-backend-testing-and-acceptance.md`
- `../../docs/testing-coverage-assessment.md`

## Design Goals

The harness should make it easy to:

1. give each acceptance item one obvious test home
2. keep `v1` extension-path coverage distinct from phase-2 backend-path coverage
3. keep migration and compatibility checks explicit
4. keep browser- and UI-dependent checks out of routine local automation

## Test Layers

### Layer 1: Core Coordinator Tests

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
- lock-state and capability routing
- same-instance recovery and non-redelivery rules

### Layer 2: Repository And Migration Tests

Target:

- SQLite repository implementation in `starmaskd`

Use:

- temporary directories
- real SQLite files
- migration bootstrap from `v1` and `v2` entry states

Cover:

- schema creation
- unique constraints
- startup recovery queries
- result eviction
- positive `v1 -> v2` migration behavior
- rollback safety when migration backfill fails
- post-migration compatibility reads

### Layer 3: Daemon Transport Tests

Target:

- `starmaskd`

Use:

- real daemon process or in-process daemon runtime
- local socket or pipe
- protocol `v1` and protocol `v2` test clients

Cover:

- JSON-RPC request parsing
- shared error mapping
- command dispatch
- state persistence across requests
- `extension.register` and `extension.updateAccounts`
- `backend.register`, `backend.heartbeat`, and `backend.updateAccounts`
- `request.pullNext`, `request.presented`, `request.resolve`, and `request.reject`
- disabled-backend and unknown-instance rejection

### Layer 4: Native Messaging Bridge Tests

Target:

- `starmask-native-host` Chrome Native Messaging bridge

Keep ownership explicit: `starmask-mcp` is the MCP stdio adapter, `starmaskd` owns lifecycle and persistence, `starmask-native-host` is the bridge, and `Starmask` extension is the approval UI and signing authority.

Use:

- framed stdin/stdout harness
- fake extension message source
- fake daemon backend

Cover:

- frame parsing
- maximum-length rejection
- stderr/stdout separation
- reconnect and resume behavior
- message correlation through `message_id` and `reply_to`

### Layer 5: MCP Shim Tests

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

### Layer 6: Local Backend Agent Tests

Target:

- `starmask-local-account-agent`

Use:

- temporary account directories
- prompt stubs
- fake daemon client when full daemon startup is unnecessary

Cover:

- account discovery and snapshot publication
- read-only account handling
- `sign_message`
- `sign_transaction`
- unlock success, failure, and cancellation paths
- heartbeat state tracking
- no-secret-over-transport rules

### Layer 7: End-To-End Local Stack Tests

Target:

- full local stack without requiring the real browser UI

Split:

- extension-backed path:
  - `starmaskd`
  - `starmask-native-host`
  - fake extension runtime
  - `starmask-mcp`
- local-backend path:
  - `starmaskd`
  - `starmask-local-account-agent`
  - temporary account directory
  - `starmask-mcp` when host-visible behavior matters

Cover:

- request create -> claim -> present -> resolve
- request create -> claim -> reject
- cancel while approval is active
- restart before presentation
- restart after presentation with same-instance resume
- migration and compatibility smoke paths

### Layer 8: Real-Environment Validation

Target:

- browser-, extension-, and Inspector-dependent checks

Cover:

- MCP Inspector interoperability
- real Chrome or Chromium Native Messaging registration
- approval UI rendering and state transitions
- live browser reconnect and cancellation behavior

Current runbook:

- `docs/mcp-shim-real-environment-runbook.md`

When phase-2 browser- or prompt-surface checks need dedicated real-environment validation, add a
separate phase-2 runbook instead of overloading the `v1` extension-backed runbook.

## Shared Test Utilities

Reusable utilities should stay explicit and small:

- fake clock
- fake id generator
- fake extension runtime
- fake daemon backend
- local prompt stub
- temporary account-directory builder with permission and symlink helpers

## Fixture Strategy

Use repository-stable fixtures for:

- daemon JSON-RPC payloads
- Native Messaging payloads
- MCP tool outputs

Preferred location:

```text
starmask-mcp/tests/fixtures/   # historical adapter-fixture location
```

The canonical payload examples from:

- `docs/native-messaging-examples.md`

should seed those fixtures when fixture coverage is a better fit than handwritten assertions.

## Acceptance Traceability

Each acceptance item from:

- `docs/testing-and-acceptance.md`
- `docs/wallet-backend-testing-and-acceptance.md`

should map to at least one automated test layer or one explicit real-environment runbook step.

The repository-level assessment in:

- `../../docs/testing-coverage-assessment.md`

records current evidence status. This document defines where missing coverage should live.

## Recommended Sequence

Implement or expand tests in this order:

1. keep `v1` extension-backed regression coverage green
2. add positive `v1 -> v2` migration and compatibility tests
3. add local-account sign-message and sign-transaction integration tests
4. add `backend.heartbeat`, `backend.updateAccounts`, and backend-path recovery tests
5. add dedicated phase-2 end-to-end local stack scenarios
6. add a separate phase-2 real-environment runbook only when local automation no longer answers the release question

## Harness Readiness Checklist

This harness plan is ready to implement when:

1. a fake clock interface exists
2. a fake id generator exists
3. a fake extension runtime contract exists
4. temp-directory SQLite bootstrap exists
5. at least one End-To-End Local Stack Test exists

At that point, implementation can move directly toward integration and acceptance instead of inventing test strategy ad hoc.
