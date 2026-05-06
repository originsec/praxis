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

**Windows / macOS:**
```powershell
irm https://praxis.originhq.com/install.ps1 | iex     # Windows
curl -fsSL https://praxis.originhq.com/install.sh | bash   # macOS, then pick Docker
```

The Praxis service is Linux-only, so Windows and macOS run it in **Docker**. The CLI is always built natively (`praxis` / `praxis.exe`).

**Arch Linux:**
```bash
yay -S praxis        # builds from source
yay -S praxis-bin    # prebuilt release
```

### Use it

Launch the TUI:

```bash
praxis
```

Configure LLM providers and everything else from the TUI. On a native Linux install, control the service itself with `praxisctl status` / `praxisctl start | stop | restart`.

> Detailed install options, cross-compile recipes, and deployment patterns: [full documentation](https://originsec.github.io/praxis/).

### Deploy a node

Nodes are standalone binaries that run on target systems. After install, find them at:

| Install method | Linux node | Windows node |
|---|---|---|
| Native (Linux) | `/usr/local/share/praxis/nodes/praxis_node_linux` | `/usr/local/share/praxis/nodes/praxis_node_windows.exe` *(use `--with-win-node`)* |
| Docker         | `docker compose exec praxis ls /usr/local/share/praxis/nodes/` (both shipped) | same |
| AUR (`praxis-bin`) | `/usr/share/praxis/nodes/praxis_node_linux` | `/usr/share/praxis/nodes/praxis_node_windows.exe` |
| GitHub release | [`praxis_node-linux-x86_64`](https://github.com/originsec/praxis/releases/latest) | [`praxis_node-windows-x86_64.exe`](https://github.com/originsec/praxis/releases/latest) |

Copy the binary to the target system and run it pointed at your RabbitMQ:

```bash
PRAXIS_RABBITMQ_URL=amqp://user:pass@your-server:5672 ./praxis_node
```

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
