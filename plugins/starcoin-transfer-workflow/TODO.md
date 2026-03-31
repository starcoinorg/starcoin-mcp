# TODO

## vm_profile follow-ups

- [ ] Document more explicitly that `vm_profile = "auto"` is not a one-time VM selection. It uses per-call fallback rules, and different RPC categories do not all prefer the same path.
- [ ] Expose `vm_profile` as a first-class plugin/runtime choice for transfer workflows. Today the skill and `prepare_transfer` tool inputs do not let a caller pick the profile per transfer.
- [ ] Add a `--vm-profile` option to `scripts/run_transfer_test.py`. The test script currently writes `vm_profile = "auto"` into the generated node-mcp config.
- [ ] Decide whether the project needs a strict `vm1_only` mode. Current choices are `auto`, `vm2_only`, and `legacy_compatible`; `legacy_compatible` still falls back to the newer path when legacy methods are unavailable.
- [ ] If per-transfer profile selection is needed, choose an implementation direction. Candidates:
  - [ ] Profile-aware server startup/config switching before the MCP session starts.
  - [ ] Separate node-mcp instances for legacy-compatible and vm2-only paths.
  - [ ] Extend the plugin surface so a transfer flow can target a selected node-mcp instance explicitly.
- [ ] Add a short user-facing note in the plugin docs explaining which methods are legacy-first, vm2-first, or vm2-only so operators can predict `auto` behavior more easily.

## transfer confirmation follow-ups

- [ ] Change the default post-submit behavior so a submitted transaction waits for at least 1 additional block before the workflow reports success. The current flow treats `watch_transaction` as optional, which makes "submitted" and "confirmed" too easy to conflate.
- [ ] Add a user-facing transfer option for block-based confirmation depth, for example `confirm_blocks` or `min_confirmed_blocks`, so callers can request more than the default 1 block when they need stronger confirmation.
- [ ] Prefer `watch_transaction` as the single source of truth for confirmation-depth waiting. It should accept the target confirmation depth and own the block-based success criteria.
- [ ] Expose one higher-level confirmation setting in the transfer skill/plugin and map it onto `watch_transaction`, so users do not need to reason about tool boundaries during a transfer.
- [ ] Keep `submit_signed_transaction` focused on submission and immediate execution status. If it offers a blocking convenience mode, it should reuse `watch_transaction` semantics instead of defining a second confirmation model.
- [ ] Define the confirmation semantics precisely in docs and tool output: whether the count means the inclusion block only, inclusion plus N more blocks, or a minimum chain height delta observed after inclusion.
- [ ] Update the transfer skill, README, and `scripts/run_transfer_test.py` so the documented default matches the runtime behavior and the CLI test can exercise both the default 1-block wait and a user-specified confirmation depth.

## .mcp.json simplification follow-ups

- [ ] Simplify `plugins/starcoin-transfer-workflow/.mcp.json` so the default launcher assumes `starcoin-node-mcp` and `starmask-mcp` binaries are already installed on `PATH` and relies on each tool's default config and socket locations.
- [ ] Remove environment-variable-driven launcher indirection from the default `.mcp.json` path, including workspace-root, manifest, binary, and runtime-metadata discovery logic that is only needed for source-tree or ad hoc local setups.
- [ ] If source-tree fallback remains useful for development, move it behind an explicit dev-only path or separate example instead of keeping the default `.mcp.json` startup flow shell-heavy.
- [ ] Update `scripts/doctor.py` so it validates the simplified default startup contract directly: binaries available on `PATH`, default config files present, and default socket path reachable, without depending on the same env-heavy launcher assumptions as `.mcp.json`.
- [ ] Review `scripts/doctor.py` output and remediation text so normal users are guided toward default install locations and `PATH` setup first, while any remaining non-default dev overrides are documented separately.
- [ ] Update `README.md` so the normal setup story matches the simplified `.mcp.json` and `doctor.py` contract.

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
