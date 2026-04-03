# TODO

## vm_profile summary

- Current conclusion:
  - `vm_profile` is node-side RPC surface routing, not an account property.
  - `auto` does not detect whether one account is "VM1" or "VM2".
  - For dual-surface transaction methods, `auto` prefers the VM2 RPC surface and only falls back to the VM1 RPC surface when the VM2 method is unavailable.
  - For some account/resource read paths, `auto` may still start from a VM1 read path and only use VM2 for repair or retry.
  - Because of that mixed routing behavior, `auto` is not a good default when the caller already knows the operation must use VM1 semantics or VM2 semantics.
  - Transfer flows with fixed semantics should choose `vm1_only` or `vm2_only` explicitly and pass a matching `token_code`.

## vm_profile follow-ups

- [x] Expose `vm_profile` as a first-class plugin/runtime choice for transfer workflows.
  - `scripts/node_cli_client.py` now accepts `--vm-profile` and rewrites a temporary `node-cli.toml` per invocation so host-side transfer calls can target an explicit routing profile.
- [x] Add a `--vm-profile` option to `scripts/run_transfer_test.py`.
  - Prefer the generated `node-cli.toml` name and keep `node-mcp.toml` only as an input compatibility alias.
- [ ] Decide whether transfer-oriented configs and examples should keep defaulting to `vm_profile = "auto"` or require an explicit `vm1_only` / `vm2_only` choice for semantically fixed operations.
- [ ] If per-transfer profile selection is needed, choose an implementation direction. Candidates:
  - [x] Profile-aware CLI startup/config switching before the transfer run starts.
  - [ ] Separate node-cli configs for VM1-only and VM2-only paths.
  - [ ] Extend the plugin surface so a transfer flow can target a selected routing profile explicitly.
- [x] Add a short user-facing note in the plugin docs explaining that `auto` is RPC routing, not per-account VM detection, and which methods are shared, VM1-surface, or VM2-surface.

## transfer confirmation follow-ups

- [x] Change the default post-submit behavior so a submitted transaction waits for at least 1 additional block before the workflow reports success.
  - The converged default is now `min_confirmed_blocks = 2`, which means the inclusion block plus at least 1 additional observed block.
- [x] Add a user-facing transfer option for block-based confirmation depth.
  - `scripts/run_transfer_test.py` now exposes `--min-confirmed-blocks`.
- [x] Prefer `watch_transaction` as the single source of truth for confirmation-depth waiting.
  - `starcoin-node-mcp.submit_signed_transaction(blocking = true)` now reuses `watch_transaction` semantics instead of defining a separate confirmation model.
- [x] Expose one higher-level confirmation setting in the transfer skill/plugin and map it onto `watch_transaction`, so users do not need to reason about tool boundaries during a transfer.
- [x] Keep `submit_signed_transaction` focused on submission and immediate execution status. If it offers a blocking convenience mode, it should reuse `watch_transaction` semantics instead of defining a second confirmation model.
- [x] Define the confirmation semantics precisely in docs and tool output.
  - `confirmed_blocks` now means `head_block_number - inclusion_block_number + 1`, and `status_summary.confirmed = true` with top-level `confirmed = false` means "included but not yet deep enough".
- [x] Update the transfer skill, README, and `scripts/run_transfer_test.py` so the documented default matches the runtime behavior and the CLI test can exercise both the default 1-block wait and a user-specified confirmation depth.

## transfer usability and recovery follow-ups

- [ ] Strengthen the transfer host contract so the `starcoin-node-cli` + `starmaskd` path is treated as the only valid transfer execution path. If the local runtime is unavailable, the host should stop and send the user to `scripts/doctor.py` instead of falling back to direct `starcoin` CLI commands for prepare, submit, watch, balance, or transaction-status steps.
- [ ] Reduce CLI-biased setup language in `README.md` so the `starcoin` examples are clearly scoped to wallet bootstrap and local funding, not the normal host-side transfer flow that Codex should execute.
- [ ] Add a first-class amount-normalization story for transfers. `prepare_transfer.amount` is currently a raw integer string, which leaves human-readable amounts as ad hoc host logic.
- [ ] Improve common STC ergonomics explicitly. When the token is omitted or `0x1::STC::STC`, the workflow should support or at least clearly document 9-decimal normalization so standard STC transfers do not stall on avoidable precision confirmation.
- [ ] Decide the general decimals strategy for non-STC assets. Candidates: a token-metadata query tool, an explicit `amount_unit` or `decimals` input on the script and skill surface, or a separate normalization helper that returns the canonical raw amount plus display metadata.
- [ ] Show both the raw on-chain integer amount and the human-readable amount in transfer confirmations whenever normalization happened, so the user can verify what is actually being signed.
- [ ] Make submit failure handling more deterministic in the host workflow. `submission_unknown` should route to reconcile-by-hash or watch-by-hash behavior, while `transaction_expired`, `sequence_number_stale`, and `invalid_chain_context` should route to reprepare-and-resign guidance instead of generic failure text.
- [ ] Tighten the submitted-but-unconfirmed state in docs and host UX. If the submit call was accepted but the watch step is missing, timed out, or failed, the workflow should surface that as an intermediate state rather than a silent success or opaque error.
- [ ] Add coverage in `scripts/run_transfer_test.py` or adjacent acceptance tests for human-readable STC amounts, `submission_unknown` recovery, and reprepare-resign flows after stale sequence or expiration failures.

