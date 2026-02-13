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

### Superuser Mode

When the node runs as root, it can operate as different users based on the selected working directory. Selecting a working directory owned by another user will cause agent sessions to run as that user (with the appropriate `HOME` environment variable set).

**Note**: Full superuser support is still under development. Users may notice unexpected behaviour when running sessions as different users from a root node. If you encounter issues, try running the node as the target user directly instead.

### Node List

In the web UI, the left sidebar shows all connected nodes. Click a node to select it. The main panel then shows that node's details and agents.

### Removing Nodes

If a node disconnects and you want to remove it from the list, click the remove button. This clears the node from the service's tracking. If the node reconnects, it will appear again.

## Agents

Agents are the AI assistants detected on each node. When a node fingerprints successfully, you'll see agents like:

- **Claude Code** - Anthropic's CLI assistant
- **Claude Desktop** - Anthropic's desktop app (Windows only)
- **Codex CLI** - OpenAI's CLI assistant
- **Gemini CLI** - Google's CLI assistant
- **M365 Copilot** - Microsoft 365 Copilot (Windows only)

### Agent Selection

Click an agent to select it. This focuses operations on that specific agent:
- Recon targets that agent
- Sessions connect to that agent
- Operations execute through that agent

Only one agent can be selected at a time per node.

### Agent States

An agent can be in different states:

**Fingerprinted** - The agent was detected but no session exists.

**Session Active** - There's an active session with the agent. You can send prompts and run operations.

**Session (YOLO)** - Active session with auto-approve enabled.

## Working with Nodes and Agents

### Typical Workflow

1. **Deploy node** to target system
2. **Select node** in the UI
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

| Feature | Claude Code | Claude Desktop | Codex | Gemini | M365 Copilot |
|---------|-------------|----------------|-------|--------|--------------|
| Static Recon | ✓ | ✓ | ✓ | ✓ | ✓ |
| Semantic Recon | ✓ | ✓ | ✓ | ✓ | ✓ |
| Sessions | ✓ | ✓ | ✓ | ✓ | ✓ |
| Config Editing | ✓ | ✓ | ✓ | ✓ | - |
| MCP Discovery | ✓ | ✓ | ✓ | ✓ | - |
| Traffic Intercept | ✓ | ✓ | - | ✓ | ✓ |

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
