# CLI

The Praxis CLI (`praxis_cli`) provides a command-line interface for interacting with the Praxis C2 network.

## Purpose

The CLI is designed for **external agent orchestration** and **programmatic exploration** of the Praxis network. It is not intended to replace the web interface at this stage - not all features are available in the CLI.

Primary use cases:
- Scripting and automation
- Integration with external AI agents (via MCP server mode)
- Headless environments without GUI access
- Quick operations from the command line

## Installation

The CLI is installed automatically with the standard Praxis installation scripts:

```bash
# Linux/macOS
curl -fsSL https://praxis.originhq.com/install.sh | bash

# Windows (PowerShell)
irm https://praxis.originhq.com/install.ps1 | iex
```

The binary is installed to `~/.praxis/bin/praxis_cli`.

## Getting Help

View basic help:
```bash
praxis_cli --help
```

View comprehensive help for all commands:
```bash
praxis_cli --fullhelp
```

The `--fullhelp` option outputs documentation for every command and subcommand, including all available options and arguments.

## Global Options

| Option | Description | Default |
|--------|-------------|---------|
| `-r, --rabbitmq` | RabbitMQ URL | `amqp://praxis:praxis@localhost:5672` |
| `-o, --output` | Output format (`text` or `json`) | `text` |
| `-t, --timeout` | Command timeout in seconds | `300` |
| `--fullhelp` | Show comprehensive help | - |
| `--clear` | Clear local state and exit | - |
| `--status` | Check service connection status | - |
| `--mcp` | Run as MCP server (stdio) | - |

The RabbitMQ URL can also be set via the `PRAXIS_RABBITMQ_URL` environment variable.

## Local State

The CLI stores persistent state in `~/.praxis/cli.json`. This file contains:

- **client_id**: A unique identifier for this CLI instance, used for RabbitMQ queue routing

The client ID is generated on first run and reused for subsequent executions. This allows the Praxis service to maintain consistent communication with the CLI across sessions.

To reset local state:
```bash
praxis_cli --clear
```

This removes `~/.praxis/cli.json`, causing a new client ID to be generated on the next run.

## Checking Connection Status

Verify the CLI can connect to the Praxis service:

```bash
praxis_cli --status
```

This connects to RabbitMQ, registers with the service, and displays connection information including the number of connected nodes.

## Commands

### Node Management

```bash
# List all connected nodes
praxis_cli node list

# Select a node by ID prefix
praxis_cli node select abc123
```

### Agent Management

```bash
# List agents on a node
praxis_cli agent list --node abc123

# Select an agent
praxis_cli agent select --node abc123 claudecode

# Request agent info update
praxis_cli agent update --node abc123

# Perform reconnaissance
praxis_cli agent recon --node abc123
praxis_cli agent recon-semantic --node abc123
```

### Sessions

```bash
# Create a session with YOLO mode and working directory
praxis_cli session create --node abc123 --yolo --project /path/to/project

# Send a prompt
praxis_cli session prompt --node abc123 "list files in current directory"

# Close session
praxis_cli session close --node abc123
```

Session options:
- `--yolo`: Enable YOLO mode (auto-approve actions)
- `--project <PATH>`: Set the working directory for the session

### Semantic Operations

```bash
# List available operations
praxis_cli op list

# Run an operation
praxis_cli op run recon::system_info --node abc123 --agent claudecode

# Run with working directory
praxis_cli op run recon::system_info --node abc123 --agent claudecode --working-dir /path/to/project

# Check status
praxis_cli op status abc123

# List running operations
praxis_cli op running

# Cancel an operation
praxis_cli op cancel abc123
```

### Chains

```bash
# List available chains
praxis_cli chain list

# Run a chain
praxis_cli chain run mychain --node abc123 --agent claudecode

# Run with working directory
praxis_cli chain run mychain --node abc123 --agent claudecode --working-dir /path/to/project

# Check status
praxis_cli chain status abc123

# List running executions
praxis_cli chain running

# Cancel an execution
praxis_cli chain cancel abc123
```

### Traffic Search

```bash
# Search intercepted traffic
praxis_cli traffic search "api\.openai\.com" --limit 20

# Filter by node and agent
praxis_cli traffic search "Bearer" --node abc123 --agent claudecode
```

## JSON Output

Use `--output json` for machine-readable output:

```bash
praxis_cli --output json node list | jq '.nodes[].node_id'
```

## MCP Server Mode

The CLI can run as a [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server, enabling integration with any MCP-compatible AI assistant including Claude Code, Claude Desktop, Cursor, Windsurf, and others.

### Running as MCP Server

```bash
praxis_cli --mcp
```

This starts the CLI in MCP server mode, communicating via stdio. The server exposes all CLI functionality as MCP tools.

### Claude Code Integration

Add to your Claude Code MCP configuration (`~/.claude/mcp.json`):

```json
{
  "mcpServers": {
    "praxis": {
      "command": "/home/user/.praxis/bin/praxis_cli",
      "args": ["--mcp"],
      "env": {
        "PRAXIS_RABBITMQ_URL": "amqp://praxis:praxis@localhost:5672"
      }
    }
  }
}
```

### Other MCP Clients

The MCP server works with any client supporting the MCP protocol. Configuration varies by client:

- **Claude Desktop**: `~/.config/claude-desktop/config.json` (Linux) or `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS)
- **Cursor**: Settings → Features → MCP Servers
- **Windsurf**: Settings → MCP

Use the same server configuration format shown above.

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

## Agentic Use (skill.md)

The CLI includes a `skill.md` file located at `cli/skill.md` in the repository. This file provides guidance for AI agents on how to use the Praxis CLI.

When integrating with an external AI agent (like Claude Code), you can include this skill.md in the agent's context. The skill instructs the agent to run `praxis_cli --fullhelp` to discover all available commands and capabilities.

Example workflow for an AI agent:
1. Run `praxis_cli --fullhelp` to learn available commands
2. Run `praxis_cli node list` to see connected nodes
3. Select a node and agent
4. Create a session and interact

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
