# Starcoin MCP Testing Coverage Assessment

## Status

This note records a repository-level assessment of automated test coverage relative to the current
design and acceptance documents.

It is based on:

- local test execution on the current `main` branch
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
| `starmask-mcp` | `98 passed` | all local Rust workspace tests passed |

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
- MCP tool-surface tests in `crates/starcoin-node-mcp-server/tests/tool_surface.rs`
- runtime bootstrap tests in `crates/starcoin-node-mcp-server/tests/runtime.rs`
- shared-schema compatibility test in `crates/starcoin-node-mcp-types/tests/schema_compat.rs`
- config parsing and schema tests in `crates/starcoin-node-mcp-types/src/config.rs`
- one ignored live read-only smoke test in `crates/starcoin-node-mcp-core/tests/live_read_only.rs`

Assessment:

| Area | Status | Evidence | Gap |
| --- | --- | --- | --- |
| Required Rust test layers | `partial` | Layers 1-3 are present through core, RPC, MCP, and schema tests. A live endpoint layer exists as an ignored read-only smoke test. | The required end-to-end transaction layer from the acceptance doc is not present as a routinely running automated test. The only live test is read-only and ignored by default. |
| Startup and capability probing | `partial` | Config and probe behavior are covered in `crates/starcoin-node-mcp-types/src/config.rs`, `crates/starcoin-node-mcp-rpc/src/tests.rs`, and `crates/starcoin-node-mcp-server/tests/runtime.rs`. | The acceptance doc asks for startup behavior on chain mismatch, genesis mismatch, and capability refresh after reconnect. Those scenarios are not fully exercised as explicit startup tests. |
| Query and ABI correctness | `partial` | Query degradation and pending-transaction behavior are covered in `crates/starcoin-node-mcp-core/tests/flow_closure.rs`; adapter capability normalization is covered in `crates/starcoin-node-mcp-rpc/src/tests.rs`; MCP JSON output checks exist in `crates/starcoin-node-mcp-server/tests/tool_surface.rs`. | The design expects stable normalized outputs for the whole query and ABI surface. Coverage exists, but not as a comprehensive matrix or snapshot suite for all query tools. |
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
- phase 2 multi-backend code is implemented, but its automated coverage is still materially behind
  the phase-2 acceptance contract

Current evidence:

- core lifecycle and routing tests in `crates/starmask-core/src/service.rs`
- daemon restart and persistence tests in `crates/starmaskd/tests/recovery.rs`
- daemon transport tests in `crates/starmaskd/tests/transport.rs`
- Native Messaging framing and bridge tests in `crates/starmask-native-host/src/*`
- MCP shim request and error mapping tests in `crates/starmask-mcp/tests/*`
- local-account helper and capability tests in `crates/starmask-local-account-agent/src/agent.rs`
- daemon config validation tests in `crates/starmaskd/src/config.rs`
- one migration rollback-safety test in `crates/starmaskd/src/sqlite_store.rs`

### `v1` Extension-Backed Contract

| Area | Status | Evidence | Gap |
| --- | --- | --- | --- |
| Protocol, lifecycle, and recovery | `strong` | `crates/starmask-core/src/service.rs`, `crates/starmaskd/tests/recovery.rs`, and `crates/starmaskd/tests/transport.rs` cover idempotency, lifecycle transitions, restart behavior, and same-instance resume. | The acceptance doc still requires manual evidence for browser- and UI-dependent release checks. |
| Native Messaging and MCP shim | `strong` | `crates/starmask-native-host/src/framing.rs`, `crates/starmask-native-host/src/bridge.rs`, `crates/starmask-native-host/src/notify.rs`, and `crates/starmask-mcp/tests/*` cover framing, bridge mapping, notification tracking, tool registration, request mapping, and MCP error mapping. | Real Chrome registration and MCP Inspector interoperability still need manual evidence. |
| Current release gate | `partial` | The local automated story for `v1` is substantial. | The repository does not include manual verification records for approval UI rendering, live browser reconnect behavior, or real Chrome/Inspector checks required by the current acceptance doc. |

### Phase 2 Multi-Backend Contract

