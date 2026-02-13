# Web Architecture

The web component serves the frontend and provides the communication layer between browsers and the service.

## Overview

```diagram
┌───────────────────────────────────────────────────────────┐
│                       Web Component                       │
│                                                           │
│  ┌─────────────────────────────────────────────────────┐  │
│  │                  HTTP Server (Axum)                 │  │
│  │                                                     │  │
│  │   GET /        → Static files (React SPA)           │  │
│  │   GET /ws      → WebSocket upgrade                  │  │
│  │   GET /api/*   → API endpoints                      │  │
│  └─────────────────────────────────────────────────────┘  │
│                             │                             │
│  ┌──────────────────────────▼──────────────────────────┐  │
│  │                  WebSocket Handler                  │  │
│  │                                                     │  │
│  │   Client ◀───JSON Messages───▶ RabbitMQ             │  │
│  └─────────────────────────────────────────────────────┘  │
│                                                           │
│  ┌─────────────────────────────────────────────────────┐  │
│  │                   React Frontend                    │  │
│  │                                                     │  │
│  │   TypeScript + Tailwind + React Flow                │  │
│  │   (Embedded in binary)                              │  │
│  └─────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────┘
```

## HTTP Server

The web server is built with Axum and handles:

### Static Assets

The React frontend is compiled and embedded in the binary at build time. When you request `/`, you get the SPA.

### WebSocket Endpoint

`/ws` upgrades HTTP connections to WebSocket for real-time communication. Each connected browser gets:
- A unique client ID
- A dedicated RabbitMQ queue for responses
- State updates via broadcast exchange (fanout)

### API Endpoints

Minimal REST API for specific operations:
- `/api/health` - health check
- `/api/nodes` - node list (for programmatic access)

Most functionality uses WebSocket for bidirectional communication.

## WebSocket Handler

### Connection Lifecycle

1. Browser connects to `/ws`
2. Server generates client ID
3. Client registered with RabbitMQ (creates response queue)
4. Initial state sent to client
5. Bidirectional message flow begins
6. On disconnect, cleanup and queue deletion

### Message Flow

**Client → Server:**
```diagram
Browser → WebSocket → Handler → RabbitMQ (ClientSignal) → Service
```

**Server → Client:**
```diagram
Service → RabbitMQ (Client_{id} or ClientBroadcast exchange) → Handler → WebSocket → Browser
```

### Message Types

Messages are JSON-encoded variants of `ClientSignalMessage` and `ClientDirectMessage`:

```typescript
// Sent by client
interface ClientMessage {
  type: 'Command' | 'SemanticOpRun' | 'ChainRun' | ...;
  payload: {...};
}

// Received by client
interface ServerMessage {
  type: 'StateUpdate' | 'CommandResponse' | 'SemanticOpUpdate' | ...;
  payload: {...};
}
```

## React Frontend

### Technology Stack

- **React 18** with TypeScript
- **Tailwind CSS** for styling
- **React Flow** for chain builder visualization
- **xterm.js** for terminal emulation
- **Vite** for build tooling

### Application Structure

```
web/frontend/src/
├── components/       # Reusable UI components
├── pages/            # Page components
├── hooks/            # Custom React hooks
├── contexts/         # React context providers
├── utils/            # Utility functions
└── App.tsx           # Main application
```

### Key Components

**AppContext** - Global state management:
- Connected nodes and their state
- Selected node and agent
- WebSocket connection status
- Settings and configuration

**NodeList** - Sidebar showing all connected nodes and their agents.

**NodeDetailPage** - Shows node info and an agents table with columns for name, short name, version, and session status.

**AgentDetailPage** - Agent header with name, version, and session controls. Includes session interaction panel, recon results, and operation/chain runners.

**ReconPanel** - Displays reconnaissance results organized by category.

**SessionPanel** - Interactive session interface for sending prompts.

**ChainBuilder** - Visual workflow editor using React Flow.

**TrafficViewer** - Table and detail view of intercepted traffic.

**Terminal** - PTY terminal emulator using xterm.js.

### State Management

The frontend uses React Context for global state:

```typescript
interface AppState {
  nodes: Map<string, NodeState>;
  selectedNode: string | null;
  selectedAgent: string | null;
  settings: Settings;
  wsConnected: boolean;
}
```

State is primarily driven by `StateUpdate` messages from the service, keeping all clients in sync.

### Real-Time Updates

WebSocket messages trigger state updates:

1. `StateUpdate` arrives with all node data
2. Context updates state
3. Components re-render with new data

This means multiple browser tabs see the same state-select an agent in one tab, see it selected in another.

## Orchestrator

The Orchestrator is an AI-powered agent that can autonomously interact with the Praxis network. It connects to the built-in MCP SSE server as a client to access all Praxis tools dynamically.

### Architecture

```diagram
┌─────────────────────────────────────────────────────┐
│                   Orchestrator                       │
│                                                     │
│   LLM (Claude/GPT/etc)                              │
│      │                                              │
│      ▼                                              │
│   Tool Parser ──▶ Local Tools (wait, report_plan)   │
│      │                                              │
│      ▼                                              │
│   MCP Client ──SSE──▶ MCP Server (Service)          │
│                       └──▶ All Praxis tools         │
└─────────────────────────────────────────────────────┘
```

### How It Works

1. On session start, the Orchestrator connects to the MCP SSE server at `http://127.0.0.1:{port}/sse`
2. It fetches all available tools via `list_tools` and converts them to the AI tool format
3. Two local tools (`wait` and `report_plan`) are appended for sleep and plan tracking
4. The combined tool definitions are included in the system prompt
5. User prompts enter a tool-use loop: the LLM generates responses, tool calls are parsed and executed (local tools handled in-process, everything else delegated to the MCP server), and results fed back to the LLM
6. The MCP client connection is dropped when the session ends

### Prerequisites

- **MCP server must be enabled** in Settings > MCP Server
- **Orchestrator LLM must be configured** in Settings > LLM Providers > Feature Selection

### Tool Execution

Tools are stateless — each MCP tool call includes explicit parameters (e.g., `node` ID) rather than relying on selected-node context. The LLM manages passing the correct IDs based on previous tool results.

## Build Process

### Development

```bash
cd web/frontend
npm install
npm run dev  # Starts Vite dev server on :5173
```

The dev server proxies API requests to the running web component.

### Production

The frontend is built and embedded during `cargo build`:

1. `npm run build` produces static files
2. Build script embeds files in binary
3. Axum serves from embedded assets

To skip frontend build during development:
```bash
PRAXIS_SKIP_FRONTEND=1 cargo build
```

## Configuration

### Environment Variables

| Variable | Effect |
|----------|--------|
| `PRAXIS_NODES_DIR` | Directory with node binaries for download |
| `PRAXIS_SKIP_FRONTEND` | Skip frontend build |

### Ports

- Default HTTP/WebSocket port: 8080
- Can be changed via command line or environment

## Error Handling

### WebSocket Errors

- Connection drops handled with reconnection logic
- Stale state detected via sequence numbers
- Reconnect requests full state update

### API Errors

- HTTP errors returned as JSON with status codes
- WebSocket errors sent as error messages

## Security Considerations

- No authentication by default (intended for internal use)
- Should be behind firewall or VPN in production
- HTTPS can be configured via reverse proxy
