# Repository Architecture Closure Plan

## Purpose

This document defines what must be true before coding the next cross-project orchestration feature:
an operator-facing runtime supervision TUI.

Status note:

- this repository already contains real implementations for `starcoin-node-cli`, `starmaskd`, and
  `local-account-agent`
- this is no longer a pre-implementation closure plan for the old in-tree `starmask-runtime`
  adapter
- the remaining architecture gap is cross-project runtime supervision, not missing core wallet or
  chain semantics

## Closed Facts

The following repository facts are now treated as closed inputs to the TUI design:

1. The repository does not currently ship in-tree stdio adapters for `starcoin-node` or
   `starmask-runtime`.
2. The current chain-side executable is `starcoin-node-cli`, and it is intentionally short-lived.
3. The current wallet-side runtime is `starmaskd` plus backend helper processes.
4. `local_account_dir` is real code, not a future-only design branch.
5. Extension-backed wallets remain supported, but Chrome owns `starmask-native-host` lifecycle.
6. The current wallet runtime is Unix-only; Windows support remains future design work.
7. A TUI may supervise processes, but it must not become a signer, a chain adapter, or a merged
   trust domain.

## Required Document Set

The following document set must stay aligned before TUI coding begins:

1. `docs/architecture/overview.md`
2. `docs/architecture/host-integration.md`
3. `docs/architecture/deployment-model.md`
4. `docs/architecture/library-packaging.md`
5. `docs/architecture/runtime-supervision-tui.md`
6. `starcoin-node/docs/deployment-model.md`
7. `starcoin-node/docs/configuration.md`
8. `starmask-runtime/docs/configuration.md`
9. `starmask-runtime/docs/wallet-backend-configuration.md`
10. `starmask-runtime/docs/starmask-interface-design.md`
11. `starmask-runtime/docs/security-model.md`
12. `starmask-runtime/docs/daemon-protocol.md`
13. `starmask-runtime/docs/native-messaging-contract.md`
14. `starmask-runtime/docs/persistence-and-recovery.md`
15. `starmask-runtime/docs/approval-ui-spec.md`
16. `starmask-runtime/docs/testing-and-acceptance.md`
17. `starmask-runtime/docs/rust-implementation-strategy.md`

## TUI Closure Checklist

The TUI design is ready for coding only when all of the following are explicit:

1. The TUI launches `starmaskd` exactly once per selected runtime profile.
2. The TUI launches one `local-account-agent` per enabled `local_account_dir` backend and never
   guesses backend identity.
3. The TUI never tries to keep `starmask-native-host` running outside Chrome ownership.
4. The TUI treats node-side service management as optional and distinct from `starcoin-node-cli`.
5. The TUI uses readiness checks that match current process reality:
   - daemon socket plus daemon health for `starmaskd`
   - wallet-instance registration for local agents
   - RPC health for any managed node-side service
6. The TUI has a clear ownership rule for logs, pid metadata, and process re-attachment after a
   crash or restart.
7. The TUI reuses existing config paths and config validation rules instead of inventing parallel
   chain and wallet configuration semantics.
8. The TUI keeps current trust boundaries intact:
   - signing still happens only in the selected wallet backend
   - chain submission still happens through `starcoin-node`
   - the TUI never handles passwords or private keys as application state

## Current Architecture Drift That Needed Closing

The previous top-level document set had four material mismatches with the current repository:

1. it still described in-tree `starmask-runtime` and `starcoin-node` stdio binaries as if they
   were present
2. it treated the wallet side as extension-only and ignored the implemented `local_account_dir`
   path
3. it implied `starcoin-node` was still a server-shaped deployment even though the current code is
   centered on `starcoin-node-cli`
4. it described Windows wallet-runtime transport as first-implementation behavior even though the
   current daemon is Unix-only

These mismatches are now considered closed by the updated document set.

## Coding-Ready Decision

The repository is ready to start TUI implementation when the first pass stays within this scope:

- start, stop, and observe `starmaskd`
- start, stop, and observe enabled `local_account_dir` backends
- show extension-backend manifest and connection health without supervising Chrome-owned processes
- optionally start and observe one node-side service
- reuse `starcoin-node-cli` as an on-demand command rather than a daemon

Out of scope for the first TUI pass:

- reintroducing in-tree MCP adapters
- embedding `starmaskd` or `starcoin-node-cli` as in-process libraries by default
- browser automation
- Windows wallet-runtime support
- TUI-managed signing flows or chain-call orchestration

## Closure Status

With the updated top-level and lower-level documents, the repository is now at the point where the
runtime supervision TUI can move from architecture work into implementation.