| Area | Status | Evidence | Gap |
| --- | --- | --- | --- |
| Generic backend transport happy path | `partial` | `crates/starmaskd/tests/transport.rs` proves `backend.register -> request.pullNext -> request.presented -> request.resolve` for a local backend over protocol `v2`. | The phase-2 transport contract also requires explicit coverage for `backend.heartbeat`, `backend.updateAccounts`, unknown-instance rejection, disabled backend rejection, and the optional `request.hasAvailable` behavior. Those tests are not present. |
| `local_account_dir` capability and helper behavior | `partial` | `crates/starmask-local-account-agent/src/agent.rs` tests locked-account capability advertisement, public-key formatting, decode helpers, and unlock-capability failure behavior. | These are helper/unit tests, not backend integration tests. They do not prove the real local-account sign-message or sign-transaction flows end to end. |
| `local_account_dir` signing flows | `missing release evidence` | The implementation exists in `crates/starmask-local-account-agent/src/agent.rs`. | The phase-2 acceptance doc requires integration tests using temporary account directories. The repository does not contain end-to-end tests proving `sign_transaction` and `sign_message` return real signatures through the backend agent path. |
| Backend-local unlock behavior | `partial` | Routing and capability policy are tested in `crates/starmask-core/src/service.rs`; helper checks exist in `crates/starmask-local-account-agent/src/agent.rs`. | There is no automated integration test showing successful backend-local unlock, unlock failure, or unlock cancellation during a sign flow without exposing secrets over daemon transport. |
| Filesystem and security checks | `partial` | Strict-permission rejection is tested in `crates/starmaskd/src/config.rs`, and symlink-escape validation exists in implementation. | There is no explicit symlink-escape test, despite phase-2 security docs requiring it. |
| Phase-2 recovery | `missing release evidence` | Recovery coverage is strong for the extension-backed path in `crates/starmaskd/tests/recovery.rs`. | The recovery tests use extension registration helpers, not generic local-backend registration. The phase-2 acceptance doc specifically requires restart coverage for `local_account_dir` requests and same-instance resume on that backend path. |
| Migration and compatibility | `partial` | `crates/starmaskd/src/sqlite_store.rs` tests rollback safety when the `v2` migration backfill fails. | The phase-2 contract requires positive `v1 -> v2` migration coverage, proof that migrated `v1` rows remain readable, and proof that extension-backed compatibility stays green after migration. Those tests are not present. |
| Configuration acceptance | `partial` | `crates/starmaskd/src/config.rs` tests duplicate backend IDs, prompt-mode validation, permission strictness, and legacy-field conflicts. | The phase-2 acceptance doc also requires explicit coverage for legacy implicit-backend translation, invalid local-account paths, missing `chain_id`, and other backend-entry validation scenarios that are not fully represented as focused acceptance tests. |
| Performance and boundedness | `missing release evidence` | Existing tests indirectly cover lease expiry, TTL clamping, and non-redelivery in the core coordinator. | The phase-2 contract asks for explicit proof that idle polling remains cheap and stable, account snapshot replacement is atomic, result retention remains bounded after migration, and one backend cannot resume another backend's presented request on the multi-backend path. The repository does not contain a dedicated phase-2 performance or boundedness suite. |
| Phase-2 release gate | `not enough` | Phase-2 implementation code exists, and some happy-path transport coverage exists. | The repository does not yet meet the documented phase-2 rule that every acceptance area have a test or manual verification record. |

Conclusion:

- `starmask-mcp` is in a split state: the `v1` extension-backed implementation has strong local
  automated coverage, while phase 2 does not.
- The main problem is not a missing assertion or two. The phase-2 backend path needs a dedicated
  integration harness for `local_account_dir`, migration/compatibility tests, and backend-specific
  recovery tests.
- Phase-2 coverage should be expanded structurally, not by sprinkling isolated one-off assertions
  across unrelated files.

## Repository-Level Conclusion

1. `starcoin-node-mcp` has a useful and reasonably well-structured automated baseline, but it is
   still short of its own release-gate standard.
2. `starmask-mcp` satisfies much more of its `v1` contract than its phase-2 contract.
3. The highest-value next step is not to add random test cases. It is to add or reorganize the
   missing test harness layers so each major design contract has one obvious home:
   - `starcoin-node-mcp`: resource-governance and live transaction end-to-end coverage
   - `starmask-mcp`: phase-2 backend integration, migration/compatibility, and backend-path
     recovery coverage
