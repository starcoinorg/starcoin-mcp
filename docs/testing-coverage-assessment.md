# Starcoin MCP Testing Coverage Assessment

## Status

This note records a repository-level assessment of automated test coverage relative to the current
design and acceptance documents.

It is based on:

- local test execution on the current testing branch
- direct inspection of the current Rust tests and test-support code
- comparison against the current design and acceptance documents in both subprojects

For this note, "enough coverage" means the standard used by the design documents themselves:

- every acceptance area has at least one passing automated test, or
- the repository contains a clear manual verification record for that area

## Local Test Execution

The following commands were run locally:

- `cargo test --workspace` in `starcoin-node-mcp/`
- `cargo test --workspace` in `starmask-mcp/`

Summary:

| Subproject | Result | Notes |
| --- | --- | --- |
| `starcoin-node-mcp` | `40 passed`, `1 ignored` | the ignored test is `crates/starcoin-node-mcp-core/tests/live_read_only.rs` and requires `STARCOIN_NODE_MCP_E2E_RPC_URL` |
| `starmask-mcp` | `134 passed` | all local Rust workspace tests passed after the phase-2 backend additions on this branch |

Follow-up targeted runs on this branch:

| Subproject | Result | Notes |
| --- | --- | --- |
| `starmask-local-account-agent` | `24 passed` | covers phase-2 signing, unlock success/failure/cancellation, prompt rendering, no-password-over-daemon proof, snapshot sync, and full local-stack restart flows |
| `starmaskd` | `45 passed` | includes transport, config, migration, compatibility, recovery, and boundedness coverage for phase 2 |

## `starcoin-node-mcp`

Primary design references:

- `starcoin-node-mcp/docs/testing-and-acceptance.md`
- `starcoin-node-mcp/docs/design-closure-plan.md`
- `starcoin-node-mcp/docs/starcoin-node-mcp-interface-design.md`
- `starcoin-node-mcp/docs/rpc-adapter-design.md`

Current evidence:

- core unit and policy tests in `crates/starcoin-node-mcp-core/src/tests.rs`
- flow-closure tests in `crates/starcoin-node-mcp-core/tests/flow_closure.rs`
- RPC adapter tests in `crates/starcoin-node-mcp-rpc/src/tests.rs`
- shared-schema compatibility test in `crates/starcoin-node-mcp-types/tests/schema_compat.rs`
- config parsing and schema tests in `crates/starcoin-node-mcp-types/src/config.rs`
- one ignored live read-only smoke test in `crates/starcoin-node-mcp-core/tests/live_read_only.rs`

Assessment:

| Area | Status | Evidence | Gap |
| --- | --- | --- | --- |
| Required Rust test layers | `partial` | Layers 1-2 are present through core, RPC, and schema tests. A live endpoint layer exists as an ignored read-only smoke test. | The repository no longer includes an MCP adapter crate, so there is no in-tree MCP transport layer under test. The required end-to-end transaction layer from the acceptance doc is also not present as a routinely running automated test. |
| Startup and capability probing | `partial` | Config and probe behavior are covered in `crates/starcoin-node-mcp-types/src/config.rs` and `crates/starcoin-node-mcp-rpc/src/tests.rs`. | The acceptance doc asks for startup behavior on chain mismatch, genesis mismatch, and capability refresh after reconnect. Those scenarios are not fully exercised as explicit startup or CLI bootstrap tests. |
| Query and ABI correctness | `partial` | Query degradation and pending-transaction behavior are covered in `crates/starcoin-node-mcp-core/tests/flow_closure.rs`; adapter capability normalization is covered in `crates/starcoin-node-mcp-rpc/src/tests.rs`. | The design expects stable normalized outputs for the whole query and ABI surface. Coverage exists, but not as a comprehensive matrix or snapshot suite for all query tools. |
| Preparation and simulation correctness | `strong` | Preparation, skipped-simulation behavior, explicit follow-up simulation, sequence fallback, and shared-schema compatibility are covered in `crates/starcoin-node-mcp-core/tests/flow_closure.rs`, `crates/starcoin-node-mcp-core/src/tests.rs`, and `crates/starcoin-node-mcp-types/tests/schema_compat.rs`. | The acceptance doc asks for host-facing result snapshots; current tests use structural assertions rather than snapshot fixtures. |
| Submission and reconciliation behavior | `partial` | Local reconciliation policy, `submission_unknown`, stale blind-resubmission blocking, chain-context validation, and simulation-attestation policy are covered in `crates/starcoin-node-mcp-core/src/tests.rs` and `crates/starcoin-node-mcp-core/tests/flow_closure.rs`. | There is no automated end-to-end test proving a successful prepare -> simulate -> sign -> submit -> watch flow against a live or local endpoint. Expiry and stale-sequence handling are validated in policy code paths, but not as full endpoint-integrated scenarios. |
| Security behavior | `partial` | Chain-context validation, submit policy, and config redaction behavior are covered in `crates/starcoin-node-mcp-core/src/tests.rs` and `crates/starcoin-node-mcp-types/src/config.rs`. | The acceptance doc also calls for evidence around transport security defaults, log redaction, and wallet-side security boundaries. The repository does not currently contain explicit manual verification records for those release-gate items. |
| Configuration safety | `partial` | Missing chain pins, enum parsing, TOML round-trip, schema emission, and redacted token handling are covered in `crates/starcoin-node-mcp-types/src/config.rs`. | The acceptance doc explicitly calls out disallowed hosts, insecure remote transport rejection, and remote genesis-hash requirements. Those rules exist in implementation, but there are not matching explicit tests for each acceptance item. |
| Resource and performance governance | `missing release evidence` | The implementation includes bounds and permit checks in `crates/starcoin-node-mcp-core/src/submission.rs` and `crates/starcoin-node-mcp-core/src/transaction.rs`. | The repository does not currently contain direct tests for `watch_transaction` bound clamping, watch permit exhaustion, expensive-request `rate_limited` behavior, permit release after cancellation, list-tool clamping, or `prepare_publish_package` oversize rejection. |
| Release-gate evidence | `not enough` | The project has a solid local unit and integration baseline. | The acceptance doc requires every area to have an automated test or manual verification record. That standard is not yet met in-repo. |

