# Local Development

This guide is for **contributors** working on Praxis itself. To **install** Praxis, use the one-liner installer:

```bash
curl -fsSL https://praxis.originhq.com/install.sh | bash
```

See [Installation](../getting-started/installation.md) for all install options.

## Building from Source

### Prerequisites

- Rust 1.70+ with cargo, via [rustup](https://rustup.rs):
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
- RabbitMQ running locally
- Linux build dependencies (Debian/Ubuntu):
  ```bash
  sudo apt-get install -y build-essential pkg-config libssl-dev
  ```
  - `build-essential` provides `cc`/`make`/libc headers — without it the build fails with ``linker `cc` not found``.
  - `pkg-config` + `libssl-dev` let `openssl-sys` (pulled in via `native-tls`) find the system OpenSSL install.

### Build Steps

1. Clone the repository:
```bash
git clone https://github.com/originsec/praxis.git
cd praxis
```

2. Build the default workspace members:
```bash
cargo build --release
```

This builds the service, node, and TUI (`praxis`) components.

### Cross-Compiling the Node for Windows

To build `praxis_node.exe` for Windows targets from a Linux host:

```bash
sudo apt-get install -y mingw-w64
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu -p praxis_node
```

`mingw-w64` provides the `x86_64-w64-mingw32-gcc` cross-linker that cargo
uses for this target. The resulting binary is at
`target/x86_64-pc-windows-gnu/release/praxis_node.exe`. This is the same
toolchain the installer uses for `--with-win-node --src` (see
[Installation](../getting-started/installation.md)).

## Running Locally

### Start RabbitMQ

If not using Docker:

```bash
# Linux
sudo systemctl start rabbitmq-server
```

Create the praxis user:
```bash
rabbitmqctl add_user praxis praxis
rabbitmqctl set_permissions -p / praxis ".*" ".*" ".*"
```

### Start the Service

```bash
cargo run --release --bin praxis_service
```

The service starts and connects to RabbitMQ, creating necessary queues.

### Start a Node

For testing locally, run a node on your own machine:

```bash
cargo run --release --bin praxis_node
```

The node connects to RabbitMQ and registers with the service.

## Environment Variables

Configure via environment or `.env` file:

| Variable | Default | Description |
|----------|---------|-------------|
| `PRAXIS_RABBITMQ_URL` | `amqp://praxis:praxis@localhost:5672` | RabbitMQ connection |
| `PRAXIS_DATABASE_URL` | `~/.praxis/operations.db` | Database path |
| `RUST_LOG` | `info` | Log level |

## Database Options

SQLite is used by default with no configuration required.

For PostgreSQL or advanced configuration, see [Database Configuration](./database.md).

## Development Workflow

### Code Changes

1. Make changes to Rust code
2. Rebuild: `cargo build`
3. Restart affected component

### Testing

Run tests:
```bash
cargo test
```

### Logs

Adjust log verbosity:

```bash
RUST_LOG=debug cargo run --bin praxis_service
RUST_LOG=praxis_node::intercept=trace cargo run --bin praxis_node
```

## Common Issues

### RabbitMQ connection failed

- Verify RabbitMQ is running
- Check credentials match
- Ensure the `PRAXIS_RABBITMQ_URL` is correct

### Database errors

- Check file permissions for SQLite
- Verify PostgreSQL is running and accessible
- Check the connection URL format

### Node not appearing

- Verify the node connected to RabbitMQ
- Check node logs for errors
- Ensure service is running

## Multiple Nodes

You can run multiple nodes locally (useful for testing):

```bash
# Terminal 1
cargo run --bin praxis_node

# Terminal 2
cargo run --bin praxis_node
```

Each node gets a unique ID and appears separately in the TUI.

## Debugging

### Enable debug logging

```bash
RUST_LOG=debug cargo run --bin praxis_service
```

### Check RabbitMQ queues

Open http://localhost:15672 (praxis/praxis) to see queue activity.
