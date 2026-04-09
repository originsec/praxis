# Installation

There are a few ways to get Praxis running. The one-liner scripts are the easiest for getting started; building from source gives you more control.

## Quick Install (One-Liner)

These scripts automatically fetch the **latest release** and set everything up.

### Docker (Recommended)

```bash
# Linux/macOS
curl -fsSL https://praxis.originhq.com/docker.sh | bash
```

```powershell
# Windows
irm https://praxis.originhq.com/docker.ps1 | iex
```

This clones the latest release, builds with Docker Compose, and starts everything.

### Prerequisites

RabbitMQ must be running before starting Praxis. If you're not using Docker (which includes RabbitMQ), install and start it separately:

```bash
# Linux
sudo systemctl start rabbitmq-server

# macOS (Homebrew)
brew services start rabbitmq
```

### Arch Linux (AUR)

```bash
yay -S praxis
```

Or with `makepkg`:

```bash
git clone https://aur.archlinux.org/praxis.git
cd praxis
makepkg -si
```

This installs:
- `/usr/bin/praxis_service`, `/usr/bin/praxis_web`, `/usr/bin/praxis_cli` - binaries
- `/usr/share/praxis/nodes/praxis_node_linux` - node agent for deployment to targets
- Systemd system services (runs as dedicated `praxis` user)
- `/etc/praxis/env` - configuration

After installing:

```bash
sudo systemctl enable --now rabbitmq
sudo systemctl enable --now praxis
```

### Native Install (Linux/macOS)

```bash
curl -fsSL https://praxis.originhq.com/install.sh | bash
```

This installs Rust if needed, builds from source, and sets up:
- `~/.praxis/bin/praxis_service` - backend service
- `~/.praxis/bin/praxis_web` - web server + frontend
- `~/.praxis/bin/praxis_cli` - command-line interface
- `~/.praxis/bin/nodes/<platform>/praxis_node` - node agent
- Systemd user services (Linux) for automatic startup
- PATH is configured automatically

### Native Install (Windows)

```powershell
irm https://praxis.originhq.com/install.ps1 | iex
```

### Removing

To uninstall Praxis (stops services, removes binaries, config, and PATH entries):

```bash
# Linux/macOS
curl -fsSL https://praxis.originhq.com/install.sh | bash -s -- --remove
```

```powershell
# Windows
irm https://praxis.originhq.com/install.ps1 | iex -- --remove
```

### Pinning a Specific Version

To install a specific version instead of latest:

```bash
# Docker (Linux/macOS)
PRAXIS_VERSION=v0.1.0 curl -fsSL https://praxis.originhq.com/docker.sh | bash

# Native (Linux/macOS)
PRAXIS_VERSION=v0.1.0 curl -fsSL https://praxis.originhq.com/install.sh | bash
```

```powershell
# Docker (Windows)
$env:PRAXIS_VERSION = "v0.1.0"; irm https://praxis.originhq.com/docker.ps1 | iex

# Native (Windows)
$env:PRAXIS_VERSION = "v0.1.0"; irm https://praxis.originhq.com/install.ps1 | iex
```

## Manual Docker Setup

If you prefer to clone and run Docker manually:

```bash
git clone https://github.com/originsec/praxis.git
cd praxis
docker compose up --build
```

This starts:
- **Praxis** (service + web) on port 8080
- **RabbitMQ** on ports 5672 (AMQP) and 15672 (management UI)
- **MCP server** on port 8585 (when enabled in Settings > MCP Server)
- **Claude Bridge CCRv1** on port 8586 (when enabled in Settings > Claude Bridge)
- **Claude Bridge CCRv2** on port 8587 (when enabled in Settings > Claude Bridge)

Open **http://localhost:8080** and you're in.

To run without the web UI (headless mode for CLI-only usage):

```bash
PRAXIS_HEADLESS=1 docker compose up --build
```

### Getting the CLI from Docker

The CLI binary is built into the Docker image and copied to the data volume on startup. Extract it with:

```bash
docker cp $(docker compose ps -q praxis):/app/praxis_cli ./praxis_cli
chmod +x ./praxis_cli
./praxis_cli
```

> **Note:** Run this from the directory containing your `docker-compose.yml`. The container name varies by project directory.

To add a macOS node binary to Docker downloads, provide it explicitly (optional):