Conclusion:

- `starcoin-node-mcp` has a credible development baseline and a good amount of policy and adapter
  coverage.
- It does not yet meet its own documented release-gate standard.
- The biggest structural gap is not small assertions; it is the absence of a dedicated resource-
  governance and transaction end-to-end test layer that exercises the implemented watch, rate-limit,
  and publish-package boundaries.

## `starmask-mcp`

Primary design references:

- `starmask-mcp/docs/testing-and-acceptance.md`
- `starmask-mcp/docs/wallet-backend-testing-and-acceptance.md`
- `starmask-mcp/docs/test-harness-design.md`
- `starmask-mcp/docs/wallet-backend-agent-contract.md`
- `starmask-mcp/docs/wallet-backend-local-socket-binding.md`

Important scope distinction:

- the repository still contains a strong `v1` extension-backed acceptance and coverage story
- phase 2 multi-backend code is implemented, and this branch now covers most of that contract in
  local automation

Current evidence:

- core lifecycle and routing tests in `crates/starmask-core/src/service.rs`
- daemon restart and persistence tests in `crates/starmaskd/tests/recovery.rs`
- phase-2 local-backend recovery tests in `crates/starmaskd/tests/local_backend_recovery.rs`
- daemon transport tests in `crates/starmaskd/tests/transport.rs`
- migration compatibility tests in `crates/starmaskd/tests/migration_compatibility.rs`
- Native Messaging framing and bridge tests in `crates/starmask-native-host/src/*`
- local-account signing, unlock, snapshot, and heartbeat tests in `crates/starmask-local-account-agent/src/agent.rs`
- full local-stack daemon-plus-agent tests in `crates/starmask-local-account-agent/src/agent/stack_tests.rs`
- local prompt rendering tests in `crates/starmask-local-account-agent/src/prompt.rs`
- daemon config validation tests in `crates/starmaskd/src/config.rs`
- positive and rollback-safety migration tests in `crates/starmaskd/src/sqlite_store.rs`

### `v1` Extension-Backed Contract

| Area | Status | Evidence | Gap |
| --- | --- | --- | --- |
| Protocol, lifecycle, and recovery | `strong` | `crates/starmask-core/src/service.rs`, `crates/starmaskd/tests/recovery.rs`, and `crates/starmaskd/tests/transport.rs` cover idempotency, lifecycle transitions, restart behavior, and same-instance resume. | The acceptance doc still requires manual evidence for browser- and UI-dependent release checks. |
| Native Messaging and MCP shim | `partial` | `crates/starmask-native-host/src/framing.rs`, `crates/starmask-native-host/src/bridge.rs`, and `crates/starmask-native-host/src/notify.rs` cover framing, bridge mapping, and notification tracking. | The repository no longer includes the in-tree `starmask-mcp` adapter, so there is no local MCP shim coverage. Real Chrome registration and any external MCP adapter interoperability still need manual evidence. |
| Current release gate | `partial` | The local automated story for `v1` is substantial. | The repository does not include manual verification records for approval UI rendering, live browser reconnect behavior, or real Chrome/Inspector checks required by the current acceptance doc. |

### Phase 2 Multi-Backend Contract

