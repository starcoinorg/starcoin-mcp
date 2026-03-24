# Starcoin Node MCP

This subproject contains the interface design draft for the chain-facing Starcoin MCP server.

The intended role of `starcoin-node-mcp` is:

- chain and node data access
- transaction preparation
- transaction simulation
- submission of already signed transactions

It does not hold private keys and does not perform wallet signing.

## Contents

- `docs/starcoin-node-mcp-interface-design.md`: detailed interface design draft

## Status

Draft for review as a subproject of `starcoin-mcp`.
