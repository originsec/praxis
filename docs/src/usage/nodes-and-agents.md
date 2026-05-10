# Nodes & Agents

Understanding how Praxis organizes nodes and agents is key to using the platform effectively.

## Nodes

A node represents a system running the Praxis node binary. When you deploy a node to a target machine, it:

1. Connects to RabbitMQ
2. Registers with the service
3. Fingerprints installed AI agents
4. Begins listening for commands

### Node Identity

Each node gets a unique ID generated on first run. This ID persists across restarts, so the service recognizes when a node reconnects.

The node also reports:
- **Machine name** - hostname of the system
- **OS details** - operating system and version
- **Agent list** - discovered AI agents
- **Privileged status** - whether the node is running as root/admin

### Superuser Mode

When the node runs as root, it can operate as different users based on the selected working directory. Selecting a working directory owned by another user will cause agent sessions to run as that user (with the appropriate `HOME` environment variable set).

**Note**: Full superuser support is still under development. Users may notice unexpected behaviour when running sessions as different users from a root node. If you encounter issues, try running the node as the target user directly instead.

### Privileged Status

Each node reports whether it is running with elevated privileges. On Linux/macOS this means running as root (UID 0); on Windows this means running as an elevated administrator.

Privileged nodes display a **ROOT** badge in the praxis TUI. Some features — particularly interception methods that modify system-level configuration (VPN, Hosts, TPROXY) — require elevated privileges. The TUI disables the intercept Enable button on non-privileged nodes.

### Node List

Open the **Nodes** window (`Ctrl+L`) in the praxis TUI to see all connected nodes. Select a node to view its details and agents.

### Bridge Nodes

In addition to deployed nodes, Praxis supports **bridge nodes** -- virtual nodes created when Claude Code connects directly to the service using the Claude Bridge. Bridge nodes appear in the TUI alongside regular nodes but have some differences:

- They only support sessions (no interception, recon, or terminal)
- They are ephemeral -- they disappear when Claude disconnects
- Sessions are automatically active in YOLO mode
- The node type shows as `claude-ccrv1` or `claude-ccrv2`

Bridge nodes are created by enabling the Claude Bridge in Settings and launching Claude Code with the appropriate environment variables. See [Claude Bridge](../connectors/claude-bridge.md) for setup details.

### Removing Nodes

If a node disconnects and you want to remove it from the list, click the remove button. This clears the node from the service's tracking. If the node reconnects, it will appear again.

### Resetting Nodes

You can reset a node to cancel all in-flight operations and return it to a clean state. Reset will:

- Cancel all running transactions (prompts, recon, etc.)
- Drop every live ACP session and its per-session Lua VM
- Close any terminal session
- Disable interception and restore system settings
- Re-register the node with the service

Use the reset button (↻) in the node card header, the CLI command `node reset <id>`, or the MCP tool `node_reset`. The node briefly goes offline during reset and comes back with fresh state. Clients drop their local entries for the reset node immediately and re-pull `session/list` after a short grace period so the Active Sessions overlay reflects reality.

## Agents

Agents are the AI assistants detected on each node. When a node fingerprints successfully, you'll see agents like:

- **Claude Code** - Anthropic's CLI assistant
- **Claude Desktop** - Anthropic's desktop app (Windows only)
- **Codex CLI** - OpenAI's CLI assistant
- **Cursor Agent** - Cursor's background agent CLI (Linux only)
- **Gemini CLI** - Google's CLI assistant
- **M365 Copilot** - Microsoft 365 Copilot (Windows only)

### Agent Selection

Click an agent to focus operations on it — recon targets that agent,
actions in the agent's card (config read/write, session create) route to
that agent. A node can host concurrent sessions across any combination
of its agents; the focus is purely a UI convenience, not a routing
constraint. Recon is agent-scoped (`_praxis/recon` is called with the
agent's `short_name`), and each session explicitly names its connector
via `_meta.praxis.connector` on `session/new`.

### Agent States

**Fingerprinted** — the agent was detected but no session is open.

**Session Active** — one or more live sessions exist. The card shows a
`LIVE` indicator and, when applicable, a `YOLO` tag for auto-approve
sessions. The Sessions panel lists each live session with resume /
discard controls.

## Working with Nodes and Agents

### Typical Workflow

1. **Deploy node** to target system
2. **Select node** in the praxis TUI's Nodes window (`Ctrl+L`)
3. **Check agents** that were fingerprinted
4. **Select an agent** to work with
5. **Run recon** to see what the agent knows
6. **Create session** for interactive use

### Multiple Nodes

When you have multiple nodes:
- Each node appears in the sidebar
- Select one to work with it
- Operations target the selected node/agent
- Traffic interception is per-node

### Refreshing

The service periodically requests updates from nodes. You can also:
- Click refresh to update a specific node
- Trigger re-fingerprinting if agents changed

## Agent Capabilities

Different agents support different features:

| Feature | Claude Code | Claude Bridge | Claude Desktop | Codex | Cursor | Gemini | M365 Copilot |
|---------|-------------|---------------|----------------|-------|--------|--------|--------------|
| Static Recon | ✓ | - | ✓ | ✓ | ✓ | ✓ | ✓ |
| Semantic Recon | ✓ | - | ✓ | ✓ | ✓ | ✓ | ✓ |
| Sessions | ✓ | ✓ | ✓ | ✓ | ✓ (ACP) | ✓ (ACP) | ✓ |
| Config Editing | ✓ | - | ✓ | ✓ | ✓ | ✓ | - |
| MCP Discovery | ✓ | - | ✓ | ✓ | - | ✓ | - |
| Traffic Intercept | ✓ | - | ✓ | - | ✓ | ✓ | ✓ |

## Troubleshooting

### Node not appearing

- Check RabbitMQ connection from the node
- Verify PRAXIS_RABBITMQ_URL is correct
- Look at node logs for errors

### Agent not fingerprinted

- Ensure the agent is installed and configured
- Check that config files exist in expected locations
- Verify the agent binary is in PATH

### Agent disappeared

- The agent may have been uninstalled
- Config files may have moved
- Try refreshing the node

### Can't select agent

- Ensure the node is connected
- Check that fingerprinting succeeded
- Look for errors in the node logs
