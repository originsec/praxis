# CLI

The Praxis CLI controls the Praxis C2 network. It has two modes in one
binary: an interactive terminal UI (the default when run with no
arguments) and a non-interactive command-line interface for scripting.

> **`praxis` and `praxis_cli` are the same binary.** The crate and binary
> are named `praxis_cli`; the installers also lay down `praxis` as the
> preferred command name — a symlink on Linux/macOS, a file copy on
> Windows. The CLI derives its own display name from `argv[0]`, so
> `praxis --help` and `praxis_cli --help` are identical apart from the
> name shown in the usage text. **This documentation uses `praxis`
> throughout**; the only places `praxis_cli` appears are where the
> artifact itself is named (installed file paths and `cargo` build
> targets).

This page covers non-interactive usage. For the interactive terminal UI,
see [TUI](./tui.md).

## Purpose

The CLI is the **first-party** and only first-class supported client for
Praxis. It provides:
- Full-featured interactive terminal UI for hands-on control
- Non-interactive commands for scripting and automation
- Works equally well over SSH and in headless environments

## Installation

The CLI is installed automatically with the native installation scripts:

```bash
# Linux/macOS
curl -fsSL https://praxis.originhq.com/install.sh | bash
```

On Linux/macOS this installs `/usr/local/bin/praxis_cli` and symlinks
`/usr/local/bin/praxis` to it. Override the prefix with `INSTALL_PREFIX`.
On Windows the CLI is installed under `%USERPROFILE%\.praxis\bin\` (as
both `praxis_cli.exe` and a `praxis.exe` copy) and that directory is added
to the user `PATH`. Override the location with `PRAXIS_CLI_DIR`.

When using Docker, the CLI binary is built into the container image and copied to the data volume on startup. You can extract it with:

```bash
docker cp $(docker compose ps -q praxis):/app/praxis_cli ./praxis
```

> **Note:** The container name depends on your project directory. Run this from the directory containing your `docker-compose.yml`.

## Non-Interactive Mode

### One-Shot Commands

Use `-C` to run a single command and exit:

```bash
praxis -C "node list"
praxis -C "intercept enable abc123 --method tproxy"
praxis -C "session create --node abc123 --agent codex --yolo"
```

### Direct Subcommands

Subcommands can also be passed directly:

```bash
praxis node list
praxis intercept status
praxis session create --node abc123 --agent codex --yolo
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
agent list --node <prefix>                   # List agents on a node
agent update --node <prefix>                 # Request agent info update
agent config read --node <prefix> --agent <name> <path>     # Read config file
agent config write --node <prefix> <path> <contents>        # Write config file (agent-independent)
agent config grep --node <prefix> --agent <name> <path> <pattern>  # Grep config file
agent session read --node <prefix> --agent <name> <file>    # Read session file
agent session grep --node <prefix> --agent <name> <file> <pattern> # Grep session file
```

**Session Management:**
```bash
session create --node <prefix> --agent <name> [--yolo] [--project <path>] [--timeout <secs>]
session prompt --node <prefix> <text>
session close --node <prefix>
```

**Traffic Interception:**
```bash
intercept status [node-prefix]                    # Show interception state
intercept enable <node-prefix> [--method proxy|vpn|hosts|tproxy]
intercept disable <node-prefix>
```

Enable and disable wait for the selected node to finish setup or cleanup and
return the node's error if the operation fails. Independent node commands are
correlated separately, so concurrent callers cannot consume one another's
responses.

Only `session create` takes `--agent` explicitly, to pick which agent to
start the session with; `session prompt` and `session close` operate on
whichever session is currently persisted for that node and take no
`--agent`.

Non-interactive mode persists a single session id per node in
`~/.praxis/cli.json` — `session create` stores it, `session prompt` and
`session close` read it. The interactive TUI runs concurrent in-memory
sessions and does not share state with the non-interactive subcommands.

## Global Options

| Option | Description | Default |
|--------|-------------|---------|
| `-t, --timeout` | Connection/command timeout in seconds | `600` |
| `-C, --command` | Run a single command and exit | - |
| `--acp` | Run as an ACP bridge (stdin/stdout proxy) | - |
| `--clear` | Clear local state and exit | - |
| `--status` | Check service connection status | - |
| `--continue` | Resume the most recent saved orchestrator session | - |
| `--resume` | List saved orchestrator sessions and pick one to resume | - |

The RabbitMQ URL can also be set by writing `PRAXIS_RABBITMQ_URL=<url>` to `~/.config/praxis/config`, or via the subcommands below.

### Configuration

```bash
praxis set-rabbitmqurl <url>   # Persist a RabbitMQ URL to ~/.config/praxis/config
praxis config                  # Show the effective RabbitMQ URL and config file path
```

## ACP Bridge Mode

The CLI can act as an [Agent Client Protocol](https://agentclientprotocol.com/) bridge, exposing the Praxis service as a standard ACP agent over stdin/stdout. This allows any ACP-compatible client to interact with Praxis.

```bash
praxis --acp
```

In this mode the CLI:
- Reads NDJSON JSON-RPC requests from **stdin**
- Forwards them to the Praxis service via RabbitMQ
- Writes JSON-RPC responses and notifications to **stdout** as NDJSON
- Only forwards responses to requests it originated (filters out other clients' traffic)

This means any ACP client can use Praxis as its agent. For example, using [acpx](https://www.npmjs.com/package/acpx):

```bash
acpx --agent 'praxis --acp' 'list agents'
```

The bridge connects with an `acp_` prefixed client ID, so sessions created through it get `ACP_` prefixed session IDs.

## Local State

The CLI stores persistent state in `~/.praxis/cli.json`. This file contains:

- **client_id**: A unique identifier for this CLI instance, used for RabbitMQ queue routing
- **sessions**: A map of node ID → session ID, used by `session prompt` and `session close` to resume the session `session create` started for that node

The client ID is generated on first run and reused for subsequent executions.

To reset local state:
```bash
praxis --clear
```
