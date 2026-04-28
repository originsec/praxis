<p align="center"><code>curl -fsSL https://praxis.originhq.com/install.sh | bash</code><br />or <code>docker run originsec/praxis</code><br />or <code>yay -S praxis</code></p>

<p align="center"><strong>Praxis</strong> is an open-source research platform for discovering, controlling, and orchestrating AI agents on endpoints.</p>

<p align="center">
  <img src="assets/demo.gif" width="800">
</p>

## Quick Start

### Install

**Linux/macOS:**
```bash
curl -fsSL https://praxis.originhq.com/install.sh | bash
```

**Arch Linux (AUR):**
```bash
yay -S praxis
```

**Windows:**
```powershell
irm https://praxis.originhq.com/install.ps1 | iex
```

**Docker:**
```bash
curl -fsSL https://praxis.originhq.com/docker.sh | bash
```

Then open <http://localhost:8080> in your browser.

> For detailed install options (cross-compilation, deployment patterns), see the [full documentation](https://originsec.github.io/praxis/).

### Deploy a node

1. In the web UI, go to **Settings** → **Service** and download a node binary
2. Run it on the target system:
```bash
PRAXIS_RABBITMQ_URL=amqp://user:pass@your-server:5672 ./praxis_node
```

### Configure an LLM provider

Go to **Settings** → **LLM Providers** in the web UI, add a model, and assign it to the features you want (semantic operations, recon, traffic parsing, orchestrator).

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
