# Starcoin MCP

This repository contains Starcoin-related MCP projects, with current emphasis on `starmask-runtime` and `starcoin-node`.

These instructions apply to the whole repository unless a subproject adds stricter local guidance.

## Scope

- Keep repository-level architecture and shared contracts authoritative.
- Do not bypass `shared/` definitions by inventing parallel lifecycle or error vocabularies inside subprojects.
- When changing protocol behavior, update the relevant documents in `docs/architecture/`, `shared/`, and the affected subproject docs in the same change.

## Rust Conventions

- Prefer crate names with a repository-consistent prefix.
  - For the wallet-facing implementation, prefer `starmask-...`.
- Default to `#![forbid(unsafe_code)]` in Rust crates and module files that define core runtime
  behavior.
- If `unsafe` is ever unavoidable, stop adjacent feature work and handle that unsafe code first in
  one focused pass: isolate it, document why it is required, add the narrowest possible tests, and
  only then continue generating surrounding code.
- When using `format!` and variables can be inlined into `{}`, inline them.
- Always collapse `if` statements when doing so improves readability.
- Prefer method references over redundant closures when they are equally clear.
- Prefer exhaustive `match` statements over wildcard arms when practical.
- Avoid adding or preserving stringly-typed state in core Rust logic when a typed enum or newtype is more appropriate.

## API Design

- Avoid bool or ambiguous `Option` parameters that create hard-to-read callsites such as `foo(false)` or `bar(None)`.
- Prefer enums, named methods, newtypes, or dedicated request structs when they keep callsites self-documenting.
- If an opaque positional literal is still necessary, use the `argument_comment_lint` convention:
  - add an exact `/*param_name*/` comment before ambiguous positional literals such as `None`, booleans, and numeric literals
  - the comment must exactly match the callee signature

## Project Structure

- Keep transport adapters thin.
- Keep lifecycle policy and persistence logic out of MCP-specific adapters.
- Prefer mature MCP SDKs such as the official Rust SDK `rmcp` at MCP server boundaries.
- Do not let MCP SDK types leak into shared protocol crates, core domain crates, persistence layers, or bridge layers.
- Prefer adding a new module over continuing to grow an already large file.
- Target Rust modules under roughly 500 lines excluding tests.
- If a file approaches roughly 800 lines, strongly prefer extracting new functionality into a new module unless there is a documented reason not to.

## Documentation Sync

- If you change an API, protocol, lifecycle rule, config surface, or persistence behavior, update the corresponding docs in the same change.
- If you change repository-wide engineering conventions, layering rules, SDK usage policy, or workflow expectations, update `AGENTS.md` in the same change.
- Do not update `AGENTS.md` for incidental implementation details that do not change project guidance.
- For `starmask-runtime`, the following docs are part of the implementation contract:
  - `starmask-runtime/docs/starmask-interface-design.md`
  - `starmask-runtime/docs/security-model.md`
  - `starmask-runtime/docs/daemon-protocol.md`
  - `starmask-runtime/docs/native-messaging-contract.md`
  - `starmask-runtime/docs/persistence-and-recovery.md`
  - `starmask-runtime/docs/configuration.md`
  - `starmask-runtime/docs/approval-ui-spec.md`
  - `starmask-runtime/docs/testing-and-acceptance.md`
  - `starmask-runtime/docs/rust-implementation-strategy.md`

## Testing

- Prefer asserting equality on whole objects instead of checking fields one by one when that remains readable.
- Prefer `pretty_assertions::assert_eq` in Rust tests.
- Avoid mutating process environment in tests when a parameterized dependency or config object can be passed instead.
- For protocol-heavy code, prefer fixture or snapshot-style coverage for:
  - JSON-RPC payloads
  - Native Messaging payloads
  - config serialization
  - MCP tool results

## Rust Workflow

- Run `cargo fmt` automatically after Rust code changes.
- Run tests for the affected crate or binary first.
- Do not default to `--all-features` for routine local runs.
- Ask before running a full workspace test suite if the change is broad or expensive.
- Name new task branches as `codex/<kind>/<topic>` by default.
- Prefer these branch kinds:
  - `feat` for new behavior or user-visible capability
  - `fix` for bug fixes or regression corrections
  - `refactor` for behavior-preserving structural changes
  - `chore` for small maintenance work when no more specific kind fits
  - `docs` for documentation-led changes
  - `test` for test-only or test-led changes
- Do not use `chore` when `feat`, `fix`, `refactor`, `docs`, or `test` describes the branch more precisely.
- Default to a dedicated git worktree for each new task branch, created from the latest relevant `main` branch.
- Reuse the current worktree only when the user explicitly asks for that, or when the task is to continue an already-existing in-place dirty worktree.
- If you continue in a dirty worktree instead of creating a worktree, state that reason before making substantial changes.
- After reaching a verified milestone, create a commit promptly instead of leaving large validated changes uncommitted.
- Push promptly when the user asks, when the branch has reached a shareable checkpoint, or when remote backup materially reduces risk.
- Do not push half-finished or unverified changes just to satisfy a cadence rule.

## Review Workflow

- Fix review findings by issue class, not only by the commented line.
- When a review comment identifies a bug pattern, audit the surrounding module for the same pattern before pushing.
- For RPC and protocol code, explicitly re-check adjacent logic for:
  - malformed payload handling
  - fail-closed versus silent fallback behavior
  - degradable versus hard-fail dependency boundaries
  - error-code semantics and retryability
  - secret exposure in logs, debug helpers, or config accessors
  - exhaustive handling when enums or shared contracts grow
- Do not push a review-fix round until the touched area has been re-reviewed locally with the above checklist and the relevant tests pass.
- After each push, re-read the latest review summary instead of assuming resolved threads mean the PR is review-clean.
- If an automated reviewer keeps surfacing adjacent issues, stop the line-by-line patching pattern and do a module-level sweep before the next push.

## Starmask Runtime-Specific Rules

- `Starmask` extension is the only signing authority.
- `starmaskd` owns lifecycle state transitions.
- `starmask-runtime` is a host-adapter contract, not a policy engine.
- `rmcp` belongs at the MCP shim boundary, not inside `starmaskd` or `starmask-core`.
- `starmask-native-host` is a transport shim, not a wallet runtime.
- After `request.presented`, recovery is same-instance only.
- Unsupported payloads must be rejected, not blind-signed.
- Signed results use bounded multi-read retention in the first implementation.

## Non-Goals

- Do not import repository-specific conventions from unrelated projects unless they are adapted to this repository first.
- Do not add build-system-specific guidance such as Bazel rules unless this repository actually adopts that tooling.