| Area | Status | Evidence | Gap |
| --- | --- | --- | --- |
| Generic backend transport happy path | `strong` | `crates/starmaskd/tests/transport.rs` now proves `backend.register`, `backend.heartbeat`, `backend.updateAccounts`, `request.pullNext`, `request.presented`, `request.resolve`, `request.reject`, unknown-instance rejection, disabled-backend rejection, `request.hasAvailable`, and `protocol_version = 1` rejection for generic backend methods. | No major automated gap remains in the transport layer itself. |
| `local_account_dir` capability and helper behavior | `strong` | `crates/starmask-local-account-agent/src/agent.rs` covers locked-account capability advertisement, public-key formatting, decode helpers, snapshot sync, heartbeat payload reporting, read-only account listing, and read-only signing rejection. | No obvious helper-layer acceptance gap remains. |
| `local_account_dir` signing flows | `strong` | `crates/starmask-local-account-agent/src/agent.rs` proves `sign_message` and `sign_transaction`; `crates/starmask-local-account-agent/src/agent/stack_tests.rs` proves both flows again through a real daemon plus local agent over the local-socket path. | No major automated gap remains in signing-flow correctness. |
| Backend-local unlock behavior | `strong` | `crates/starmask-local-account-agent/src/agent.rs` covers unlock success, wrong-password rejection, cancellation rejection, fail-closed behavior without `unlock`, user rejection for both request kinds, and an explicit proof that the unlock password does not appear in the daemon RPC transcript. | This now covers the backend-local unlock contract. |
| Filesystem and security checks | `strong` | `crates/starmaskd/src/config.rs` covers insecure-permission rejection and symlink-escape rejection; `crates/starmask-local-account-agent/src/prompt.rs` covers canonical payload rendering and keeps host-provided display fields labeled as untrusted; `crates/starmask-local-account-agent/src/agent.rs` proves the unlock password does not cross the daemon RPC boundary; `crates/starmask-local-account-agent/src/agent/stack_tests.rs` now captures daemon logs during unlock plus signing flows and proves default logs omit the unlock password, exported private key bytes, canonical message text, and raw transaction payload bytes. | No major automated security-evidence gap remains in the repository. |
| Phase-2 recovery | `strong` | `crates/starmaskd/tests/local_backend_recovery.rs` now covers restart with a generic backend registration record present, plus `created`, `dispatched`, and `pending_user_approval` local-backend requests; `crates/starmask-local-account-agent/src/agent/stack_tests.rs` covers backend restart before and after `request.presented`, including same-instance resume. | No major automated gap remains in phase-2 recovery. |
| Migration and compatibility | `strong` | `crates/starmaskd/src/sqlite_store.rs` covers positive `v1 -> v2` backfill/readability and rollback safety; `crates/starmaskd/tests/migration_compatibility.rs` proves migrated extension-backed rows still route requests and still respect result retention; `crates/starmaskd/tests/transport.rs` proves `protocol_version = 1` clients are rejected rather than silently treated as generic `v2` clients. | No major automated compatibility gap remains. |
| Configuration acceptance | `strong` | `crates/starmaskd/src/config.rs` now explicitly covers legacy implicit-backend translation, legacy-field conflicts, duplicate backend IDs, prompt-mode validation, invalid local-account paths, missing `chain_id`, strict permissions, and symlink escape rejection. | No major automated gap remains in config validation. |
| Performance and boundedness | `strong` | `crates/starmaskd/tests/transport.rs` proves repeated empty `request.pullNext` stays stable, `crates/starmaskd/tests/transport.rs` proves account snapshot replacement is atomic, `crates/starmaskd/tests/migration_compatibility.rs` proves result retention remains bounded after migration, and `crates/starmaskd/tests/local_backend_recovery.rs` proves one backend cannot resume another backend's presented request. | No major automated boundedness gap remains. |
| Phase-2 release gate | `strong` | Phase-2 acceptance areas now have direct automated evidence in one obvious test layer, including explicit default-log redaction coverage for sensitive signing material. | No major automated release-gate gap remains inside this repository. |

Conclusion:

- `starmask-mcp` is in a split state: the `v1` extension-backed implementation has strong local
  automated coverage, while phase 2 is now close to its documented release gate.
- This branch adds the missing structural layers that were previously absent: phase-2 backend-path
  recovery tests, full daemon-plus-agent local-stack tests, migration compatibility smoke tests,
  and explicit boundedness checks.
- The remaining release work is now mostly external or environment-specific validation rather than
  core phase-2 correctness or security evidence inside this repository.

## Repository-Level Conclusion

1. `starcoin-node-mcp` has a useful and reasonably well-structured automated baseline, but it is
   still short of its own release-gate standard.
2. `starmask-mcp` now has strong automated coverage for both its `v1` contract and its documented
   phase-2 contract inside the repository.
3. The highest-value remaining testing work is now concentrated in two places:
   - `starcoin-node-mcp`: resource-governance and live transaction end-to-end coverage
   - `starmask-mcp`: external/manual environment validation beyond repo-local automation
