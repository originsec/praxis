# Cursor Agent Connector

The Cursor connector enables interaction with Cursor's background agent CLI.

## Overview

Cursor Agent is Cursor's command-line interface for AI-assisted coding. It provides similar functionality to the Cursor IDE but in a headless CLI form. The connector is Linux-only.

## Fingerprinting

The connector looks for the Cursor agent CLI by checking:

1. **PATH search** - Finding the `cursor-agent` executable in PATH
2. **Explicit paths** - Checking known installation locations:
   - `/usr/bin/cursor-agent`
   - `~/.local/bin/cursor-agent`

If found, fingerprinting succeeds and the agent appears in the node's agent list.

## Interception

Traffic is intercepted for the following domains:
- `api.cursor.sh`
- `agent.api5.cursor.sh`
- `api2.cursor.sh`
- `cursor.sh`

The proxy supports subdomain matching, so any subdomain of `cursor.sh` will be intercepted.

When interception is enabled, you'll see:
- Prompts sent to the Cursor API
- Responses including assistant messages
- Tool calls and results

### HTTP/2 and gRPC Support

Cursor uses HTTP/2 with gRPC for its streaming API (e.g., `/agent.v1.AgentService/Run`). The proxy fully supports HTTP/2 frame-level interception:

- **Frame types captured**: HEADERS, DATA, SETTINGS, GOAWAY, etc.
- **Traffic entries**: Logged as `H2_HEADERS` and `H2_DATA` methods
- **Stream tracking**: Extracts `:path` from HPACK headers for URL context
- **Bidirectional**: Both request and response frames are captured

In the praxis TUI's Intercept window, HTTP/2 traffic appears grouped by URL (similar to WebSocket), with individual frames expandable to view payloads.

## Session Management

Sessions use the [Agent Client Protocol](https://agentclientprotocol.com/) (ACP) -- a JSON-RPC 2.0 protocol over NDJSON stdio. Praxis uses the `agent-client-protocol` crate's `ClientSideConnection` for typed, async communication.

```diagram
┌───────────────────────────────────────────────────────┐
│                      Praxis Node                      │
│                                                       │
│  cursor-agent acp                                     │
│         │                                             │
│         ├──▶ initialize (InitializeRequest)           │
│         ├──▶ session/new → session_id + models        │
│         ├──▶ session/prompt → streaming updates       │
│         └──▶ session/close → cleanup                  │
└───────────────────────────────────────────────────────┘
```

### Session Context

When creating a session, you can specify:

**Working Directory** - Where Cursor should operate.

**YOLO Mode** - When enabled, tool permission requests are auto-approved.

**Interactive Mode** - When set (TUI sessions), permission requests are forwarded to the user for approval. Non-interactive sessions (MCP, orchestrator) auto-deny permission requests.

### Session Creation

1. `cursor-agent acp` is spawned as an async subprocess via `tokio::process::Command`
2. `ClientSideConnection` established over stdin/stdout
3. `InitializeRequest` handshake establishes the connection and negotiates capabilities
4. `NewSessionRequest` creates a session with the working directory

### Transacting

Sending prompts uses typed ACP requests:
1. A `PromptRequest` is sent with the prompt text as `ContentBlock::Text`
2. The agent streams back real-time `SessionUpdate` notifications: `AgentMessageChunk`, `ToolCall`, `ToolCallUpdate`, `Plan`, and `UsageUpdate`
3. Permission requests arrive via the `Client` trait's `request_permission` callback
4. The prompt completes with a `PromptResponse` containing a `StopReason`

### Cancellation

Sessions support mid-prompt cancellation:
1. A `CancelNotification` is sent to the agent
2. The agent responds to the original `PromptRequest` with `StopReason::Cancelled`
3. Any partial output is preserved in the conversation

### Session Cleanup

When a session is closed, Praxis sends `CloseSessionRequest` via ACP, then terminates the subprocess.

## Files and Paths

**Session History**

| Location | Path | Content |
|----------|------|---------|
| Chat history | `~/.config/cursor/chats/<project_hash>/<chat_id>/` | Session files |

**Binary Locations**

| Platform | Paths Checked |
|----------|---------------|
| Linux | `/usr/bin/cursor-agent`, `~/.local/bin/cursor-agent`, PATH |

## Troubleshooting

### "Agent not fingerprinted"

- Ensure `cursor-agent` is installed
- Verify the command is in PATH or at a known location
- Check file permissions

### "Session creation failed"

- Verify `cursor-agent create-chat` works from terminal
- Check that Cursor is authenticated
- Look at node logs for detailed errors

### "Traffic not appearing"

- Ensure interception is enabled
- Check that the proxy is using VPN or TPROXY mode (not system proxy)
- Verify HTTP/2 traffic is being captured (check for H2_DATA entries)

### "HTTP/2 connection issues"

- The proxy handles HTTP/2 frame-level interception automatically
- If traffic appears but the agent fails, check for certificate trust issues
- gRPC streaming is supported - both directions are captured
