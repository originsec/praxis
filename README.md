# Praxis

**Semantic Command & Control Framework for AI Agents**

As AI computer-use/coding agents become more prevalent - with tools that can read files, execute commands, and interact directly with systems - understanding their security properties becomes critical.

Praxis is an open-source research and experimentation platform for discovering, controlling, and orchestrating computer-use AI agents across endpoints.

We're hoping to enrich our understanding of what's possible when you have legitimate access to the systems these agents run on, and what that means for endpoint security.

Built by [Origin](https://originhq.com) for security research and red team operations.

## Features

| Feature | Description |
|---------|-------------|
| **Agent Discovery** | Fingerprint and detect computer-use agents on endpoints |
| **Reconnaissance** | Enumerate tools (MCP servers, skills), configurations, and session histories |
| **Config Visibility** | View and edit agent configuration files directly |
| **Traffic Interception** | MITM proxy for agent-to-LLM traffic |
| **Agent Dialog** | Create interactive sessions with agents |
| **Semantic Operations** | Define and chain natural language tasks for multi-step automation |
| **Terminal Access** | PTY terminal on remote nodes |
| **Agent Chat** *(experimental)* | Multi-agent collaboration and orchestration workflows |
| **Orchestrator** *(experimental)* | High-level assistant for coordinating tasks across agents |

### Supported Agents

- Claude Code CLI
- Codex CLI (OpenAI)
- Cursor Agent CLI
- Gemini CLI
- Microsoft 365 Copilot (Windows)

Connectors are implemented as Lua agent scripts and can be updated through the web UI.

## Early Release Notice

This is an early release to showcase initial capabilities. It is **not yet ready** for full-scale red teaming or production use - although you can certainly experiment to your hearts content!

The platform is under active development and:

- Some features are incomplete or experimental
- The codebase is evolving rapidly
- **This is not designed to be stealthy** it installs root certificates, modifies system settings, and is generally quite noisy

We're releasing early to get feedback and contributions from the community.

## Quick Start

### One-Liner (Docker)

```bash
curl -fsSL https://praxis.originhq.com/docker.sh | bash
```

This clones the latest release, builds with Docker Compose, and starts the service, web UI, and RabbitMQ. Open **http://localhost:8080** in your browser.

The CLI binary is built into the Docker image. Extract it with:

```bash
docker cp praxis-praxis-1:/app/praxis_cli ./praxis_cli
```

### Manual Docker

```bash
git clone https://github.com/originsec/praxis.git
cd praxis
docker compose up --build
```

To run without the web UI (headless — service + RabbitMQ only):

```bash
PRAXIS_HEADLESS=1 docker compose up --build
```

### Native Install

```bash
# Linux/macOS
curl -fsSL https://praxis.originhq.com/install.sh | bash
```

This installs Rust if needed, builds from source, and sets up binaries in `~/.praxis/bin/` including `praxis_cli`.

> For detailed installation instructions (cross-compilation, Windows, deployment patterns), see the [full documentation](https://originsec.github.io/praxis/).

### Deploy Nodes

Once the service is running, deploy nodes to target systems:

1. Go to **Settings** → **Service** in the web UI
2. Download the appropriate node binary (Linux or Windows)
3. Run the node on the target system:

```bash
# Linux (localhost RabbitMQ URL for testing)
./praxis_node

# Or with custom RabbitMQ URL
PRAXIS_RABBITMQ_URL=amqp://user:pass@your-server:5672 ./praxis_node
```

Node binaries are also available from [GitHub Releases](https://github.com/originsec/praxis/releases).

## Configuration

After starting Praxis, configure at least one LLM provider in **Settings** → **LLM Providers**:

1. Add a model definition (provider + model + API key)
2. Assign it to the features you want to use:
   - **Semantic Operations** - for executing operations
   - **Semantic Parser** - for tool discovery during recon
   - **Traffic Parser** - for summarizing intercepted traffic

We recommend low-latency providers (for example **Groq** or **Cerebras**) for parser-heavy workflows.

## CLI

The Praxis CLI (`praxis_cli`) provides an interactive REPL for controlling the Praxis network from the command line.

```
praxis [b3bf7460:claudecode *] ❯ session prompt "list all files"
```

Features:
- **Interactive REPL** with selection state — select a node and agent once, then run commands without repeating `-n`/`-a` flags
- **Tab completion** for node IDs, agent names, operation names, and short IDs
- **One-shot mode** via `-C "command"` for scripting
- **MCP server mode** via `--mcp` for integration with Claude Code, Cursor, and other AI assistants

See [CLI documentation](https://originsec.github.io/praxis/usage/cli.html) for the full reference.

## Architecture At A Glance

Praxis has three core components:

- **Node**: runs on target systems, discovers agents, handles sessions/recon/interception
- **Service**: central backend + database + operation/chain orchestration
- **Web**: React frontend and WebSocket bridge

See [Architecture Overview](https://originsec.github.io/praxis/architecture/overview.html) for detailed internals.

## Documentation

Full documentation is available at **[originsec.github.io/praxis](https://originsec.github.io/praxis/)**

- [Installation](https://originsec.github.io/praxis/getting-started/installation.html) - Docker, local development builds, cross-compilation
- [Quick Start](https://originsec.github.io/praxis/getting-started/quick-start.html) - First steps walkthrough
- [Nodes & Agents](https://originsec.github.io/praxis/usage/nodes-and-agents.html) - Working with nodes and agents
- [Reconnaissance](https://originsec.github.io/praxis/usage/recon.html) - Tool and configuration discovery
- [Sessions](https://originsec.github.io/praxis/usage/sessions.html) - Interactive agent sessions
- [Agent Connectors](https://originsec.github.io/praxis/connectors/overview.html) - Supported agents and adding new ones
- [CLI](https://originsec.github.io/praxis/usage/cli.html) - Interactive REPL, one-shot mode, MCP integration
- [Interception](https://originsec.github.io/praxis/usage/interception.html) - Traffic capture setup and options
- [Architecture](https://originsec.github.io/praxis/architecture/overview.html) - How it all fits together

## License

Apache 2.0 — see [LICENSE](https://github.com/originsec/praxis/blob/main/LICENSE) and [NOTICE](https://github.com/originsec/praxis/blob/main/NOTICE)

To explore alternate licensing arrangements, please contact legal@preludesecurity.com

## By Origin

[Origin](https://originhq.com) is an endpoint security company building protection for the semantic era of computing. As AI agents become integral to enterprise workflows, Origin provides the visibility and control organizations need to safely grant agents the permissions they require.

## Contributing

Contributions are very welcome! Please feel free to:

- Open issues for bugs or feature requests
- Submit pull requests
- Share ideas for new agent connectors or capabilities

We're particularly interested in contributions around new agent support and offensive techniques.

## Contact

- **Email**: david.kaplan@preludesecurity.com
- **Twitter/X**: [@depletionmode](https://twitter.com/depletionmode)
- **GitHub Issues**: [originsec/praxis/issues](https://github.com/originsec/praxis/issues)
