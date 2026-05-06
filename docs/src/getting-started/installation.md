# Installation

The Praxis service runs only on Linux (native or in a container). The CLI runs natively on every supported platform. The one-liner installers walk you through choosing components and an install mode.

## Quick Install (One-Liner)

### Linux / macOS

```bash
curl -fsSL https://praxis.originhq.com/install.sh | bash
```

The installer first asks which components to install:

- **service** — the Praxis backend service + web (managed via `praxisctl`)
- **cli** — the Praxis CLI, always built natively, installed as `praxis`

Then asks how to install the service:

- **Native install** *(Linux only)* — installs the binaries to `/usr/local/bin`, the systemd units to `/etc/systemd/system`, config to `/etc/praxis/env`, and data to `/var/lib/praxis`. Requires a running RabbitMQ broker; the installer creates the `praxis` RabbitMQ user automatically.
- **Docker install** *(Linux + macOS)* — clones the repo into `~/.praxis-docker` and runs `docker compose up --build -d`. The Praxis container runs systemd as PID 1, so `praxisctl` works the same inside the container as on a native install.

Use `↑`/`↓` (or `j`/`k`) to navigate, `Enter` to select, `Space` to toggle checkboxes, `q` to abort.

For non-interactive use:

```bash
curl -fsSL https://praxis.originhq.com/install.sh | bash -s -- --service native
curl -fsSL https://praxis.originhq.com/install.sh | bash -s -- --service docker
curl -fsSL https://praxis.originhq.com/install.sh | bash -s -- --cli
```

### Windows

The Praxis service is Linux-only, so on Windows it runs in Docker. The CLI is always installed natively (compiled from source, requires Rust + git).

```powershell
irm https://praxis.originhq.com/install.ps1 | iex
```

The installer asks which components to install:

- **service** — Docker only on Windows
- **cli** — native Windows build, installed as `praxis.exe`

Non-interactive:

```powershell
.\install.ps1 -Service docker
.\install.ps1 -Cli
.\install.ps1 -Remove
```

