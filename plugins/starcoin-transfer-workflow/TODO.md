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
