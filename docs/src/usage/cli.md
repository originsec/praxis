# CLI

The Praxis CLI (`praxis_cli`) provides both an interactive terminal UI and a non-interactive command-line interface for controlling the Praxis C2 network.

## Purpose

The CLI is the primary terminal interface for Praxis. It provides:
- Full-featured interactive terminal UI for hands-on control
- Non-interactive commands for scripting and automation
- Headless environments without browser access

## Installation

The CLI is installed automatically with the native installation scripts:

```bash
# Linux/macOS
curl -fsSL https://praxis.originhq.com/install.sh | bash
```

The binary is installed to `~/.praxis/bin/praxis_cli`.

When using Docker, the CLI binary is built into the container image and copied to the data volume on startup. You can extract it with:

```bash
docker cp $(docker compose ps -q praxis):/app/praxis_cli ./praxis_cli
```

> **Note:** The container name depends on your project directory. Run this from the directory containing your `docker-compose.yml`.

## Interactive Terminal UI (Default Mode)

Running `praxis_cli` with no arguments launches the interactive terminal UI:

```
$ praxis_cli
```

The terminal UI provides four main windows, switched with keyboard shortcuts:

### Orchestrator (`Ctrl+O`)

LLM-powered conversation interface for coordinating operations across the Praxis network. Features:
- Real-time streaming responses with tool execution display
- Plan tracking with step visualization
- Token usage statistics
- Command history and conversation scrolling
- Save conversations (`Ctrl+W`)
- Model selection (`Ctrl+N`)

### Nodes (`Ctrl+L`)

Node and agent management with integrated session chat and terminal access:
- Node list with status indicators (active/warning/inactive), OS details, and agent counts
- Agent selection and session management
- **Session Chat** — direct conversation with agents, with YOLO mode and working directory selection
- **Terminal** (`Ctrl+R` to create, `Ctrl+T` to toggle) — full PTY terminal emulation with scrollback

### Operations (`Ctrl+P`)

Operation and chain management with two tabs:
- **Library** — browse operation and chain definitions with search filtering and detail view
- **Executions** — live tracking of running/queued/completed operations and chains with duration timers
- Create new operations inline
- Run operations with node/agent selection and YOLO mode

### Settings (`Ctrl+S`)

Configuration management:
- **LLM** — model definitions, provider selection, API keys, and feature assignment (orchestrator, semantic ops, semantic parser, traffic parser)
- **Service** — MCP server toggle, MCP port, Claude Bridge settings (CCRv1/CCRv2 enable and port configuration), logging, hunting row limits, prompt timeout
- **About** — connection info

### Mouse Support

The TUI supports mouse interactions across all windows:

- **Click** — select items in lists, tabs, and interactive elements
- **Double-click** — activate items (e.g. open an operation, select a node)
- **Drag** — scroll through lists and content areas
- **Scroll wheel** — scroll through lists, chat history, and scrollable content

Mouse interactions work alongside keyboard controls in all windows and popups.

### Global Keybindings

| Key | Action |
|-----|--------|
| `Ctrl+O` | Orchestrator window |
| `Ctrl+L` | Nodes window |
| `Ctrl+P` | Operations window |
| `Ctrl+S` | Settings window |
| `Ctrl+T` | Toggle terminal mode |
| `Ctrl+Q` | Quit |

## Non-Interactive Mode

### One-Shot Commands

Use `-C` to run a single command and exit:

```bash
praxis_cli -C "node list"
praxis_cli -C "session create --node abc123 --yolo"
```

### Direct Subcommands

Subcommands can also be passed directly:

```bash
praxis_cli node list
praxis_cli session create --node abc123 --yolo
```

### Available Commands

**Node Management:**
```bash
node list                          # List all connected nodes
node select <prefix>               # Select node by ID prefix
node reset <prefix>                # Reset a node
```

**Agent Management:**
```bash
agent list --node <prefix>         # List agents on a node
agent select --node <prefix> <name>  # Select agent
agent update --node <prefix>       # Request agent info update
agent config read --node <prefix> <path>    # Read config file
agent config write --node <prefix> <path> <contents>  # Write config file
agent config grep --node <prefix> <path> <pattern>    # Grep config file
agent session read --node <prefix> <file>   # Read session file
agent session grep --node <prefix> <file> <pattern>   # Grep session file
```

**Session Management:**
```bash
session create --node <prefix> [--yolo] [--project <path>] [--timeout <secs>]
session prompt --node <prefix> <text>
session close --node <prefix>
```

## Global Options

| Option | Description | Default |
|--------|-------------|---------|
| `-r, --rabbitmq` | RabbitMQ URL | `amqp://praxis:praxis@localhost:5672` |
| `-t, --timeout` | Connection/command timeout in seconds | `600` |
| `-C, --command` | Run a single command and exit | - |
| `--clear` | Clear local state and exit | - |
| `--status` | Check service connection status | - |

The RabbitMQ URL can also be set via the `PRAXIS_RABBITMQ_URL` environment variable.

## Local State

The CLI stores persistent state in `~/.praxis/cli.json`. This file contains:

- **client_id**: A unique identifier for this CLI instance, used for RabbitMQ queue routing

The client ID is generated on first run and reused for subsequent executions.

To reset local state:
```bash
praxis_cli --clear
```