## script runtime contract follow-ups

- [ ] Make `node-cli.toml` the only documented chain config name and keep `node-mcp.toml` as a compatibility fallback until migration is complete.
- [ ] Reduce environment-variable-driven launcher indirection in the normal script path so PATH-based binaries and default config locations remain the primary operator story.
- [ ] Move any remaining source-tree-only overrides behind clearly dev-scoped documentation instead of mixing them into the default setup path.
- [ ] Keep reviewing `scripts/doctor.py` output and remediation text so normal users are guided toward default install locations and PATH setup first.

## wallet runtime tui follow-ups

- [ ] Implement a new `desktop_prompt` path inside `starmask-local-account-agent` instead of routing approval through `wallet_runtime.py`. Reuse the existing `ApprovalPrompt` abstraction and keep request resolution, rejection, unlock, and signing logic inside the agent.
- [ ] Add a first TUI approval prompt implementation for `local_account_dir` that supports: canonical request rendering, approve/reject, optional raw-payload inspection, and local password entry for locked accounts.
- [ ] Write a dedicated local approval UI spec for `desktop_prompt` instead of implicitly borrowing the extension UI contract, including required fields, recovery states, rejection states, and what remains visually secondary or untrusted.
- [ ] Relax the current `desktop_prompt` rejection in `starmaskd` config loading and `local-account-agent` startup once the prompt surface is implemented, and add coverage that both `tty_prompt` and `desktop_prompt` remain valid and distinct.
- [ ] Tighten the `request.presented` timing for local prompts so the agent reports presentation only after the TUI is actually visible and actionable, matching the backend-agent contract more closely.
- [ ] Build a runtime-manager TUI that owns `starmaskd` lifecycle as a child process, shows daemon/socket/agent status in one screen, and can start/stop/restart the wallet-side stack without changing the daemon's coordinator role.
- [ ] Keep the first TUI architecture process-separated: TUI process as runtime owner, `starmaskd` as child process, and local-account-agent request handling behind a prompt bridge, rather than embedding the daemon directly into the UI process.
- [ ] Define the bridge between the TUI event loop and the agent prompt implementation, including how one pending request is surfaced to the UI, how the UI returns approve/reject/password decisions, and how terminal cleanup behaves on cancel, panic, or Ctrl-C.
- [ ] Add a safe startup fallback so unsupported terminals or TUI initialization failures degrade to `tty_prompt` cleanly instead of leaving the wallet runtime unusable.
- [ ] Make the TUI emphasize chain and environment identity before approval: backend label, account address, chain id, and when available the node-side expected network and genesis hash, with explicit warnings for mismatch or missing chain context.
- [ ] Add operator guardrails in the runtime manager: show whether a request is currently pending, block stop/restart by default while an approval is active, and require an explicit force action for disruptive operations.
- [ ] Treat local password handling as sensitive memory in the new prompt path: avoid logging prompt contents, redact crash output, and zeroize password buffers where practical after unlock/signing completes.
- [ ] Expose lock-state and unlock-cache posture more clearly in the TUI, including a visible locked/unlocked indicator and an explicit relock action where the local backend can support it safely.
- [ ] Extend `scripts/doctor.py` and runtime preflight checks to validate wallet `chain_id` against node-side chain expectations when both configs are present, and to surface insecure or surprising local-account runtime choices earlier.
- [ ] Add a minimal local audit trail for operators that records request id, payload hash, backend id, timestamps, and terminal decision without logging plaintext passwords, private keys, or full signed payloads.
- [ ] Add acceptance coverage for the new local prompt surface: approve, reject, password cancel, daemon restart recovery, stale terminal cleanup, and side-by-side validation that `tty_prompt` still works.
- [ ] Update the plugin README and wallet backend docs so `tty_prompt` becomes the conservative path, `desktop_prompt` becomes the integrated TUI path, and the runtime-manager story explains that `starmaskd` remains a coordinator rather than a signer.
- [ ] Evaluate a later phase for embedding `starmaskd` directly inside the TUI process only after the child-process runtime-manager path is stable and shutdown semantics are explicit.
