# CLI

The Praxis CLI (`praxis_cli`) provides a command-line interface for interacting with the Praxis C2 network.

## Purpose

The CLI is designed for **external agent orchestration** and **programmatic exploration** of the Praxis network. It is not intended to replace the web interface at this stage - not all features are available in the CLI.

Primary use cases:
- Interactive REPL for hands-on exploration and control
- Scripting and automation
- Integration with external AI agents (via MCP server mode)
- Headless environments without GUI access

## Installation

The CLI is installed automatically with the native installation scripts:

```bash
# Linux/macOS
curl -fsSL https://praxis.originhq.com/install.sh | bash
```

The binary is installed to `~/.praxis/bin/praxis_cli`.

When using Docker, the CLI binary is built into the container image and copied to the data volume on startup. You can extract it with:

```bash
docker cp praxis-praxis-1:/app/praxis_cli ./praxis_cli
```

## Interactive REPL (Default Mode)

Running `praxis_cli` with no arguments launches an interactive REPL:

```
$ praxis_cli

    ____                  _
   / __ \_________ __  __(_)____
  / /_/ / ___/ __ `/ |/_/ / ___/
 / ____/ /  / /_/ />  </ (__  )
/_/   /_/   \__,_/_/|_/_/____/

  praxis 0.9.3 | client 953da792 | 1 node(s)
  amqp://praxis:praxis@localhost:5672
  Type help for commands, exit (or ctrl+d) to quit

praxis [b3bf7460:claudecode *] ❯
```

### Selection State

The REPL tracks your selected node, agent, and active session. The prompt updates to reflect the current state:

```
praxis ❯                                 # nothing selected
praxis [b3bf7460] ❯                      # node selected
praxis [b3bf7460:claudecode] ❯           # node + agent
praxis [b3bf7460:claudecode *] ❯         # node + agent + active session
```

Select a node and agent:

```
praxis ❯ node select b3bf7460
✓ Selected node: b3bf7460 (kaplan-ws-linux)

praxis [b3bf7460] ❯ agent select claudecode
✓ Selected agent: claudecode

praxis [b3bf7460:claudecode] ❯
```

On startup, if there is exactly one active node, it is auto-selected along with any existing agent selection and session state.

### Implicit Flag Injection

Once a node and agent are selected, the `-n` and `-a` flags are injected automatically. You don't need to pass them for every command:

```
praxis [b3bf7460:claudecode] ❯ session create
✓ Session created: a1b2c3d4

praxis [b3bf7460:claudecode *] ❯ session prompt "list files"
```

This is equivalent to typing `session create -n b3bf7460` and `session prompt -n b3bf7460 "list files"`. You can always override by passing the flag explicitly.

### Tab Completion

The REPL provides context-aware tab completion:

- **Command names**: `op<TAB>` → `op`, `node<TAB>` → `node`
- **Node IDs**: `node select <TAB>` shows connected node IDs
- **Agent names**: `agent select <TAB>` shows discovered agent names
- **Operation names**: `op run <TAB>` shows available operations and chains
- **Short IDs**: `op status <TAB>` shows tracked operation/chain IDs
- **Flag values**: `-n <TAB>` shows node IDs, `-a <TAB>` shows agent names

The completion cache refreshes after every command.

### Error Messages

The REPL provides contextual error messages instead of raw usage text:

```
praxis ❯ session prompt "hi"
✗ No node selected. Use 'node select <id>' first, or pass -n <id>.
```

## One-Shot Mode

Use `-C` to run a single command and exit:

```bash
praxis_cli -C "node list"
praxis_cli -C "op run recon::system_info -n abc123 -a claudecode"
```

For backwards compatibility, subcommands can also be passed directly:

```bash
praxis_cli node list
praxis_cli op run recon::system_info --node abc123 --agent claudecode
```

## Global Options

| Option | Description | Default |
|--------|-------------|---------|
| `-r, --rabbitmq` | RabbitMQ URL | `amqp://praxis:praxis@localhost:5672` |
| `-o, --output` | Output format (`text` or `json`) | `text` |
| `-t, --timeout` | Command timeout in seconds | `300` |
| `-C, --command` | Run a single command and exit | - |
| `--fullhelp` | Show comprehensive help | - |
| `--clear` | Clear local state and exit | - |
| `--status` | Check service connection status | - |
| `--mcp` | Run as MCP server (stdio) | - |

The RabbitMQ URL can also be set via the `PRAXIS_RABBITMQ_URL` environment variable.

## Commands

### Node Management

```bash
# List all connected nodes
node list

# Select a node by ID prefix
node select abc123
```

### Agent Management

```bash
# List agents on a node
agent list

# Select an agent
agent select claudecode

# Request agent info update
agent update

# Perform reconnaissance
agent recon
agent recon-semantic

# Read/write/grep config content
agent config read /home/user/.codex/config.toml
agent config read /home/user/.codex/config.toml --line-start 1 --line-end 50
agent config write /home/user/.codex/config.toml "new content"
agent config grep /home/user/.codex/config.toml "model|profile"

# Read/grep session content
agent session read /home/user/.codex/sessions/2026-02-13.jsonl
agent session grep /home/user/.codex/sessions/2026-02-13.jsonl "error|warning"
```

### Sessions

```bash
# Create a session with YOLO mode and working directory
session create --yolo --project /path/to/project

# Send a prompt
session prompt "list files in current directory"

# Close session
session close
```

