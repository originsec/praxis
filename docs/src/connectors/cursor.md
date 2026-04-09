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

In the web UI, HTTP/2 traffic appears grouped by URL (similar to WebSocket), with individual frames expandable to view payloads.

## Session Management

Sessions use the Agent Communication Protocol (ACP) -- a JSON-RPC 2.0 protocol over NDJSON stdio that provides real-time streaming updates during prompt execution.

```diagram
┌───────────────────────────────────────────────────────┐
│                      Praxis Node                      │
│                                                       │
│  cursor-agent acp                                     │
│         │                                             │
│         ├──▶ ACP initialize handshake                 │
│         ├──▶ session/create → session_id              │
│         ├──▶ session/prompt → streaming updates       │
│         └──▶ session/close → cleanup                  │
└───────────────────────────────────────────────────────┘
```

### Session Context

When creating a session, you can specify:

**Working Directory** - Where Cursor should operate.

**YOLO Mode** - When enabled, tool permission requests are auto-approved.

**Interactive Mode** - When set (TUI or web sessions), permission requests are forwarded to the user for approval. Non-interactive sessions (MCP, orchestrator) auto-deny permission requests.

### Session Creation

1. `cursor-agent acp` is spawned as a long-lived subprocess
2. An ACP initialize handshake establishes the connection
3. `session/create` creates a new session with the working directory

### Transacting

Sending prompts works via ACP streaming:
1. A `session/prompt` request is sent with the prompt text
2. The agent streams back real-time updates: text chunks, tool calls, tool results, and permission requests
3. The response is assembled from the streamed chunks and returned

### Cancellation

Sessions support mid-prompt cancellation:
1. A cancel signal is sent to the ACP client
2. A `session/cancel` request tells the agent to stop
3. Stale responses are drained to prevent cross-prompt data leakage
4. Any partial output is preserved in the conversation

### Session Cleanup

When a session is closed, Praxis sends `session/close` via ACP, then terminates the subprocess.

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
