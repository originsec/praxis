# Local Development

This guide is for **contributors** working on Praxis itself. To **install** Praxis, use the one-liner installer:

```bash
curl -fsSL https://praxis.originhq.com/install.sh | bash
```

See [Installation](../getting-started/installation.md) for all install options.

## Building from Source

### Prerequisites

- Rust 1.70+ with cargo
- RabbitMQ running locally

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

This builds the service, node, and CLI components. The web component is
not part of the default build; use the TUI (`praxis`) as the client.

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
