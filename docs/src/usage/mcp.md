# MCP Server

Praxis exposes its capabilities via a [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server over streamable-HTTP transport. This server is built into the Praxis service and provides tool access for both external AI agents and the built-in Orchestrator.

## Overview

The MCP server serves two purposes:

1. **Orchestrator backend** — The built-in Orchestrator connects to the MCP server as a client to access all Praxis tools. This is how the Orchestrator coordinates operations across nodes and agents.

2. **External AI agent integration** — Any MCP-compatible AI assistant (Claude Code, Cursor, Windsurf, etc.) can connect to the same server to control Praxis programmatically.

## Enabling the MCP Server

The MCP server is controlled via service settings:

1. Open **Settings** (`Ctrl+S`) > **Service** in the praxis TUI
2. Toggle **MCP Server** to turn it on
3. Configure the port (default: `8585`)

The MCP endpoint is available at `http://localhost:{port}/mcp`.

> **Note:** The MCP server must be enabled for the Orchestrator to function. If disabled, the Orchestrator will display an error directing you to enable it.

When running with Docker, port 8585 is exposed by default. To use a different port:

```bash
PRAXIS_MCP_PORT=9090 docker compose up --build
```

Then update the port under **Settings** > **Service** to match.

## AI Agent Integration

MCP-compatible AI assistants can connect to the Praxis MCP server to control the entire C2 network. This enables AI agents to discover nodes, run recon, create sessions, execute operations, and search traffic — all through structured tool calls.

### Configuration

For any MCP-compatible client, point it at the MCP endpoint:

```json
{
  "mcpServers": {
    "praxis": {
      "url": "http://localhost:8585/mcp"
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
- `session_list` — Enumerate active ACP sessions on a node (`node`). Returns each session's id (full + short), title, and cwd.
- `session_prompt` — Send a prompt to a session (`node`, `session_id`, `prompt`)
- `session_cancel` — Cancel an active prompt without closing its session (`node`, `session_id`). Cancellation bypasses long-running tool requests.
- `session_close` — Close a session (`node`, `session_id`)

### Operations & Chains

- `op_available` — List available operations and chains
- `op_definition` — Show the full definition of an operation or chain
- `chain_create` — Create a reusable linear chain from existing operations
- `op_create` — Create a new operation and persist it to the library. **Warning:** this writes a real, reusable operation — getting the prompt wrong can cause unintended agent behavior on target systems the next time it runs.
- `op_run` — Run an operation or chain. Returns the existing short ID plus `id_full` for operations or `execution_id_full` for chains. Chain launches use request-correlated replies, so simultaneous launches receive their own execution IDs. Preserve full IDs to track and cancel child work reliably.
- `op_info` — Show full info for an operation or chain execution
- `op_cancel` — Cancel a running operation or chain execution. Accepts full IDs as well as prefixes and bypasses long-running tool requests; the execution must be present in the client cache (`op_list` refreshes it).
- `op_delete` — Permanently remove an operation definition. **Warning:** this is destructive and cannot be undone.
- `op_list` — List tracked operations and chain executions

### Chain Triggers

- `trigger_list` — List all chain triggers
- `trigger_create` — Create a trigger for a chain
- `trigger_delete` — Delete a trigger by ID prefix
- `trigger_toggle` — Enable or disable a trigger by ID prefix

To automate an operation that is not in a chain yet, create the chain first:

```json
{
  "name": "CI/CD on connect",
  "description": "Run CI/CD discovery whenever a node registers",
  "operations": ["custom::cicd"],
  "category": "custom",
  "timeout": 600
}
```

`chain_create` resolves each operation by full name, short name, or display name
and connects them in the given order between a manual start element and a
termination element. The resulting chain can then be passed by name to
`trigger_create`.

`trigger_create` accepts `scheduled`, `intercept_match`, and `new_node` trigger
configurations. For example, an Orchestrator can make an existing chain run
whenever a node registers:

```json
{
  "chain": "CI/CD",
  "trigger": {
    "type": "new_node"
  },
  "target": {
    "agent_short_names": ["codex"],
    "include_triggering_node": true
  }
}
```

New-node triggers run after a 10-second discovery delay. Target node IDs must be
full IDs from `node_list`; leaving `node_ids` empty targets all registered nodes.
For event triggers, `include_triggering_node` ensures that the node which caused
the event is included even when an explicit node list would otherwise exclude it.

`agent_short_names` matching is exact and case-sensitive against each node's
discovered agents — a name that doesn't match anything silently resolves to
zero targets, and the trigger fires as a no-op. `trigger_create`'s response
includes a `warning` field when a requested short name doesn't currently
match any connected agent; confirm the real short name with `node_list`/
`agent_list` rather than guessing.

Trigger mutations wait for the service to confirm the database operation before
returning success. `trigger_create` returns the created trigger's full ID as
`id` (and its short form as `id_short`), while `trigger_delete` and
`trigger_toggle` return only after the requested trigger is deleted or updated.
Service-side validation and database errors are returned as MCP tool errors.

### Traffic

- `traffic_search` — Search intercepted traffic

## Durable mission tool

The `mission` tool accepts a tagged command object. It returns immediately after
the service accepts the command; creating a mission does not hold the MCP request
open for the duration of the work. Disconnecting the MCP client leaves it running.

```json
{
  "action": "create",
  "spec": {
    "name": "Configuration audit",
    "objective": "Inspect selected agent configuration and cite evidence",
    "scope": "Read-only inspection of node abc",
    "budget": {"max_iterations": 30, "max_tokens": 200000, "max_active_seconds": 1800},
    "verify": true
  }
}
```

Supported actions:

| Action | Fields |
|--------|--------|
| `create` | `spec`: name, objective, optional scope/model/budget/verify |
| `update` | `id`, `changes`: any name/objective/scope/model/clear_model/budget/verify fields; `budget` may contain only changed limits; pause running work first |
| `list` | None |
| `get` | `id` |
| `events` | `id`, optional `after` event sequence (default 0) |
| `steer` | `id`, `instruction` |
| `pause` | `id` |
| `resume` | `id`, optional `reconciliation`, optional `budget` with any total limits to change |
| `cancel` | `id` |

For example, after pausing a mission, change its scope and token total without
re-entering the other limits:

```json
{"action":"update","id":"MISSION_ID","changes":{"scope":"Local lab only","budget":{"max_tokens":800000}}}
```

Responses are tagged as `mission`, `missions`, or `events`, with their payload in
`data`. Poll `events` with the last observed sequence for incremental progress,
and `get` for status, checkpoint, budget usage, pending actions, and results.
Event requests return at most 500 records; advance `after` to fetch further pages.
Pause/cancel remain independent requests while other clients poll. If a request
times out, inspect mission records before repeating a mutation. Recovery from
uncertain side effects requires an operator reconciliation; see
[Durable missions](./missions.md).