Session options:
- `--yolo`: Enable YOLO mode (auto-approve actions)
- `--project <PATH>`: Set the working directory for the session

### Operations and Chains

Operations and chains are managed under the `op` command. When running or checking status, the CLI searches both operations and chains automatically.

```bash
# List available operations and chains
op available

# Run an operation
op run recon::system_info

# Run a chain (same command — chains are matched by name or ID)
op run full_recon_chain

# Run with working directory
op run recon::system_info --working-dir /path/to/project

# List tracked (running/completed) operations and chains
op list

# Check status of an operation or chain execution
op status abc123

# Cancel a running operation or chain
op cancel abc123
```

### Traffic Search

```bash
# Search intercepted traffic
traffic search "api\.openai\.com" --limit 20

# Filter by node and agent
traffic search "Bearer" --node abc123 --agent claudecode
```

## JSON Output

Use `--output json` for machine-readable output:

```bash
praxis_cli -o json -C "node list" | jq '.nodes[].node_id'
```

## Local State

The CLI stores persistent state in `~/.praxis/cli.json`. This file contains:

- **client_id**: A unique identifier for this CLI instance, used for RabbitMQ queue routing

The client ID is generated on first run and reused for subsequent executions.

To reset local state:
```bash
praxis_cli --clear
```

## MCP Server Mode

The CLI can run as a [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server, enabling integration with any MCP-compatible AI assistant including Claude Code, Claude Desktop, Cursor, Windsurf, and others.

### Running as MCP Server

```bash
praxis_cli --mcp
```

This starts the CLI in MCP server mode, communicating via stdio. The server exposes all CLI functionality as MCP tools.

### Claude Code Integration

Add the Praxis MCP server to Claude Code:

```bash
# User scope (available across all projects)
claude mcp add praxis --scope user -- ~/.praxis/bin/praxis_cli --mcp

# Or project scope (shared with team via .mcp.json)
claude mcp add praxis -- ~/.praxis/bin/praxis_cli --mcp
```

To set a custom RabbitMQ URL, set the environment variable before running Claude Code:

```bash
export PRAXIS_RABBITMQ_URL="amqp://praxis:praxis@your-server:5672"
```

### Other MCP Clients

For other MCP-compatible clients, use JSON configuration:

```json
{
  "mcpServers": {
    "praxis": {
      "command": "/home/user/.praxis/bin/praxis_cli",
      "args": ["--mcp"],
      "env": {
        "PRAXIS_RABBITMQ_URL": "amqp://praxis:praxis@your-server:5672"
      }
    }
  }
}
```

### Available MCP Tools

The MCP server exposes the following tools:

**Node Management:**
- `node_list` - List all connected nodes
- `node_select` - Get details for a specific node

**Agent Management:**
- `agent_list` - List agents on a node
- `agent_select` - Get details for a specific agent
- `agent_update` - Request agent info refresh
- `agent_recon` - Run agent reconnaissance
- `agent_recon_semantic` - Run semantic reconnaissance
- `read_file` - Read config/session file content (`file_type: Config|Session`, optional line range)
- `write_file` - Write file content (`file_type: Config` only)
- `grep_file` - Search config/session file content with regex (`file_type: Config|Session`)

**Sessions:**
- `session_create` - Create a new session
- `session_prompt` - Send a prompt to the active session
- `session_close` - Close the active session

**Operations:**
- `op_list` - List available semantic operations
- `op_run` - Run a semantic operation
- `op_status` - Check operation status
- `op_cancel` - Cancel a running operation
- `op_running` - List all running operations

**Chains:**
- `chain_list` - List available chains
- `chain_run` - Run a chain workflow
- `chain_status` - Check chain execution status
- `chain_cancel` - Cancel a running chain
- `chain_running` - List all running chain executions

**Traffic:**
- `traffic_search` - Search intercepted traffic

## AI Agent Integration

There are two ways to integrate Praxis with AI coding agents:

### Option 1: MCP Server (Recommended)

Use `praxis_cli --mcp` for native tool integration. The AI assistant sees Praxis tools directly in its tool list and can call them without shell access.

**Pros:**
- Native tool integration - tools appear in the assistant's tool list
- Structured input/output - no shell parsing needed
- Works with agents that don't have shell access
- Cleaner error handling

**Cons:**
- Requires MCP client support (Claude Code, Cursor, Windsurf, etc.)
- Configuration required per client

### Option 2: SKILL.md (Shell-based)

Include `cli/SKILL.md` in the agent's context. The agent uses shell commands to interact with Praxis.

**Pros:**
- Works with any agent that has shell access
- No configuration needed - just add the skill file
- Agent can combine with other shell tools

**Cons:**
- Requires shell access
- Output parsing can be fragile
- More verbose interactions

### Which to Choose?

- **Use MCP** if your AI assistant supports it and you want seamless tool integration
- **Use SKILL.md** if you need shell-based workflows or your assistant doesn't support MCP

Both approaches provide the same functionality - choose based on your environment and preferences.

## Limitations

The CLI currently supports a subset of Praxis features focused on orchestration:
- Node and agent management
- Sessions and prompts
- Semantic operations and chains
- Traffic search
- MCP server mode for AI assistant integration

Features **not** available in the CLI:
- AgentChat (IRC-style multi-agent chat)
- Visual chain editor
- Real-time traffic monitoring
- Terminal emulation
- Intercept rule management
- Agent discovery

Use the web interface for these features.
