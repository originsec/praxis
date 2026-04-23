# MCP Server

Praxis exposes its capabilities via a [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server over SSE transport. This server is built into the Praxis service and provides tool access for both external AI agents and the built-in Orchestrator.

## Overview

The MCP server serves two purposes:

1. **Orchestrator backend** — The built-in Orchestrator connects to the MCP server as a client to access all Praxis tools. This is how the Orchestrator coordinates operations across nodes and agents.

2. **External AI agent integration** — Any MCP-compatible AI assistant (Claude Code, Cursor, Windsurf, etc.) can connect to the same server to control Praxis programmatically.

## Enabling the MCP Server

The MCP server is controlled via service settings:

1. Go to **Settings** > **MCP Server** (web UI or CLI Settings window)
2. Toggle **Enable** to turn on the server
3. Configure the port (default: `8585`)

The SSE endpoint is available at `http://localhost:{port}/sse`.

> **Note:** The MCP server must be enabled for the Orchestrator to function. If disabled, the Orchestrator will display an error directing you to enable it.

When running with Docker, port 8585 is exposed by default. To use a different port:

```bash
PRAXIS_MCP_PORT=9090 docker compose up --build
```

Then update the port in **Settings** > **MCP Server** to match.

## AI Agent Integration

MCP-compatible AI assistants can connect to the Praxis SSE server to control the entire C2 network. This enables AI agents to discover nodes, run recon, create sessions, execute operations, and search traffic — all through structured tool calls.

### Configuration

For any MCP-compatible client, point it at the SSE endpoint:

```json
{
  "mcpServers": {
    "praxis": {
      "url": "http://localhost:8585/sse"
    }
  }
}
```

Adjust the host and port to match your deployment. For remote deployments, ensure the MCP port is accessible from the client machine.

## Available Tools

The MCP server exposes the following tools:

### Node Management

- `node_list` — List all connected nodes (includes privileged status)
- `node_select` — Get details for a specific node
- `node_reset` — Reset a node (cancel operations, close sessions, re-register)

### Agent Management

- `agent_list` — List agents on a node
- `agent_update` — Request agent info refresh

> Agents are selected per-session rather than per-node. `session_create`
> and the recon tools each take an `agent` parameter, so the same node
> can run concurrent sessions against different agents.

### Reconnaissance

All recon tools take a `node` prefix and an `agent` short-name.

- `recon_run` — Run static reconnaissance (`node`, `agent`)
- `recon_run_semantic` — Run semantic reconnaissance, includes internal tools (`node`, `agent`)
- `recon_list` — List stored recon data (`node`, `agent`, `section` = all/sessions/tools/projects/configs)
- `recon_config_read` — Read config file content discovered by recon (`node`, `agent`, optional `path`)
- `recon_session_read` — Read session file content (`node`, `agent`, optional `path`)
- `recon_config_grep` — Grep config files with regex (`node`, `agent`, `pattern`, optional `paths`)
- `recon_session_grep` — Grep session files with regex (`node`, `agent`, `pattern`, optional `paths`)
- `write_file` — Write file content

### Sessions

- `session_create` — Create a new ACP session (`node`, `agent`, optional `project`, `yolo`). Returns a `session_id`.
- `session_prompt` — Send a prompt to a session (`node`, `session_id`, `prompt`)
- `session_close` — Close a session (`node`, `session_id`)

### Operations & Chains

- `op_available` — List available operations and chains
- `op_definition` — Show the full definition of an operation or chain
- `op_run` — Run an operation or chain
- `op_info` — Show full info for an operation or chain execution
- `op_cancel` — Cancel a running operation or chain execution
- `op_list` — List tracked operations and chain executions

### Chain Triggers

- `trigger_list` — List all chain triggers
- `trigger_create` — Create a trigger for a chain
- `trigger_delete` — Delete a trigger by ID prefix
- `trigger_toggle` — Enable or disable a trigger by ID prefix

### Traffic

- `traffic_search` — Search intercepted traffic
