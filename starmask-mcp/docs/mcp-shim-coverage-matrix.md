# Starmask MCP Shim Coverage Matrix

## Status

This note tracks coverage for the historical `v1` `starmask-mcp` adapter layer.

Repository status note: `crates/starmask-mcp` has been removed from the workspace, so this matrix
is now archival reference material rather than current in-tree coverage.

It is intentionally narrower than the full project acceptance matrix in
`docs/testing-and-acceptance.md`.

For the current repository-level assessment that also considers the implemented phase-2 code paths,
see:

- `../../docs/testing-coverage-assessment.md`

## Scope

Use this note to answer:

1. what the MCP shim already covers with local automated tests
2. what should be covered by other automated layers
3. what still needs a real environment

Detailed real-environment steps live in:

- `docs/mcp-shim-real-environment-runbook.md`

## Adapter Automated Coverage

Historical local automated coverage lived in `crates/starmask-mcp/tests/` before that adapter was
removed:

| Flow | Status | Evidence |
| --- | --- | --- |
| tool registration surface | automated | `tool_surface.rs::advertised_tools_expose_expected_wallet_surface` |
| `wallet_status` result mapping | automated | `request_mapping.rs::call_tool_json_wallet_status_serializes_response` |
| `wallet_list_instances` request/result mapping | automated | `request_mapping.rs::call_tool_json_list_instances_requests_all_instances` |
| `wallet_list_accounts` request/result mapping | automated | `tool_surface.rs::call_tool_json_list_accounts_uses_structured_request` |
| `wallet_get_public_key` request/result mapping | automated | `request_mapping.rs::call_tool_json_get_public_key_tracks_target_wallet_instance` |
| `wallet_request_sign_transaction` request/result mapping | automated | `request_mapping.rs::call_tool_json_sign_transaction_maps_host_request_to_daemon_params` |
| `wallet_sign_message` request/result mapping | automated | `request_mapping.rs::call_tool_json_sign_message_maps_format_and_ttl` |
| `wallet_get_request_status` request/result mapping | automated | `request_mapping.rs::call_tool_json_get_request_status_parses_string_ids` |
| `wallet_cancel_request` request/result mapping | automated | `request_mapping.rs::call_tool_json_cancel_request_parses_string_ids` |
| invalid empty `wallet_instance_id` input | automated | `request_mapping.rs::invalid_wallet_instance_id_is_reported_as_invalid_request` |
| unknown tool name handling | automated | `request_mapping.rs::unknown_tool_is_reported_as_invalid_request` |
| `protocol_version_mismatch` MCP error mapping | automated | `error_mapping.rs::protocol_version_mismatch_maps_to_invalid_params_with_shared_code` |
| `wallet_selection_required` MCP error mapping | automated | `error_mapping.rs::wallet_selection_required_preserves_shared_code_in_internal_error` |
| id validation to MCP invalid-params mapping | automated | `error_mapping.rs::id_validation_error_maps_to_invalid_params` |
| default socket path convention | automated | `tool_surface.rs::default_socket_path_matches_platform_convention` |

## Flows Intentionally Covered Elsewhere

These are not good fits for more `starmask-mcp`-only tests and should instead be covered by the
other harness layers.

| Acceptance area | Recommended layer | Reason |
| --- | --- | --- |
| `client_request_id` retry returns same `request_id` | Layer 3 `starmaskd` | idempotency is daemon-owned state, not adapter-owned state |
| duplicate `client_request_id` plus different payload returns `idempotency_key_conflict` | Layer 3 `starmaskd` | requires daemon persistence and payload hashing |
| lifecycle transitions such as `created -> dispatched -> pending_user_approval -> approved` | Layer 1/3/6 | lifecycle ownership lives in `starmaskd` and `starmask-core` |
| lease expiry returning `dispatched -> created` | Layer 1/2/6 | requires fake clock or persisted daemon state |
| restart and disconnect recovery | Layer 2/4/6 | requires SQLite persistence, native host reconnect, or fake extension reconnect |
| result retention and post-eviction status behavior | Layer 1/2/6 | requires persistence and retention expiry behavior |
| Native Messaging frame parsing and `message_id` / `reply_to` correlation | Layer 4 `starmask-native-host` | framing is outside the MCP shim |
| `request.presented`, same-instance resume, and no cross-instance redelivery | Layer 4/6 | requires extension/native-host/daemon interaction |
| locked-wallet rejection and unsupported-payload refusal | Layer 3/6 | requires daemon policy and extension approval logic |

## Real Environment Validation

The following checks should be recorded as real-environment or manual validation, not replaced by
fake-daemon unit tests:

| Flow | Why real environment is needed |
| --- | --- |
| MCP Inspector over stdio against a running `starmaskd` and `starmask-mcp` | validates real stdio wiring, process startup, and inspector interoperability |
| real Chrome Native Messaging registration | validates Chrome host manifest discovery, caller origin handling, and actual browser process behavior |
| approval UI visual states such as `loading`, `ready`, `cancelled`, `expired`, `unsupported`, and recovery banner | fake adapter tests cannot prove the actual rendered screen behavior |
| transaction approval renders canonical payload fields rather than trusting `display_hint` | requires the real extension decode and render path |
| production-channel extension ID rejection in a browser-like setup | requires real extension packaging, origin behavior, and registration flow |

## Current Recommendation

Near-term automation work should continue to focus on:

1. daemon JSON-RPC tests for idempotency, lifecycle-backed shared errors, and persistence-backed
   behavior
2. native host tests for framing and reconnect behavior
3. local stack tests for cancel-while-open and resume-after-presentation flows

Real-environment checks should remain separate release-gate evidence rather than being mixed into
unit or integration test counts.