```bash
# Build macOS node binary on macOS
cargo build --release -p praxis_node

# Put it in a local directory
mkdir -p ~/.praxis/bin/nodes/macos-arm64
cp target/release/praxis_node ~/.praxis/bin/nodes/macos-arm64/praxis_node
```

Then mount it and enable multi-directory lookup:

```yaml
# docker-compose.override.yml
services:
  praxis:
    environment:
      PRAXIS_NODES_DIRS: /app/nodes,/app/nodes-host
    volumes:
      - ~/.praxis/bin/nodes:/app/nodes-host:ro

  praxis-postgres:
    environment:
      PRAXIS_NODES_DIRS: /app/nodes,/app/nodes-host
    volumes:
      - ~/.praxis/bin/nodes:/app/nodes-host:ro
```

This keeps Linux/Windows defaults unchanged while adding macOS as an opt-in download.

The RabbitMQ management UI at **http://localhost:15672** uses credentials `praxis/praxis`.

### Useful Docker Commands

```bash
# Run in background
docker compose up -d

# View logs
docker compose logs -f

# Stop everything
docker compose down

# Rebuild after code changes
docker compose up --build
```

## Building from Source

If you want to build natively or contribute to development:

### Prerequisites

- **Rust** 1.75+ (install via [rustup](https://rustup.rs/))
- **Node.js** 18+ (for the web frontend)
- **Docker** (for RabbitMQ, or install it separately)

### Build Steps

```bash
# Clone the repo
git clone https://github.com/originsec/praxis.git
cd praxis

# Build everything
cargo build --release
```

This produces four binaries in `target/release/`:
- `praxis_service` - the backend service
- `praxis_web` - the HTTP/WebSocket server + frontend
- `praxis_node` - the node agent
- `praxis_cli` - the command-line interface

### Running

You'll need RabbitMQ running first:

```bash
docker run -d --name rabbitmq \
  -p 5672:5672 -p 15672:15672 \
  -e RABBITMQ_DEFAULT_USER=praxis \
  -e RABBITMQ_DEFAULT_PASS=praxis \
  rabbitmq:3-management
```

Then start the service and web components:

```bash
./target/release/praxis_service &
./target/release/praxis_web &
```

If you used the install script on Linux, the service and web components are managed via systemd user services:

```bash
# Start/stop
systemctl --user start praxis
systemctl --user stop praxis

# Check status
systemctl --user status praxis

# View logs
journalctl --user -u praxis-service
journalctl --user -u praxis-web
```

Praxis starts automatically on login. Edit `~/.config/praxis/env` to configure the RabbitMQ URL and other environment variables.

## Getting Node Binaries

Nodes need to run on target systems. You have a few options:

### From the Web UI

If you're using Docker, precompiled node binaries are bundled with the image. Go to **Settings** → **Service** and download the Linux or Windows binary.

### From GitHub Releases

Each tagged release publishes node binaries for Linux and Windows:

- [Latest Release](https://github.com/originsec/praxis/releases/latest)
- `praxis_node-linux-x86_64` - Linux binary
- `praxis_node-windows-x86_64.exe` - Windows binary
- `praxis_node-macos-arm64` - macOS (Apple Silicon) binary

### Building Yourself

```bash
# Linux (native)
cargo build --release -p praxis_node

# macOS (Apple Silicon, native)
cargo build --release -p praxis_node

# Windows (cross-compile from Linux)
# Requires: rustup target add x86_64-pc-windows-gnu
# Requires: mingw-w64 toolchain
cargo build --release -p praxis_node --target x86_64-pc-windows-gnu
```

## Running Nodes

Once you have a node binary, run it on the target system:

```bash
# Linux
chmod +x praxis_node
./praxis_node

# Windows
praxis_node.exe
```

By default, nodes connect to RabbitMQ at `localhost:5672`. To connect to a remote service:

```bash
# Linux
PRAXIS_RABBITMQ_URL=amqp://praxis:praxis@your-server:5672 ./praxis_node

# Windows (PowerShell)
$env:PRAXIS_RABBITMQ_URL = "amqp://praxis:praxis@your-server:5672"
.\praxis_node.exe
```

## Version Compatibility

**Nodes must match the service version.** The RabbitMQ message format can change between versions, so a v0.2 node talking to a v0.1 service might not work correctly.

If you're getting strange errors or nodes aren't showing up, check that versions match.

## Next Steps

Once you have the service running and at least one node connected:

1. [Configure LLM providers](./configuration.md) - needed for semantic features
2. [Walk through the Quick Start](./quick-start.md) - see the basic workflow
