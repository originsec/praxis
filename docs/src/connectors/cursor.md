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

Sessions are created using the `cursor-agent` CLI:

```diagram
┌───────────────────────────────────────────────────────┐
│                      Praxis Node                      │
│                                                       │
│  cursor-agent create-chat                             │
│         │                                             │
│         └──▶ Returns chat_id                          │
│                                                       │
│  cursor-agent --resume <chat_id> -p <prompt>          │
│         │                                             │
│         └──▶ Executes prompt in existing chat         │
└───────────────────────────────────────────────────────┘
```

### Session Context

When creating a session, you can specify:

**Working Directory** - Where Cursor should operate. Passed via `--workspace <path>`.

**YOLO Mode** - When enabled, passes flags for auto-approval:
- `--force` - Auto-approve file changes
- `--approve-mcps` - Auto-approve MCP server connections
- `--browser` - Allow browser automation

### Session Creation

1. `cursor-agent create-chat` is called to create a new chat
2. The chat ID is extracted from stdout
3. Subsequent prompts use `--resume <chat_id>` to continue the conversation

### Transacting

Sending prompts works by:
1. Spawning `cursor-agent --resume <chat_id> -p` with the prompt on stdin
2. Using `--output-format text` for clean output parsing
3. Waiting for the process to complete
4. Returning the assistant's response from stdout

### Session Cleanup

When a session is closed, Praxis:
1. Aborts any in-progress transaction (kills the process tree)
2. Deletes the chat history folder at `~/.config/cursor/chats/<project_hash>/<chat_id>/`

The cleanup searches through project hash directories to find and remove the specific chat folder, ensuring no session history is left behind.

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
