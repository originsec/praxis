# CLI

The Praxis CLI (`praxis_cli`) provides a command-line interface for interacting with the Praxis C2 network.

## Purpose

The CLI is designed for **external agent orchestration** and **programmatic exploration** of the Praxis network. It is not intended to replace the web interface at this stage - not all features are available in the CLI.

Primary use cases:
- Scripting and automation
- Integration with external AI agents
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
# Create a session with YOLO mode
praxis_cli session create --node abc123 --yolo --project /path/to/project

# Send a prompt
praxis_cli session prompt --node abc123 "list files in current directory"

# Close session
praxis_cli session close --node abc123
```

### Semantic Operations

```bash
# List available operations
praxis_cli op list

# Run an operation
praxis_cli op run recon::system_info --node abc123 --agent claudecode

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

# Check status
praxis_cli chain status abc123

# List running executions
praxis_cli chain running
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

Features **not** available in the CLI:
- AgentChat (IRC-style multi-agent chat)
- Visual chain editor
- Real-time traffic monitoring
- Terminal emulation
- Intercept rule management
- Agent discovery

Use the web interface for these features.
