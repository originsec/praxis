<p align="center"><code>curl -fsSL https://praxis.originhq.com/install.sh | bash</code><br />or <code>irm https://praxis.originhq.com/install.ps1 | iex</code> (Windows)<br />or <code>yay -S praxis</code> (Arch)</p>

<p align="center"><strong>Praxis</strong> is an open-source research platform for discovering, controlling, and orchestrating AI agents on endpoints.</p>

<p align="center">
  <img src="assets/demo.gif" width="800">
</p>

## Quick Start

### Install

**Linux / macOS:**
```bash
curl -fsSL https://praxis.originhq.com/install.sh | bash
```

Launches an interactive menu:

- **Native install** *(Linux only)* — system-wide systemd service, requires RabbitMQ
- **Docker install** *(Linux + macOS)* — RabbitMQ + the praxis container
- **Client only** — just installs the `praxis` TUI

The CLI is always installed natively. Skip the menu with `--service native`, `--service docker`, `--cli`, or `--remove`.

**Windows:**
```powershell
irm https://praxis.originhq.com/install.ps1 | iex
```

The Praxis service is Linux-only, so on Windows the installer runs the service in **Docker**. The CLI is always built natively as `praxis.exe`.

**Arch Linux:**
```bash
yay -S praxis        # builds from source
yay -S praxis-bin    # prebuilt release
```

### Use it

Drive Praxis through the `praxis` TUI:

```bash
praxis                                              # interactive TUI
praxis set-rabbitmqurl amqp://praxis:praxis@localhost:5672
```

On a native Linux install, control the service with `praxisctl`:

```bash
praxisctl status
praxisctl start | stop | restart
praxisctl set-rabbitmqurl <url>
```

> Detailed install options, cross-compile recipes, and deployment patterns: [full documentation](https://originsec.github.io/praxis/).

### Deploy a node

Nodes are distributed binaries that run on target systems. They live at `/usr/local/share/praxis/nodes/` after a native install. Run one on a target with:

```bash
PRAXIS_RABBITMQ_URL=amqp://user:pass@your-server:5672 ./praxis_node
```

### Configure an LLM provider

Use the `praxis` TUI to add a model and assign it to the features you want (semantic operations, recon, traffic parsing, orchestrator).

## Documentation

Full docs: **[originsec.github.io/praxis](https://originsec.github.io/praxis)**

- [Architecture](https://originsec.github.io/praxis/architecture/overview.html)
- [Quick Start](https://originsec.github.io/praxis/getting-started/quick-start.html)
- [CLI](https://originsec.github.io/praxis/usage/cli.html)
- [MCP Server](https://originsec.github.io/praxis/usage/mcp.html)

## Early Release Notice

This is an early release for research and experimentation. Some features are incomplete, the codebase is evolving rapidly, and it is **not designed to be stealthy** (installs root certificates, modifies system settings, etc.).

## License

Apache 2.0 — see [LICENSE](https://github.com/originsec/praxis/blob/main/LICENSE) and [NOTICE](https://github.com/originsec/praxis/blob/main/NOTICE)

Built by [Origin](https://originhq.com) for security research and red team operations.

Contributions are very welcome — open issues or submit pull requests.
