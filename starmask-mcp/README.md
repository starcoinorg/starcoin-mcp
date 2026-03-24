# Starmask MCP

This subproject contains the interface design draft for `starmask-mcp` under the following assumptions:

- `starmask-mcp` is a local MCP server exposed to MCP hosts such as Claude Code and Codex.
- `starmaskd` is a long-lived local daemon.
- Starmask is implemented as a Chrome extension and acts as the signing backend and approval UI.
- The MCP host entrypoint is `starmask-mcp`, not the Chrome extension directly.

## Contents

- `docs/starmask-mcp-interface-design.md`: detailed interface design draft

## Status

Draft for review as a subproject of `starcoin-mcp`.
