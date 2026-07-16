# Architecture Overview

Praxis has a distributed architecture designed for monitoring and controlling AI agents across multiple systems. Let's walk through how the pieces fit together.

## The Big Picture

```diagram
                              ┌─────────────────┐
                              │   praxis TUI    │
                              │  (terminal UI)  │
                              └────────┬────────┘
                                       │ RabbitMQ (AMQP)
                              ┌────────▼────────┐
                              │    Service      │
                              │  (Backend)      │
                              └────────┬────────┘
                                       │ RabbitMQ (AMQP)
              ┌────────────────────────┼────────────────────────┐
              │                        │                        │
       ┌──────▼──────┐          ┌──────▼──────┐          ┌──────▼──────┐
       │    Node     │          │    Node     │          │    Node     │
       │ (Target A)  │          │ (Target B)  │          │ (Target C)  │
       └─────────────┘          └─────────────┘          └─────────────┘
```

## Components

### Node

The node runs on target systems where AI agents are installed. It's the "eyes and hands" of Praxis on each endpoint.

**What it does:**
- Fingerprints installed agents
- Performs reconnaissance on agent configurations and sessions
- Intercepts traffic between agents and LLM backends
- Creates and manages sessions with agents
- Provides PTY terminal access to the system

**Key characteristics:**
- Stateless - all persistent data lives on the service
- Single binary, no dependencies
- Communicates with service over RabbitMQ

See [Node Architecture](./node.md) for details.

### Service

The service is the central backend that coordinates everything.

**What it does:**
- Tracks all connected nodes and their agents
- Stores configuration, operation definitions, and chain workflows
- Manages the semantic operations queue
- Executes chains by orchestrating multi-step workflows
- Persists intercepted traffic and recon results
- Handles LLM provider integrations

**Key characteristics:**
- Persistent storage (SQLite default, PostgreSQL for production)
- Stateful - knows about all nodes and their state
- Runs the operation manager and chain executor

See [Service Architecture](./service.md) for details.

## Communication

### No direct client↔node traffic

The service is the only component that talks to nodes. Clients (the
praxis TUI and external ACP tools) speak to the **service**; the service
forwards to the relevant node over RabbitMQ. This keeps access control,
session routing, and request correlation in one place and means node
failure modes never leak into clients.

```
 praxis TUI ─▶ RabbitMQ ─▶ Service ─▶ RabbitMQ ─▶ Node
 External ACP client ─▶
```

### ACP (Agent Client Protocol)

Each node exposes a **single ACP server** (`node/src/acp_server/`) over
RabbitMQ. That one endpoint is how every local agent on the node is
driven — the connector to use is selected per-session via
`_meta.praxis.connector` on the `session/new` request. Multiple concurrent
sessions are supported on the same node, each with its own freshly-built
Lua VM.

The service-side proxy (`service/src/acp_node_proxy.rs`) routes frames:

- External client → service → `_meta.praxis.nodeId` → target node.
- Node → service → originating client (by correlated `client_id`).
- Service's internal orchestrator → node, using a `svc_*` pseudo-client-id
  so responses are consumed in-process instead of being forwarded.

Recon is a custom ACP extension (`_praxis/recon`) plus four file-op
extensions (`_praxis/read_file`, `_praxis/write_file`, `_praxis/grep_files`,
`_praxis/write_session_content`). The node advertises them in
`InitializeResponse._meta.extensions` along with the connector catalog.

### RabbitMQ

All communication between nodes, service, and clients flows through RabbitMQ:

| Queue | Direction | Purpose |
|-------|-----------|---------|
| `NodeSignal` | Node → Service | Registration, traffic, recon results, outbound ACP frames |
| `Node_{id}` | Service → Node | Commands, parser responses, inbound ACP frames |
| `NodeBroadcast` | Service → All Nodes | Refresh requests (fanout exchange) |
| `ClientSignal` | Client → Service | UI requests, inbound ACP frames |
| `Client_{id}` | Service → Client | Direct responses, outbound ACP frames |
| `ClientBroadcast` | Service → All Clients | State updates (fanout exchange) |

RabbitMQ provides:
- Reliable message delivery
- Decoupling between components
- Easy scaling (nodes can come and go)
- Persistence for messages in flight

Named queues (`NodeSignal`, `ClientSignal`, `Node_{id}`, `Client_{id}`, event-log
queues, etc.) are declared **durable** so their definitions survive broker
restarts and stay compatible with RabbitMQ 4.3+, which denies transient
non-exclusive classic queues by default. Per-connection broadcast queues remain
exclusive + auto-delete (not subject to that deprecation).

### Message Flow Example

Here's what happens when a CLI driver runs a prompt over ACP:

1. **CLI** (ACP proxy) → `ClientSignal` → **Service**
2. **Service** (`AcpNodeProxy`) sees `_meta.praxis.nodeId`, forwards the
   raw JSON-RPC frame via `Node_{id}` → **Node**
3. **Node** (`NodeAcpServer`) processes `session/new` / `session/prompt` /
   etc., running on a per-session Lua VM
4. **Node** emits response + `session/update` notifications on `NodeSignal`
5. **Service** (`AcpNodeProxy::forward_to_client`) routes them to the
   originating `Client_{id}` queue
6. **CLI** reads responses from its client queue and emits them on stdout

## Data Flow

### Intercepted Traffic

```diagram
Agent ─HTTPS─▶ Proxy ─▶ Node ─RabbitMQ─▶ Service ─▶ Database
                                           │
                                           └─RabbitMQ─▶ praxis TUI
```

### Operations

```diagram
praxis TUI ─▶ Service ─▶ LLM (planning)
                 │
                 └─▶ Node ─▶ Agent (execution)
                       │
                       └─▶ Output ─▶ Service ─▶ praxis TUI
```

## Database Schema

The service stores everything in a relational database:

- **config** - key-value settings (LLM configs, etc.)
- **operation_definitions** - saved operation templates
- **semantic_operations** - operation execution history
- **chain_definitions** - workflow definitions
- **chain_executions** - workflow execution history
- **traffic_log** - intercepted HTTP traffic
- **intercept_rules** - traffic matching rules
- **recon_results** - cached reconnaissance data
- **application_logs** - centralized logging (controlled by `application_logs_enabled`)

## Deployment Patterns

### Development

Single machine running everything:
- Docker Compose with service and RabbitMQ
- Node running locally for testing

### Production

Separate concerns:
- Service on central server
- RabbitMQ (possibly managed service)
- Nodes deployed to target systems
- PostgreSQL for the database

### Cloud (Azure)

See [Azure Deployment](../deployment/azure.md):
- Container Apps for the service
- Managed RabbitMQ or Container Instance
- Azure Database for PostgreSQL
