# Starcoin MCP Architecture Overview

## Purpose

This document describes how the `starcoin-mcp` repository is organized at the project level.

The repository hosts multiple Starcoin-related runtimes, workflow plugins, and future host-adapter
designs under one umbrella, while keeping:

- project-specific interfaces close to each subproject
- shared protocol contracts in one place
- cross-project architecture documents at the repository level

## Current Repository Reality

The current repository should be read in two layers:

1. implemented runtimes and tools
2. logical host-adapter boundaries that may be realized by future or external adapters

Implemented local runtimes and tools today:

- `starcoin-node-cli`
- `starmaskd`
- `local-account-agent`
- `starmask-native-host`
- `starmaskctl`
- workflow scripts under `plugins/starcoin-transfer-workflow/`

Important current facts:

- the repository no longer ships in-tree stdio adapters for `starcoin-node` or
  `starmask-runtime`
- `starmask-runtime` remains the logical wallet-facing host boundary used by the design set
- `starcoin-node` remains the logical chain-facing host boundary used by the design set
- the wallet side is no longer extension-only because `local_account_dir` is implemented

## Repository Layers

The repository is organized into three logical layers:

1. `docs/architecture/`
2. `shared/`
3. subprojects such as `starmask-runtime/`, `starcoin-node/`, and workflow plugins

## 1. `docs/architecture/`

This directory holds repository-level design documents.

It answers questions such as:

- what major subprojects exist in this repository
- how they relate to each other
- how local hosts interact with chain-facing and wallet-facing boundaries
- what the trust boundaries are
- how runtimes are deployed and supervised

This layer is explanatory rather than normative at the API level.

Typical contents:

- overall architecture overview
- host integration model
- deployment model
- library packaging model
- runtime supervision TUI design
- architecture closure plan

## 2. `shared/`

This directory holds reusable contracts and conventions shared by multiple subprojects.

It should contain materials that are intended to be referenced, reused, or implemented
consistently across more than one subproject.

Typical contents:

- shared schemas
- shared protocol conventions
- shared error codes
- shared status enums
- versioning rules
- security baseline documents
- example payloads

This layer is normative. If multiple subprojects use a common request lifecycle or error taxonomy,
the canonical definition should live here.

## 3. Subprojects

Each subproject lives in its own directory.

Examples:

- `starmask-runtime/`
- `starcoin-node/`
- `plugins/starcoin-transfer-workflow/`

Each subproject should contain:

- a local README
- project-specific interface design docs
- implementation notes if needed
- implementation-language constraints when the first release is intentionally language-specific

Subprojects should not redefine shared protocol concepts unless they are intentionally
project-local.

## Current Project Structure

```text
starcoin-mcp/
  README.md
  docs/
    architecture/
      deployment-model.md
      design-closure-plan.md
      host-integration.md
      library-packaging.md
      overview.md
      runtime-supervision-tui.md
  shared/
    protocol/
      error-codes.md
      request-lifecycle.md
    schemas/
      unsigned-transaction-envelope.schema.json
      wallet-sign-request.schema.json
      wallet-sign-result.schema.json
  starcoin-node/
    README.md
    crates/
      starcoin-node-cli/
      starcoin-node-core/
      starcoin-node-rpc/
      starcoin-node-types/
    docs/
      configuration.md
      deployment-model.md
      design-closure-plan.md
      rpc-adapter-design.md
      rust-implementation-strategy.md
      security-model.md
      starcoin-node-interface-design.md
      testing-and-acceptance.md
  starmask-runtime/
    README.md
    crates/
      starmask-core/
      starmask-local-account-agent/
      starmask-native-host/
      starmask-types/
      starmaskctl/
      starmaskd/
    docs/
      configuration.md
      daemon-protocol.md
      starmask-interface-design.md
      wallet-backend-configuration.md
      unified-wallet-coordinator-evolution.md
      ...
```

## Design Intent

The current structure is designed so that:

- shared protocol and lifecycle contracts stay centralized
- chain-side and wallet-side trust boundaries stay separate
- repository-local workflow tooling can evolve without pretending it is the same thing as the
  chain or wallet runtime
- new operator tooling such as a runtime supervision TUI can be added without moving signing or
  chain logic into a merged binary

## Design Status

The repository now contains:

- a real chain-side Rust library plus CLI implementation in `starcoin-node/`
- a real multi-backend wallet runtime in `starmask-runtime/`
- logical host-adapter designs that remain useful even though the in-tree adapter crates were
  removed
- enough architecture detail to begin implementing a runtime supervision TUI as a separate
  operator-facing application