If Docker is not installed, install [Docker Desktop](https://www.docker.com/products/docker-desktop/) first. If Rust is missing, install it via [rustup](https://rustup.rs).

### Native install — RabbitMQ prerequisite

Native installs require RabbitMQ to be installed and running before the installer runs. The installer detects this and aborts with instructions if it can't find RabbitMQ.

```bash
# Debian/Ubuntu
sudo apt-get install rabbitmq-server
sudo systemctl enable --now rabbitmq-server

# Fedora/RHEL
sudo dnf install rabbitmq-server
sudo systemctl enable --now rabbitmq-server

# Arch
sudo pacman -S rabbitmq
sudo systemctl enable --now rabbitmq-server
```

The installer creates the `praxis` RabbitMQ user and grants it permissions automatically.

### What native install lays down (Linux)

- `/usr/local/bin/praxis_service` — backend service
- `/usr/local/bin/praxis_web` — web server + frontend
- `/usr/local/bin/praxis_cli` — CLI binary
- `/usr/local/bin/praxis` — symlink to `praxis_cli` (preferred command name)
- `/usr/local/bin/praxisctl` — service control utility
- `/usr/local/share/praxis/nodes/praxis_node_linux` — node agent
- `/etc/systemd/system/praxis-service.service` and `praxis-web.service` — system-wide systemd units
- `/etc/praxis/env` — service config (`PRAXIS_RABBITMQ_URL`, etc.)
- `/var/lib/praxis/` — data directory (SQLite database lives here by default)
- A dedicated `praxis` system user runs the services

### What docker install lays down

The repo is cloned into `~/.praxis-docker`. `docker compose` brings up two services:

- **rabbitmq** — `rabbitmq:3-management` with the `praxis` user pre-created
- **praxis** — Praxis container running systemd as PID 1; `praxisctl` works inside the container

The web UI, MCP server, and Claude bridges are exposed on the same ports as before (8080, 8585, 8586, 8587).

### Removing

```bash
# Linux/macOS — removes native install + docker install
curl -fsSL https://praxis.originhq.com/install.sh | bash -s -- --remove

# also wipes /etc/praxis and /var/lib/praxis
PRAXIS_REMOVE_DATA=1 curl -fsSL https://praxis.originhq.com/install.sh | bash -s -- --remove
```

```powershell
# Windows
iex "& { $(irm https://praxis.originhq.com/install.ps1) } -Remove"
```

### Pinning a Specific Version

```bash
# Linux/macOS
PRAXIS_VERSION=v0.10.0 curl -fsSL https://praxis.originhq.com/install.sh | bash
```

```powershell
# Windows
$env:PRAXIS_VERSION = "v0.10.0"; irm https://praxis.originhq.com/install.ps1 | iex
```

## Controlling the service — `praxisctl`

After a native (or docker) install, `praxisctl` is the single entry point for service lifecycle and configuration. It wraps `systemctl` and edits `/etc/praxis/env`.

```bash
# Service (praxis-service.service)
praxisctl start
praxisctl stop
praxisctl restart
praxisctl enable      # auto-start at boot
praxisctl disable
praxisctl status

# Web server (praxis-web.service)
praxisctl webserver start
praxisctl webserver stop
praxisctl webserver enable
praxisctl webserver disable
praxisctl webserver status

# Configuration
praxisctl set rabbitmq-url amqp://praxis:praxis@localhost:5672
praxisctl get rabbitmq-url
praxisctl config show
praxisctl config edit       # opens /etc/praxis/env in $EDITOR
```

`praxisctl` re-execs itself under `sudo` when run by an unprivileged user.

Inside the docker install, the same commands work via `docker compose`:

```bash
cd ~/.praxis-docker
docker compose exec praxis praxisctl status
docker compose exec praxis praxisctl webserver disable
docker compose exec praxis praxisctl set rabbitmq-url amqp://praxis:praxis@rabbitmq:5672
```

## Configuring the CLI — `praxis set-rabbitmqurl`

The `praxis` CLI reads its RabbitMQ URL from `~/.config/praxis/config` (key `PRAXIS_RABBITMQ_URL`) and falls back to `amqp://praxis:praxis@localhost:5672` if no config is set.

```bash
praxis set-rabbitmqurl amqp://praxis:praxis@my-server:5672
praxis config         # show effective URL and config file path
praxis                # launch the interactive TUI
praxis --status       # one-shot connection check
praxis -C "node list" # one-shot command
```

There is no `--rabbitmq` flag and no `PRAXIS_RABBITMQ_URL` environment variable on the CLI — point users at `praxis set-rabbitmqurl` instead.

## Manual Docker Setup

If you prefer to clone and run Docker by hand:

```bash
git clone https://github.com/originsec/praxis.git
cd praxis
docker compose up --build
```

This starts:

- **rabbitmq** — RabbitMQ on ports 5672 (AMQP) and 15672 (management UI, `praxis/praxis`)
- **praxis** — Praxis container with systemd as PID 1, exposing 8080 (web), 8585 (MCP), 8586/8587 (Claude bridges)

To run without the web UI (build-time):

```bash
PRAXIS_HEADLESS=1 docker compose up --build
```

To disable the web at runtime instead, leave it enabled at build time and use `praxisctl`:

```bash
docker compose exec praxis praxisctl webserver disable
docker compose exec praxis praxisctl webserver stop
```

### Getting the CLI from Docker

The CLI is built into the image. On Linux/macOS, install it natively via the installer (`--cli`); inside the container itself, `praxis` is already on `PATH`.

## Building from Source

```bash
git clone https://github.com/originsec/praxis.git
cd praxis
cargo build --release
```

This produces four binaries in `target/release/`:

- `praxis_service` — the backend service
- `praxis_web` — the HTTP/WebSocket server + frontend
- `praxis_node` — the node agent
- `praxis_cli` — the CLI

To stand up a development RabbitMQ:

```bash
docker run -d --name rabbitmq \
  -p 5672:5672 -p 15672:15672 \
  -e RABBITMQ_DEFAULT_USER=praxis \
  -e RABBITMQ_DEFAULT_PASS=praxis \
  rabbitmq:3-management
```

Then run the binaries directly, or install the systemd units from `pkg/systemd/` if you want them managed by `praxisctl`.

## Getting Node Binaries

Nodes need to run on target systems. Options:

- From the web UI under **Settings → Service**.
- From [GitHub Releases](https://github.com/originsec/praxis/releases/latest) — Linux, Windows, and macOS-arm64 binaries are published.
- Build manually:

```bash
cargo build --release -p praxis_node
# Cross-compile for Windows
rustup target add x86_64-pc-windows-gnu
cargo build --release -p praxis_node --target x86_64-pc-windows-gnu
```

## Running Nodes

```bash
chmod +x praxis_node
./praxis_node
```

By default, nodes connect to RabbitMQ at `localhost:5672`. Override per-node via the env var:

```bash
PRAXIS_RABBITMQ_URL=amqp://praxis:praxis@your-server:5672 ./praxis_node
```

## Version Compatibility

Nodes must match the service version. The RabbitMQ message format can change between versions, so a v0.2 node talking to a v0.1 service might not work correctly.

## Next Steps

1. [Configure LLM providers](./configuration.md)
2. [Walk through the Quick Start](./quick-start.md)
