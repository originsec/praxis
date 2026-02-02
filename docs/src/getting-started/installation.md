# Installation

There are a few ways to get Praxis running. Docker is the easiest for getting started; building from source gives you more control.

## Docker (Recommended)

Docker handles all the dependencies and gives you everything in one command:

```bash
git clone https://github.com/originsec/praxis.git
cd praxis
docker compose up --build
```

This starts:
- **Praxis** (service + web) on port 8080
- **RabbitMQ** on ports 5672 (AMQP) and 15672 (management UI)

Open **http://localhost:8080** and you're in.

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

This produces three binaries in `target/release/`:
- `praxis_service` - the backend service
- `praxis_web` - the HTTP/WebSocket server + frontend
- `praxis_node` - the node agent

### Running

You'll need RabbitMQ running first:

```bash
docker run -d --name rabbitmq \
  -p 5672:5672 -p 15672:15672 \
  -e RABBITMQ_DEFAULT_USER=praxis \
  -e RABBITMQ_DEFAULT_PASS=praxis \
  rabbitmq:3-management
```

Then start the service and web components (in separate terminals or backgrounded):

```bash
# Terminal 1: Service
./target/release/praxis_service

# Terminal 2: Web
./target/release/praxis_web
```

## Getting Node Binaries

Nodes need to run on target systems. You have a few options:

### From the Web UI

If you're using Docker, precompiled node binaries are bundled with the image. Go to **Settings** → **Service** and download the Linux or Windows binary.

### From GitHub Releases

Each tagged release publishes node binaries for Linux and Windows:

- [Latest Release](https://github.com/originsec/praxis/releases/latest)
- `praxis_node-linux-x86_64` - Linux binary
- `praxis_node-windows-x86_64.exe` - Windows binary

### Building Yourself

```bash
# Linux (native)
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
