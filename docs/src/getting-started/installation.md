# Installation

The Praxis service runs only on Linux — natively (systemd) or inside a Docker container. The CLI (TUI) runs natively on every supported platform. The one-liner installers walk you through how you want the service deployed; the CLI is always built natively.

> The Praxis service is **Linux-only**. **Windows and macOS** can only run it in **Docker** — there is no native service path on either. **Linux** can run it natively (systemd) or in Docker; Docker is offered there as an alternative when you'd rather not install RabbitMQ + systemd units on the host.

## Quick Install (One-Liner)

### Linux / macOS

```bash
curl -fsSL https://praxis.originhq.com/install.sh | bash
```

The installer asks how to install the service:

- **Native install** *(Linux only)* — installs the binaries to `/usr/local/bin`, the `praxis-service.service` systemd unit to `/etc/systemd/system`, config to `/etc/praxis/env`, and data to `/var/lib/praxis`. Requires a running RabbitMQ broker; the installer creates the `praxis` RabbitMQ user automatically.
- **Docker install** *(Linux + macOS)* — clones the repo into `~/.praxis-docker` and runs `docker compose up --build -d`. The Praxis container runs systemd as PID 1, so `praxisctl` works the same inside the container as on a native install. Pick this on macOS because there's no native option, or on Linux if you don't want to install RabbitMQ + systemd units on the host.
- **Client only** — only installs the `praxis` CLI (TUI); no service is deployed.

The CLI is always installed natively regardless of the choice.

For non-interactive use:

```bash
curl -fsSL https://praxis.originhq.com/install.sh | bash -s -- --service native
curl -fsSL https://praxis.originhq.com/install.sh | bash -s -- --service docker
curl -fsSL https://praxis.originhq.com/install.sh | bash -s -- --cli
```

#### Cross-compiling the Windows node binary (optional)

Add `--with-win-node` to a native install to also cross-compile the Windows
`praxis_node.exe` and stage it next to the Linux node binary at
`/usr/local/share/praxis/nodes/praxis_node_windows.exe`. Useful when the
service needs to deploy nodes to Windows targets without pulling them from
a release.

```bash
curl -fsSL https://praxis.originhq.com/install.sh | bash -s -- --service native --with-win-node
```

Requires `mingw-w64` and `rustup` (the rust target `x86_64-pc-windows-gnu`
is installed automatically). Install mingw-w64 with your distribution's
package manager:

- Debian/Ubuntu: `sudo apt-get install mingw-w64`
- Fedora/RHEL:   `sudo dnf install mingw64-gcc`
- Arch:          `sudo pacman -S mingw-w64-gcc`
- macOS:         `brew install mingw-w64`

The flag has no effect with `--cli`, `--service docker`, or interactive
mode — for those, use `praxis-bin` (AUR) or download the Windows node
binary from the GitHub release if you need it.

### Windows

The Praxis service is Linux-only, so on Windows the installer runs the service in **Docker** — that's the only option for the service on Windows. The CLI (TUI) is always installed natively (compiled from source, requires Rust + git).

```powershell
irm https://praxis.originhq.com/install.ps1 | iex
```

The installer asks how you want to install the service:

- **Docker install** — runs the Praxis container alongside RabbitMQ
- **Client only** — only installs the `praxis.exe` CLI; no service

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
- `/usr/local/bin/praxis_cli` — CLI binary
- `/usr/local/bin/praxis` — symlink to `praxis_cli` (preferred command name)
- `/usr/local/bin/praxisctl` — service control utility
- `/usr/local/share/praxis/nodes/praxis_node_linux` — node agent
- `/etc/systemd/system/praxis-service.service` — system-wide systemd unit
- `/etc/praxis/env` — service config (`PRAXIS_RABBITMQ_URL`, etc.)
- `/var/lib/praxis/` — data directory (SQLite database lives here by default)
- A dedicated `praxis` system user runs the service

The web UI is **not** installed by either the script or the docker image — manage and use Praxis through the `praxis` TUI.

### What docker install lays down

The repo is cloned into `~/.praxis-docker`. `docker compose` brings up two services:

- **rabbitmq** — `rabbitmq:3-management` with the `praxis` user pre-created
- **praxis** — Praxis container running systemd as PID 1; `praxisctl` works inside the container

The MCP server and Claude bridges are exposed on ports 8585, 8586, and 8587. There is no web UI in the docker image.

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

# Configuration
praxisctl set-rabbitmqurl amqp://praxis:praxis@localhost:5672
praxisctl get-rabbitmqurl
praxisctl config show
praxisctl config edit       # opens /etc/praxis/env in $EDITOR
```

`praxisctl` re-execs itself under `sudo` when run by an unprivileged user.

Inside the docker install, the same commands work via `docker compose`:

```bash
cd ~/.praxis-docker
docker compose exec praxis praxisctl status
docker compose exec praxis praxisctl set-rabbitmqurl amqp://praxis:praxis@rabbitmq:5672
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

## Getting Node Binaries

A native install lays down `praxis_node_linux` at `/usr/local/share/praxis/nodes/`. To also stage the Windows node binary alongside it, use `--with-win-node` (see above).

The `praxis-bin` AUR package ships both `praxis_node_linux` and `praxis_node_windows.exe` automatically. The same two binaries are available as standalone assets on every [GitHub Release](https://github.com/originsec/praxis/releases/latest).

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
