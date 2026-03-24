# Starcoin MCP Architecture Overview

## Purpose

This document describes how the `starcoin-mcp` repository is organized at the project level.

The repository is intended to host multiple Starcoin-related MCP projects under one umbrella, while keeping:

- project-specific interfaces close to each subproject
- shared protocol contracts in one place
- cross-project architecture documents at the repository level

## Repository Layers

The repository is organized into three logical layers:

1. `docs/architecture/`
2. `shared/`
3. subprojects such as `starmask-mcp/` and `starcoin-node-mcp/`

## 1. `docs/architecture/`

This directory holds repository-level design documents.

It answers questions such as:

- what major MCP projects exist in this repository
- how they relate to each other
- how MCP hosts interact with chain-facing and wallet-facing MCP servers
- what the trust boundaries are
- how the system is deployed

This layer is explanatory rather than normative at the API level.

Typical contents:

- overall architecture overview
- host integration model
- deployment model
- design closure plan
- signing architecture
- trust boundaries

## 2. `shared/`

This directory holds reusable contracts and conventions shared by multiple subprojects.

It should contain materials that are intended to be referenced, reused, or implemented consistently across more than one subproject.

Typical contents:

- shared schemas
- shared protocol conventions
- shared error codes
- shared status enums
- versioning rules
- security baseline documents
- example payloads

This layer is normative. If multiple subprojects use a common request lifecycle or error taxonomy, the canonical definition should live here.

## 3. Subprojects

Each MCP project lives in its own directory.

Examples:

- `starmask-mcp/`
- `starcoin-node-mcp/`

Each subproject should contain:

- a local README
- project-specific interface design docs
- implementation notes if needed
- implementation-language constraints when the first release is intentionally language-specific

Subprojects should not redefine shared protocol concepts unless they are intentionally project-local.

## Current Project Structure

```text
starcoin-mcp/
  README.md
  docs/
    architecture/
      design-closure-plan.md
      deployment-model.md
      overview.md
      host-integration.md
  shared/
    protocol/
      error-codes.md
      request-lifecycle.md
    schemas/
      unsigned-transaction-envelope.schema.json
      wallet-sign-request.schema.json
      wallet-sign-result.schema.json
  starmask-mcp/
    README.md
    docs/
      approval-ui-spec.md
      configuration.md
      daemon-protocol.md
      native-messaging-contract.md
      persistence-and-recovery.md
      security-model.md
      starmask-mcp-interface-design.md
      testing-and-acceptance.md
  starcoin-node-mcp/
    README.md
    docs/
      configuration.md
      deployment-model.md
      design-closure-plan.md
      rpc-adapter-design.md
      rust-implementation-strategy.md
      security-model.md
      starcoin-node-mcp-interface-design.md
      testing-and-acceptance.md
```

## Design Intent

The current structure is designed so that future MCP projects can be added without forcing repeated copies of:

- request status definitions
- common error codes
- security assumptions
- repository-level architecture decisions

## Design Status

The repository now contains:

- an implementation-oriented design set for `starmask-mcp`
- an implementation-oriented design set for `starcoin-node-mcp`

For the current first-release design set, the local MCP binaries are specified with Rust-first implementation constraints in their subproject documents.

Any implementation work should preserve those contracts rather than reopening them ad hoc.
