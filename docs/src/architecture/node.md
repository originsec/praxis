# Node Architecture

The node is the component that runs on target systems. It's responsible for all local interactions with AI agents.

## Overview

```diagram
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
│  │ │ Connector  │ │  │ │  TPROXY    │ │                      │
│  │ └────────────┘ │  │ ├────────────┤ │                      │
│  └────────────────┘  │ │   Hosts    │ │                      │
│                      │ └────────────┘ │                      │
│                      └────────────────┘                      │
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

The agent registry manages all supported agent connectors. On startup the
registry is built via `rebuild()` which:

1. Creates native agents from the factory (currently unused; all agents are Lua-based)
2. Loads Lua connectors from the service (delivered in the `RegistrationAck` message)
3. Falls back to embedded Lua scripts if no service scripts are provided

The service includes all stored Lua scripts in the `NodeRegistrationAck` sent
to the node's direct queue during registration. This avoids a race condition
where a fanout broadcast could arrive before the node's exchange consumer is
ready. On re-registration (e.g. after connection loss), scripts are also
delivered via the ack.

Subsequent script changes (add/edit/delete via the web UI) are broadcast to
nodes via `AgentRegistryUpdate` on the fanout exchange.

Updates are session-gated: if a session is open when an update arrives, it is
queued and applied after the session closes. If multiple updates arrive while a
session is open, only the latest is kept.

### Fingerprint Caching

Fingerprinting runs `--version` on each agent binary to verify availability and
extract the version string. Results are cached for 60 seconds when the agent is
available. Unavailable agents (not installed) are re-checked on every cycle so
they are discovered as soon as they appear.

### Development Builds

In debug builds, `PRAXIS_IGNORE_SERVICE_AGENTS=1` (the default) causes the node
to ignore service-pushed scripts and use only embedded Lua scripts. Set to `0`
to test with service-managed scripts.

## Intercept Manager

The intercept manager handles traffic capture. It supports four methods:

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

### TPROXY Mode (Linux)

Uses iptables TPROXY for transparent interception:

1. Intercept domains resolved to IP addresses
2. iptables mangle rules mark packets to target IPs
3. Policy routing directs marked packets to loopback
4. TPROXY redirects packets to proxy
5. Proxy uses `SO_ORIGINAL_DST` to get real destination

This provides kernel-level interception without a TUN device.

### Certificate Authority

All methods use a generated CA:

1. Root CA created with unique key
2. Root cert installed in system trust store
3. Leaf certificates generated per domain
4. TLS termination with valid-looking certs

## Multi-User Support

When the node runs as root, it provides multi-user support:

### User Enumeration

The node scans all user home directories (`/home/*` and `/root`) to discover:
- Agent configurations (e.g., `.claude/`, `.gemini/`, `.codex/`)
- Project directories with agent config files
- Session history files

This allows a single node running as root to manage agents across all users on the system.

### User-Aware Session Execution

When a session is created with a working directory owned by a non-root user, the node automatically:

1. Determines the directory owner's uid/gid
2. Sets the `HOME` environment variable to the user's home directory
3. Spawns the agent process as that user

This ensures the agent:
- Has appropriate file permissions for the project
- Reads its config from the correct user's home directory
- Creates files owned by the correct user

### Security Considerations

- Path validation ensures file operations stay within valid home directories
- Config file access is restricted to enumerated user homes
- The node validates all paths before reading or writing

## Session Management

Sessions allow direct interaction with agents:

### CLI Agents (PTY)

1. PTY created for the agent process
2. Agent spawned with appropriate flags (and as appropriate user when running as root)
3. Prompts written to stdin
4. Responses read from stdout
5. Output parsed and returned

### CLI Agents (ACP)

Agents that support the [Agent Client Protocol](https://agentclientprotocol.com/) (Cursor, Gemini) use a long-lived subprocess with JSON-RPC 2.0 over NDJSON stdio instead of PTY. The node uses the `agent-client-protocol` crate's `ClientSideConnection` for typed, async communication:

1. Agent spawned with ACP flag (e.g. `cursor-agent acp`, `gemini --acp`) via `tokio::process::Command`
2. `ClientSideConnection` established over the subprocess stdin/stdout
3. Initialize handshake via typed `InitializeRequest`/`InitializeResponse`
4. Prompts sent via typed `PromptRequest`, responses received as `PromptResponse` with `StopReason`
5. Real-time streaming updates (`SessionUpdate` variants: text chunks, tool calls, tool results, plans) delivered via the `Client` trait's `session_notification` callback
6. Permission requests handled via the `Client` trait's `request_permission` callback
7. Cancellation via `CancelNotification`

The connection runs on a dedicated thread with a `LocalSet` (since `ClientSideConnection` is `!Send`). An `AcpHandle` provides a `Send`-safe interface for the Lua runtime via channels.

### Browser-based Agents

1. App with webview launched with debugging enabled (on a hidden desktop in release builds; visible in debug builds by default)
2. CDP connection established via chromiumoxide
3. Prompts injected via DOM manipulation (InsertText + Enter)
4. Responses polled from page via JavaScript evaluation
5. Abort kills the entire process tree; Drop safety net cleans up even on Lua errors

### Session Context

Sessions are created with:
- **Working directory** - where the agent operates
- **YOLO mode** - auto-approve tool calls
- **Interactive** - whether permission requests should be forwarded to the user (TUI/web) or auto-denied (MCP/orchestrator)

## Terminal Manager

Provides PTY terminal access to the target system:

1. Shell spawned (bash/zsh/powershell)
2. PTY handles input/output
3. Terminal data streamed to web UI
4. Supports resize, Ctrl+C, etc.

## Message Handling

The node speaks two protocols over RabbitMQ. **Agent and session interaction
use ACP (Agent Client Protocol).** Everything else — intercept, terminal,
config, registration — uses the bespoke `NodeCommand` envelope.

### ACP (node-as-agent)

The node runs its own ACP server (`node/src/acp_server/`) and appears to the
service as a single ACP-speaking agent. The service forwards client ACP
frames to the node over RabbitMQ via `NodeDirectMessage::Acp(AcpFrame)`;
responses and notifications flow back via `NodeSignalMessage::Acp`.

Standard ACP methods supported:

- `initialize` — capability handshake. The node advertises the connector
  catalog and supported extensions in `InitializeResponse._meta`:
  ```json
  {
    "extensions": { "_praxis/recon": { "version": 1 } },
    "connectors": [ { "shortName": "claude-code", "name": "Claude Code" }, ... ],
    "nodeId": "..."
  }
  ```
- `session/new` — create a session. The target connector is selected via
  `_meta.praxis.connector`. Session options (`yolo`, `promptTimeoutSecs`,
  `interactive`) also live under `_meta.praxis`:
  ```json
  {
    "cwd": "/path",
    "_meta": {
      "praxis": {
        "connector": "claude-code",
        "yolo": false,
        "promptTimeoutSecs": 600,
        "interactive": true
      }
    }
  }
  ```
- `session/prompt` — send a prompt to the named session.
- `session/cancel` — cancel an in-flight prompt.
- `session/close` — terminate and release the session's per-session Lua VM.
- `session/list` — enumerate live sessions on the node.

Multiple concurrent sessions are supported. Each session owns a freshly
instantiated Lua VM (loaded from connector bytecode compiled once at
connector-load time), so no Lua-level state leaks between sessions sharing
the same connector script.

### ACP extensions

All are agent-scoped custom ACP methods (no `session_id` required) and are
advertised in `InitializeResponse._meta.extensions`:

- `_praxis/recon` — reconnaissance. Params
  `{ "agent_short_name": string, "is_semantic": bool }`; returns a
  `ReconResult`. Replaces the legacy `NodeCommand::Agent(Recon)` /
  `Agent(ReconSemantic)` commands.
- `_praxis/read_file`, `_praxis/write_file`, `_praxis/grep_files` —
  agent-scoped file ops used by recon tooling and the orchestrator.
- `_praxis/write_session_content` — writes agent-session content through
  the connector's `write_session_content` hook so agents with virtual
  session stores can intercept the write.

### NodeCommand (non-agent concerns)

```rust
pub enum NodeCommand {
    Intercept(InterceptCommand),
    Terminal(TerminalCommand),
    Config(ConfigCommand),
    AgentRegistry(AgentRegistryCommand),
}
```

Agent and session interaction have moved off `NodeCommand` entirely. The
legacy `NodeCommand::Agent` and `NodeCommand::Session` variants — along
with `NodeSignalMessage::ReconResultUpdate` and `::SessionUpdate` — were
removed once the CLI, web frontend, service orchestrator, and MCP server
had all been ported to ACP.

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
- Live ACP sessions keyed by `session_id`, each with its own Lua VM and
  cancellation flag
- PTY handles
- Transaction tracking

## Node Reset

A node can be reset at any time via the UI, CLI (`node reset`), or MCP
(`node_reset`). Reset cancels all in-flight operations, closes sessions and
terminals, disables interception, and re-registers the node with the service
— equivalent to a clean restart without killing the process.

The reset signal is delivered on a dedicated RabbitMQ queue
(`Node_{id}_reset`) consumed by its own task. This guarantees the signal is
never blocked by a long-running command handler in the main event loop. When
the reset consumer receives a message it cancels a `CancellationToken` that
the main loop observes. Slow commands are also wrapped in `tokio::select!`
with this token so they abort at the next `.await` point.

After cleanup the runtime returns `RuntimeExit::Reset` and the main
reconnection loop immediately re-registers without the usual reconnect delay.

## Registration

When the node starts:

1. Generates unique node ID (or uses existing)
2. Collects system information
3. Runs agent fingerprinting
4. Sends registration to service
5. Begins processing commands

Periodic updates report current state to the service.
