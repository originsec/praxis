# Node Architecture

The node is the component that runs on target systems. It's responsible for all local interactions with AI agents.

## Overview

```
┌──────────────────────────────────────────────────────────────┐
│                            Node                              │
│                                                              │
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐  │
│  │ Agent Registry │  │ Intercept Mgr  │  │  Terminal Mgr  │  │
│  │                │  │                │  │                │  │
│  │ ┌────────────┐ │  │ ┌────────────┐ │  │ ┌────────────┐ │  │
│  │ │ Connector  │ │  │ │   Proxy    │ │  │ │    PTY     │ │  │
│  │ ├────────────┤ │  │ ├────────────┤ │  │ └────────────┘ │  │
│  │ │ Connector  │ │  │ │  TUN/VPN   │ │  │                │  │
│  │ ├────────────┤ │  │ ├────────────┤ │  └────────────────┘  │
│  │ │ Connector  │ │  │ │   Hosts    │ │                      │
│  │ └────────────┘ │  │ └────────────┘ │                      │
│  └────────────────┘  └────────────────┘                      │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐  │
│  │               Runtime / Message Handler                │  │
│  └────────────────────────────────────────────────────────┘  │
│                              │                               │
│                         RabbitMQ                             │
└──────────────────────────────┼───────────────────────────────┘
                               │
                          To Service
```

## Agent Registry

The agent registry manages all supported agent connectors. On startup:

1. Factory creates instances of all connector types
2. Each connector runs fingerprinting
3. Successfully fingerprinted agents are registered
4. Registry is reported to service

```rust
// From factory.rs
pub fn create_all_agents(&self) -> Vec<Arc<dyn Agent>> {
    let mut agents = Vec::new();
    agents.push(Arc::new(ClaudeCodeAgent::new()));
    agents.push(Arc::new(GeminiAgent::new()));
    #[cfg(target_os = "linux")]
    agents.push(Arc::new(CodexAgent::new()));
    #[cfg(windows)]
    agents.push(Arc::new(M365CopilotAgent::new()));
    agents
}
```

## Intercept Manager

The intercept manager handles traffic capture. It supports three methods:

### Proxy Mode

Configures system proxy settings to route HTTP/HTTPS through a local proxy:

- **Linux**: Sets `HTTP_PROXY` and `HTTPS_PROXY` environment variables
- **Windows**: Modifies registry proxy settings

The proxy terminates TLS using a generated root CA, captures traffic, then re-encrypts and forwards to the actual destination.

### VPN Mode

Creates a TUN adapter and routes specific IPs through it:

1. TUN device created (wintun on Windows, tun crate on Linux)
2. Intercept domains resolved to IP addresses
3. Routes added through the TUN interface
4. Packet engine performs NAT to redirect to local proxy

This captures traffic even from applications that ignore proxy settings.

### Hosts Mode

Modifies the hosts file to redirect domains to localhost:

- Adds entries for intercept domains
- Proxy listens and handles redirected traffic
- Simpler but less flexible than VPN mode

### Certificate Authority

All methods use a generated CA:

1. Root CA created with unique key
2. Root cert installed in system trust store
3. Leaf certificates generated per domain
4. TLS termination with valid-looking certs

## Session Management

Sessions allow direct interaction with agents:

### CLI Agents

1. PTY created for the agent process
2. Agent spawned with appropriate flags
3. Prompts written to stdin
4. Responses read from stdout
5. Output parsed and returned

### Browser-based Agents

1. App with webview launched with debugging enabled
2. CDP connection established
3. Prompts injected via DOM manipulation
4. Responses extracted from page

### Session Context

Sessions are created with:
- **Working directory** - where the agent operates
- **YOLO mode** - auto-approve tool calls

## Terminal Manager

Provides PTY terminal access to the target system:

1. Shell spawned (bash/zsh/powershell)
2. PTY handles input/output
3. Terminal data streamed to web UI
4. Supports resize, Ctrl+C, etc.

## Message Handling

The runtime processes messages from the service:

```rust
pub enum NodeCommand {
    Agent(AgentCommand),      // Agent operations
    Session(SessionCommand),  // Session management
    Intercept(InterceptCommand), // Interception control
    Terminal(TerminalCommand),   // Terminal operations
    Config(ConfigCommand),       // Configuration
    AgentDiscovery(AgentDiscoveryCommand), // Discovery
}
```

### Agent Commands

- `Update` - refresh agent information
- `Select` - select an agent for operations
- `Recon` - perform static reconnaissance
- `ReconSemantic` - perform semantic reconnaissance
- `UpdateConfigFile` - modify agent config
- `GetSessionContent` - retrieve session history
- `GetConfigContent` - retrieve config file contents

### Session Commands

- `Create` - start a new session
- `Close` - end the session
- `Prompt` - send a prompt
- `CancelTransaction` - cancel pending operation

### Intercept Commands

- `Enable` - start interception with specified method
- `Disable` - stop interception and cleanup

## State Management

The node is mostly stateless-it reports to the service but doesn't persist data locally. However, some state is maintained:

### Intercept State

Saved to disk for crash recovery:
- Active interception method
- Installed certificate info
- Modified system settings

On restart, the node cleans up stale state.

### Session State

Kept in memory:
- Active session per agent
- PTY handles
- Transaction tracking

## Registration

When the node starts:

1. Generates unique node ID (or uses existing)
2. Collects system information
3. Runs agent fingerprinting
4. Sends registration to service
5. Begins processing commands

Periodic updates report current state to the service.
