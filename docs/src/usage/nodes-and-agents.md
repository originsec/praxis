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

This is a Linux/macOS mechanism only (it switches the process's Unix user based on file ownership). It has no equivalent on Windows: an admin-elevated Windows node always runs agent sessions as its own Windows user, regardless of working directory ownership.

**Note**: Full superuser support is still under development. Users may notice unexpected behaviour when running sessions as different users from a root node. If you encounter issues, try running the node as the target user directly instead.

### Privileged Status

Each node reports whether it is running with elevated privileges. On Linux/macOS this means running as root (UID 0); on Windows this means running as an elevated administrator.

Privileged nodes display a **priv** badge in the praxis TUI. Some features —
particularly interception methods that modify system-level configuration (VPN,
Hosts, TPROXY) — require elevated privileges. The TUI rejects an interception
enable request for a non-privileged node.

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

Removing an online deployed node sends it a graceful shutdown request, which cancels active work, restores interception settings, and exits the node process before the service removes it from the list. Start the node binary again to reconnect it.

Removing an offline or stale node only clears it from the service's tracking, so a future intentional restart can register normally.

### Resetting Nodes

You can reset a node to cancel all in-flight operations and return it to a clean state. Reset will:

- Cancel all running transactions (prompts, recon, etc.)
- Drop every live ACP session and its per-session Lua VM
- Close any terminal session
- Disable interception and restore system settings
- Re-register the node with the service

In the TUI, open **Nodes** with `Ctrl+L`, select the node, and press
`Ctrl+R` to request a reset. The same action is available through the CLI
command `node reset <id>` or the MCP tool `node_reset`. The node briefly goes
offline during reset and comes back with fresh state. Clients drop their local
entries for the reset node immediately and re-pull `session/list` after a
short grace period so the Active Sessions overlay reflects reality.

## Agents

Agents are the AI assistants detected on each node. When a node fingerprints successfully, you'll see agents like:

- **[Antigravity CLI](../connectors/agy.md)** - Google's terminal AI-agent interface (`agy`)
- **Claude Code** - Anthropic's CLI assistant
- **Claude Desktop** - Anthropic's desktop app (Windows only)
- **Codex CLI** - OpenAI's CLI assistant
- **Cursor Agent** - Cursor's background agent CLI (Linux only)
- **[Droid CLI](../connectors/droid.md)** - Factory's CLI coding agent
- **Gemini CLI** - Google's CLI assistant
- **M365 Copilot** - Microsoft 365 Copilot (Windows only)
- **[Pi Coding Agent](../connectors/pi.md)** - minimal terminal coding harness (`pi`)

See [Agent Connectors](../connectors/overview.md) for the full connector list and platform notes.

### Agent Selection

In **Nodes** (`Ctrl+L`), press `Enter` or `→` to focus the agent pane,
then use `↑`/`↓` to select an agent. Recon and new sessions target that
selection. A node can host concurrent sessions across any combination of
agents; selection is a TUI convenience, not a routing constraint. Recon is
agent-scoped (`_praxis/recon` is called with the agent's `short_name`), and
each session explicitly names its connector via `_meta.praxis.connector` on
`session/new`.

### Agent States

**Fingerprinted** — the agent was detected but no session is open.

**Session Active** — one or more live sessions exist. The TUI shows a
`LIVE` indicator and, when applicable, a `YOLO` tag. Open the Active
Sessions overlay with `Ctrl+W` to resume or discard a session; see
[Terminal UI](./tui.md#nodes-ctrll) for its controls.

## Working with Nodes and Agents

### Typical Workflow

1. **Deploy node** to target system
2. **Select node** in the praxis TUI's Nodes window (`Ctrl+L`)
3. **Check agents** that were fingerprinted
4. **Focus the agent pane** (`Enter` or `→`) and select an agent
5. **Run recon** to see what the agent knows
6. **Create session** for interactive use

### Multiple Nodes

When you have multiple nodes, select one in the **Nodes** list with
`↑`/`↓`. Operations target the selected node/agent, and traffic
interception is per-node.

### Refreshing

The service periodically requests updates from nodes. If the installed agents
change, restart the node or use its reset action so it fingerprints again.

## Agent Capabilities

Different agents support different features:

| Feature | Claude Code | Claude Bridge | Claude Desktop | Antigravity | Codex | Cursor | Droid | Gemini | M365 Copilot | Pi |
|---------|-------------|---------------|----------------|-------------|-------|--------|-------|--------|--------------|----|
| Static Recon | ✓ | - | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Semantic Recon | ✓ | - | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Sessions | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ (ACP) | ✓ | ✓ (ACP) | ✓ | ✓ |
| Config Editing | ✓ | - | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | - | ✓ |
| MCP Discovery | ✓ | - | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | - | - |
| Traffic Intercept | ✓ | - | ✓ | - | - | ✓ | ✓ | ✓ | ✓ | - |

Codex traffic interception is not yet supported; implementation is tracked in
[issue #259](https://github.com/originsec/praxis/issues/259).

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
