# Starcoin MCP

This repository groups Starcoin-related MCP projects in one place.

Repository-level materials live under:

- `docs/architecture/`
- `shared/`

Key architecture documents:

- `docs/architecture/overview.md`
- `docs/architecture/host-integration.md`
- `docs/architecture/design-closure-plan.md`
- `docs/architecture/deployment-model.md`

## Subprojects

- `starmask-mcp/`
  - local wallet-facing MCP entrypoint design
  - includes the interface draft for `starmask-mcp`, `starmaskd`, and the Chrome extension bridge
- `starcoin-node-mcp/`
  - chain-facing Starcoin MCP design
  - includes the interface draft for chain queries, transaction preparation, simulation, and signed transaction submission

## Status

Initial project layout with wallet-facing and chain-facing subprojects.
