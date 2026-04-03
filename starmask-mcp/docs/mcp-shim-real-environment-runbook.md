# Starmask MCP Real-Environment Runbook

## Status

This runbook covers the historical `v1` extension-backed MCP-adapter real-environment checks.

Repository status note: `crates/starmask-mcp` has been removed from the workspace, so these steps
are no longer runnable against the current in-tree binaries without an external adapter.

It assumes:

- the Starmask browser extension is the signer
- Native Messaging is the live backend transport

It does not cover the current phase-2 `local_account_dir` or generic backend-agent path.

For the current repository-level status of those paths, see:

- `../../docs/testing-coverage-assessment.md`

## Purpose

This runbook describes the `starmask-mcp` checks that should be executed in a real local environment rather than only through fake-daemon tests.

Use it together with:

- `docs/mcp-shim-coverage-matrix.md`
- `docs/testing-and-acceptance.md`
- `docs/approval-ui-spec.md`
- `docs/native-messaging-contract.md`

## Scope

This runbook covers:

1. MCP Inspector validation over stdio
2. real Chrome Native Messaging registration and diagnostics
3. approval UI checks that require the real extension
4. reconnect and cancellation scenarios that need a live browser/runtime

It does not replace:

- `starmask-core` lifecycle tests
- `starmaskd` JSON-RPC and persistence tests
- `starmask-native-host` framing tests

## Preconditions

Assumptions:

1. you are on macOS or Linux
2. Chrome or Chromium is installed
3. the Starmask extension can be loaded locally
4. MCP Inspector is available on the machine

Build the local binaries first:

```bash
REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT/starmask-mcp"
cargo build -p starmaskd -p starmask-mcp -p starmask-native-host -p starmaskctl
```

Choose one local runtime path set for the session:

```bash
export STARMASK_TEST_SOCKET=/tmp/starmaskd.sock
export STARMASK_TEST_DB=/tmp/starmaskd.sqlite3
```

Configure the daemon with the extension ID you intend to load:

```bash
export STARMASKD_CHANNEL=development
export STARMASKD_ALLOWED_EXTENSION_IDS=<your_extension_id>
```

## Shared Setup

Start the daemon:

```bash
REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT/starmask-mcp"
cargo run -p starmaskd -- serve \
  --socket-path "$STARMASK_TEST_SOCKET" \
  --database-path "$STARMASK_TEST_DB"
```

Run basic diagnostics in a second shell:

```bash
REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT/starmask-mcp"
cargo run -p starmaskctl -- doctor \
  --socket-path "$STARMASK_TEST_SOCKET" \
  --database-path "$STARMASK_TEST_DB"
```

Expected result:

1. `config`, `database`, and `daemon` checks are `[ok]`
2. `native-host-manifest` is `[ok]` once the Chrome manifest is installed
3. `wallet` and `accounts` remain failing until the extension actually registers

Start the MCP adapter in a third shell:

```bash
REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT/starmask-mcp"
cargo run -p starmask-mcp -- --daemon-socket-path "$STARMASK_TEST_SOCKET"
```

Keep that process running for the Inspector scenarios below.

## Scenario RE-1: Inspector Stdio Smoke

Purpose:

- verify real stdio startup and MCP Inspector interoperability

Steps:

1. connect MCP Inspector to the running `starmask-mcp` process over stdio
2. call `wallet_status`
3. call `wallet_list_instances`
4. call `wallet_list_accounts`
5. if one account is visible, call `wallet_get_public_key`

Expected result:

1. Inspector connects without transport errors
2. tool schemas appear for all wallet tools
3. results are structured JSON rather than plain-text blobs
4. no raw signatures or raw signed transaction payloads appear in stderr logs

Evidence to capture:

1. Inspector screenshot or transcript showing connected tool list
2. one structured response per tool
3. stderr snippet showing normal startup without secret leakage

## Scenario RE-2: Native Host Manifest And Browser Registration

Purpose:

- verify the browser can discover the native host and the manifest matches the configured allowlist

Steps:

1. install the native host manifest into the standard Chrome path for the current OS
2. rerun `cargo run -p starmaskctl -- doctor ...`
3. load the extension in Chrome/Chromium
4. trigger the extension startup path that performs `connectNative()`

Expected result:

1. `starmaskctl doctor` reports `[ok] native-host-manifest`
2. manifest `name` matches the daemon `native_host_name`
3. manifest `allowed_origins` contains `chrome-extension://<your_extension_id>/`
4. the extension successfully registers instead of failing with host-not-found

Evidence to capture:

1. `starmaskctl doctor` output
2. manifest file path used by the browser
3. browser console or extension log showing successful registration

## Scenario RE-3: Transaction Approval UI Uses Canonical Payload

Purpose:

- validate the extension renders canonical transaction fields and treats `display_hint` as secondary

Steps:

1. create a transaction-sign request from Inspector or another MCP host via `wallet_request_sign_transaction`
2. choose a request where `display_hint` is present
3. wait for the extension approval UI to open
4. verify the UI transitions from `loading` to `ready`
5. compare the rendered chain/account/action details with the canonical transaction payload, not just the hint text

Expected result:

1. `loading` disables approval actions
2. `ready` shows chain, account, transaction kind, gas, expiration, and request ID
3. `display_hint` appears only as supporting context
4. approval is not offered if the extension cannot decode the payload safely

Evidence to capture:

1. screenshot of `loading`
2. screenshot of `ready`
3. note describing which fields came from canonical payload rendering

## Scenario RE-4: Message Sign UI And Reject Path

Purpose:

- validate message sign rendering and explicit rejection behavior

Steps:

1. create a message-sign request through `wallet_sign_message`
2. verify the UI shows account, message format, canonical preview, byte length, and request ID
3. reject the request in the extension UI
4. poll `wallet_get_request_status` from Inspector

Expected result:

1. the UI renders canonical message details
2. the request ends in the rejected path
3. Inspector status polling reflects terminal rejection metadata

Evidence to capture:

1. screenshot of message-sign UI
2. Inspector `wallet_get_request_status` output after rejection

## Scenario RE-5: Cancel While UI Is Open

Purpose:

- validate cancellation propagation from host to a live approval UI

Steps:

1. create a sign request and wait until the approval UI reaches `ready`
2. without interacting in the extension, call `wallet_cancel_request`
3. observe the open approval UI
4. optionally poll `wallet_get_request_status`

Expected result:

1. the UI transitions to `cancelled`
2. approve becomes disabled immediately
3. any in-flight local approval action is discarded
4. daemon status reflects cancellation

Evidence to capture:

1. Inspector cancel response
2. screenshot of `cancelled` state
3. follow-up status response

## Scenario RE-6: Reconnect Before And After `request.presented`

Purpose:

- validate the real same-instance recovery rules around presentation

Steps:

1. create a sign request
2. disconnect the extension before the approval UI is fully presented
3. reconnect and observe whether the request is returned to the queue
4. create another sign request
5. allow the extension to reach the point where `request.presented` has been sent
6. disconnect the extension again
7. reconnect using the same wallet instance
8. verify the request resumes on the same instance only

Expected result:

1. before `request.presented`, disconnect may return the request to a pre-presentation state
2. after `request.presented`, the request remains pinned to the same `wallet_instance_id`
3. the UI shows the recovery banner for the resumed pending request
4. the resumed request is not delivered to a different wallet instance

Evidence to capture:

1. daemon or extension logs around disconnect/reconnect
2. screenshot of the recovery banner
3. note confirming same-instance-only resume

## Scenario RE-7: Production-Channel Extension ID Rejection

Purpose:

- validate channel allowlist enforcement in a browser-like setup

Steps:

1. stop the development daemon
2. restart `starmaskd` with:

```bash
export STARMASKD_CHANNEL=production
export STARMASKD_ALLOWED_EXTENSION_IDS=<production_extension_id_only>
```

3. load a development extension build whose ID is not in the production allowlist
4. trigger extension registration

Expected result:

1. registration fails closed
2. the extension does not continue normal request handling
3. the failure is diagnosable from logs or registration feedback

Evidence to capture:

1. daemon log entry for the rejected registration
2. browser/extension log showing the failure path

## Release Evidence Checklist

Before release, store one record per real-environment scenario that includes:

1. date and operator
2. commit SHA under test
3. browser build and extension build identifier
4. scenario ID from this runbook
5. pass/fail result
6. links to screenshots, logs, or Inspector transcripts
