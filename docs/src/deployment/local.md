# Local Development

This guide covers running Praxis locally for development and testing.

## Quick Start with Docker

The fastest way to get running:

```bash
docker compose up --build
```

This starts:
- RabbitMQ on port 5672 (management UI on 15672)
- Praxis service and web on port 8080

Open http://localhost:8080 to access the UI.

### With PostgreSQL

For PostgreSQL instead of SQLite:

```bash
docker compose --profile postgres up --build
```

### Faster Builds

Skip praxis_node binaries when you only need the service and web components:

```bash
SKIP_NODE_BUILD=1 docker compose up --build
```

Use the `release-optimized` profile for fully optimized production builds (full LTO, single codegen unit — significantly slower):

```bash
CARGO_PROFILE=release-optimized docker compose up --build
```

## Building from Source

### Prerequisites

- Rust 1.70+ with cargo
- Node.js 18+ with npm
- RabbitMQ running locally

### Build Steps

1. Clone the repository:
```bash
git clone https://github.com/originsec/praxis.git
cd praxis
```

2. Build everything:
```bash
cargo build --release
```

This builds the service, web, and node components. The frontend is built automatically during `cargo build`.

### Skip Frontend Build

During development, you can skip the frontend build:

```bash
PRAXIS_SKIP_FRONTEND=1 cargo build
```

Then run the frontend dev server separately for hot reload:

```bash
cd web/frontend
npm install
npm run dev
```

The dev server proxies to the backend.

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

### Start the Web Component

```bash
cargo run --release --bin praxis_web
```

The web component serves the UI on http://localhost:8080.

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
| `PRAXIS_DATABASE_URL` | `~/.praxis_operations.db` | Database path |
| `RUST_LOG` | `info` | Log level |

## Database Options

SQLite is used by default with no configuration required.

For PostgreSQL or advanced configuration, see [Database Configuration](./database.md).

## Development Workflow

### Code Changes

1. Make changes to Rust code
2. Rebuild: `cargo build`
3. Restart affected component

For frontend changes with the dev server running, changes hot-reload automatically.

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

### Frontend not building

- Ensure Node.js is installed
- Run `npm install` in `web/frontend`
- Check for build errors

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

Each node gets a unique ID and appears separately in the UI.

## Debugging

### Enable debug logging

```bash
RUST_LOG=debug cargo run --bin praxis_service
```

### Check RabbitMQ queues

Open http://localhost:15672 (praxis/praxis) to see queue activity.

### Frontend debugging

Open browser dev tools. The React app logs useful debug information to the console.
